use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "txt" | "lua" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "css" => "text/css",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn resolve(root: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split(['?', '#']).next().unwrap_or("/");
    let path = percent_decode(path);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| *p == ".." || p.contains('\\')) {
        return None;
    }
    let rel = parts.join("/");
    let from_repo = crate::ci::WEB_CONTENT_DIRS
        .iter()
        .any(|d| rel == *d || rel.starts_with(&format!("{d}/")));
    let base = if from_repo {
        root.to_path_buf()
    } else {
        root.join("dist").join("web")
    };
    let mut full = parts.iter().fold(base, |p, s| p.join(s));
    if full.is_dir() {
        full = full.join("index.html");
    }
    full.is_file().then_some(full)
}

fn handle(root: &Path, mut stream: TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header.trim().is_empty() => break,
            Ok(_) => {}
        }
    }
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or("");
    let url = words.next().unwrap_or("/");
    let isolation = "Cross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nCross-Origin-Resource-Policy: same-origin\r\nCache-Control: no-cache\r\n";
    let found = if method == "GET" || method == "HEAD" {
        resolve(root, url)
    } else {
        None
    };
    let response = match found.and_then(|p| std::fs::read(&p).ok().map(|d| (p, d))) {
        Some((path, data)) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n{isolation}Connection: close\r\n\r\n",
                content_type(&path),
                data.len()
            );
            let mut out = head.into_bytes();
            if method == "GET" {
                out.extend_from_slice(&data);
            }
            out
        }
        None => format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n{isolation}Connection: close\r\n\r\n"
        )
        .into_bytes(),
    };
    let _ = stream.write_all(&response);
}

pub fn run(root: &Path, options: &[String]) -> Result<(), String> {
    let port = options
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse::<u16>().ok())
        .unwrap_or(8080);
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    println!("serving http://127.0.0.1:{port}/ (cross-origin isolated)");
    for stream in listener.incoming().flatten() {
        let root = root.to_path_buf();
        std::thread::spawn(move || handle(&root, stream));
    }
    Ok(())
}
