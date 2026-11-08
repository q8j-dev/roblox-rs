use std::path::Path;

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

fn check(ok: bool, what: &str) -> Result<(), String> {
    if ok {
        println!("    ok: {what}");
        Ok(())
    } else {
        Err(format!("web output check failed: {what}"))
    }
}

pub fn run(root: &Path, options: &[String]) -> Result<(), String> {
    if !root.join("dist/web/mt/player.js").is_file()
        || !root.join("dist/web/st/player.js").is_file()
    {
        crate::ci::build_web(root, false)?;
    }
    println!("\n==> web test: checking dist/web");
    let page = read(root, "dist/web/index.html")?;
    check(
        page.contains("./mt/player.js") && page.contains("./st/player.js"),
        "page loads both builds",
    )?;
    check(
        page.contains("crossOriginIsolated"),
        "page picks the build by cross-origin isolation",
    )?;
    let index = read(root, "dist/web/content-index.txt")?;
    check(
        index
            .lines()
            .any(|l| l == "tests/places/RobloxStudio/content/FirstPlaceBasePlate.rbxl")
            && index.lines().any(|l| l.starts_with("assets/content/")),
        "content index lists places and content",
    )?;
    for variant in ["mt", "st"] {
        let js = read(root, &format!("dist/web/{variant}/player.js"))?;
        check(
            !js.contains("wasi_snapshot_preview1"),
            &format!("{variant} has no WASI imports"),
        )?;
        check(
            js.contains("export function run"),
            &format!("{variant} exports run"),
        )?;
        let wasm = std::fs::read(root.join(format!("dist/web/{variant}/player_bg.wasm")))
            .map_err(|e| e.to_string())?;
        check(
            wasm.starts_with(b"\0asm"),
            &format!("{variant} wasm is a module"),
        )?;
        check(
            !wasm.windows(15).any(|w| w == b"command_export\0"),
            &format!("{variant} exports are not wrapped in constructor calls"),
        )?;
    }
    let mt = read(root, "dist/web/mt/player.js")?;
    check(
        mt.contains("initThreadPool"),
        "threaded build exports the thread pool",
    )?;
    check(
        mt.contains("shared: true") || mt.contains("shared:true"),
        "threaded build uses shared memory",
    )?;
    println!("<== web test: output ok");
    match chrome::find() {
        Some(browser) => {
            let place = options
                .windows(2)
                .find(|w| w[0] == "--place")
                .map(|w| w[1].clone())
                .unwrap_or_else(|| {
                    "tests/places/RobloxStudio/content/FirstPlaceBasePlate.rbxl".into()
                });
            chrome::run(root, &browser, &place)
        }
        None => {
            println!("    no Chrome found (set CHROME), skipping the headless run");
            Ok(())
        }
    }
}

mod chrome {
    use base64::Engine;
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    use tungstenite::{Message, WebSocket};

    pub fn find() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("CHROME") {
            return Some(PathBuf::from(p));
        }
        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
        } else if cfg!(windows) {
            &[
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            ]
        } else {
            &[
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
            ]
        };
        candidates.iter().map(PathBuf::from).find(|p| p.is_file())
    }

    fn free_port() -> Result<u16, String> {
        let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
    }

    fn http_get(port: u16, path: &str) -> Option<String> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        write!(
            s,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = s.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&data);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(usize::MAX);
                if body.len() >= length {
                    return Some(body.to_string());
                }
            }
        }
        let text = String::from_utf8_lossy(&data).into_owned();
        text.split_once("\r\n\r\n").map(|(_, b)| b.to_string())
    }

    struct Cdp {
        ws: WebSocket<TcpStream>,
        next: u64,
        console: Vec<String>,
    }

    impl Cdp {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
            self.next += 1;
            let id = self.next;
            let msg = json!({"id": id, "method": method, "params": params}).to_string();
            self.ws
                .send(Message::text(msg))
                .map_err(|e| e.to_string())?;
            loop {
                let Message::Text(t) = self.ws.read().map_err(|e| e.to_string())? else {
                    continue;
                };
                let v: Value = serde_json::from_str(t.as_str()).map_err(|e| e.to_string())?;
                if v.get("id").and_then(Value::as_u64) == Some(id) {
                    if let Some(err) = v.get("error") {
                        return Err(err.to_string());
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
                if v.get("method").and_then(Value::as_str) == Some("Runtime.consoleAPICalled")
                    && let Some(args) = v.pointer("/params/args").and_then(Value::as_array)
                {
                    let text: Vec<String> = args
                        .iter()
                        .map(|a| {
                            a.get("value")
                                .map(|v| v.as_str().map(str::to_string).unwrap_or(v.to_string()))
                                .or_else(|| {
                                    a.get("description")
                                        .and_then(Value::as_str)
                                        .map(str::to_string)
                                })
                                .unwrap_or_default()
                        })
                        .collect();
                    self.console.push(text.join(" "));
                }
            }
        }

        fn eval(&mut self, expr: &str) -> Result<Value, String> {
            let r = self.call(
                "Runtime.evaluate",
                json!({"expression": expr, "returnByValue": true}),
            )?;
            Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
        }
    }

    fn launch(chrome: &Path, root: &Path, port: u16) -> Result<Child, String> {
        Command::new(chrome)
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--enable-unsafe-webgpu",
                "--enable-features=Vulkan,WebGPU",
                "--ignore-gpu-blocklist",
                "--window-size=1280,720",
                &format!("--remote-debugging-port={port}"),
                &format!(
                    "--user-data-dir={}",
                    root.join("target").join("web-test-profile").display()
                ),
                "about:blank",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start Chrome: {e}"))
    }

    fn connect(port: u16) -> Result<Cdp, String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(body) = http_get(port, "/json/list")
                && let Ok(Value::Array(tabs)) = serde_json::from_str::<Value>(&body)
                && let Some(url) = tabs
                    .iter()
                    .find(|t| t.get("type").and_then(Value::as_str) == Some("page"))
                    .and_then(|t| t.get("webSocketDebuggerUrl"))
                    .and_then(Value::as_str)
            {
                let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
                let (ws, _) = tungstenite::client(url, stream).map_err(|e| e.to_string())?;
                return Ok(Cdp {
                    ws,
                    next: 0,
                    console: Vec::new(),
                });
            }
            if Instant::now() > deadline {
                return Err("Chrome's debugging endpoint did not come up".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn run(root: &Path, chrome: &Path, place: &str) -> Result<(), String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let serve_root = root.to_path_buf();
        std::thread::spawn(move || crate::serve::serve(&serve_root, listener));
        let url = format!("http://127.0.0.1:{port}/?place={place}");
        let debug_port = free_port()?;
        let mut child = launch(chrome, root, debug_port)?;
        let result = drive(root, debug_port, &url);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    fn drive(root: &Path, debug_port: u16, url: &str) -> Result<(), String> {
        println!("\n==> headless Chrome: {url}");
        let mut cdp = connect(debug_port)?;
        cdp.call("Runtime.enable", json!({}))?;
        cdp.call("Page.navigate", json!({"url": url}))?;
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut frames = 0u32;
        let mut threads = String::new();
        let mut status = String::new();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
            let state = cdp.eval(
                "JSON.stringify([document.documentElement.dataset.frames || '0', document.documentElement.dataset.threads || '', (document.getElementById('status') || {}).textContent || ''])",
            )?;
            let parsed: Vec<String> = state
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            frames = parsed.first().and_then(|f| f.parse().ok()).unwrap_or(0);
            threads = parsed.get(1).cloned().unwrap_or_default();
            status = parsed.get(2).cloned().unwrap_or_default();
            if status.starts_with("Failed") || status.contains("no WebGPU") || frames >= 120 {
                break;
            }
        }
        println!("    threads={threads} frames={frames} status={status:?}");
        let shot = cdp.call("Page.captureScreenshot", json!({"format": "png"}))?;
        if let Some(data) = shot.get("data").and_then(Value::as_str) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| e.to_string())?;
            let path = root.join("dist").join("web-test.png");
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            println!("    screenshot {}", path.display());
        }
        if threads != "true" || frames < 120 {
            for line in cdp.console.iter().rev().take(20).rev() {
                println!("    console: {line}");
            }
            return Err(format!(
                "headless Chrome: threads={threads} frames={frames} status={status:?}"
            ));
        }
        println!("<== headless Chrome: ok");
        Ok(())
    }
}
