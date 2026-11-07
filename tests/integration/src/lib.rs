use rbx_dom::DataModel;
use rbx_net::quic;
use rbx_script::Identity;
use rbx_script::net::ClientSession;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub const FRAME: f64 = 1.0 / 60.0;

pub struct Admin {
    pub base: String,
}

impl Admin {
    pub fn new(addr: SocketAddr) -> Admin {
        Admin {
            base: format!("http://{addr}"),
        }
    }

    pub fn get(&self, path: &str) -> Value {
        ureq::get(format!("{}{path}", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json::<Value>())
            .unwrap_or_else(|e| panic!("GET {path}: {e}"))
    }

    pub fn post(&self, path: &str, body: Value) -> Value {
        ureq::post(format!("{}{path}", self.base))
            .send_json(body)
            .and_then(|mut r| r.body_mut().read_json::<Value>())
            .unwrap_or_else(|e| panic!("POST {path}: {e}"))
    }

    pub fn delete(&self, path: &str) {
        let _ = ureq::delete(format!("{}{path}", self.base)).call();
    }

    pub fn execute(&self, job: &str, source: &str) -> Vec<Value> {
        let r = self.post(
            &format!("/v1/jobs/{job}/execute"),
            json!({ "source": source }),
        );
        r["results"].as_array().cloned().unwrap_or_default()
    }

    pub fn wait_for_game(&self, job: &str, timeout: Duration) -> (SocketAddr, [u8; 32]) {
        let start = Instant::now();
        loop {
            let info = self.get(&format!("/v1/jobs/{job}"));
            if let (Some(port), Some(hash)) = (info["port"].as_u64(), info["certHash"].as_str()) {
                let addr = SocketAddr::from(([127, 0, 0, 1], port as u16));
                return (addr, decode_hash(hash));
            }
            assert!(
                start.elapsed() < timeout,
                "game server never started: {info}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn decode_hash(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2).unwrap_or("00"), 16).unwrap_or(0);
    }
    out
}

pub fn headless_client(
    addr: SocketAddr,
    hash: [u8; 32],
    name: &str,
    user_id: i32,
) -> ClientSession {
    let conn =
        quic::connect(addr, hash).unwrap_or_else(|e| panic!("{name} could not connect: {e}"));
    let mut dm = DataModel::new();
    for s in ["Workspace", "Players", "Lighting", "ReplicatedStorage"] {
        let _ = dm.get_service(s);
    }
    ClientSession::new(dm, Box::new(conn), name, user_id).unwrap_or_else(|e| panic!("{name}: {e}"))
}

pub fn run_lua(client: &ClientSession, source: &str) -> Vec<String> {
    client
        .runtime
        .execute_named(source, "Test", Identity::CmdLine, Default::default())
        .unwrap_or_else(|e| panic!("client script failed: {e}"))
        .into_vec()
        .into_iter()
        .map(|v| match v {
            rbx_script::mlua::Value::String(s) => s.to_string_lossy(),
            rbx_script::mlua::Value::Boolean(b) => b.to_string(),
            rbx_script::mlua::Value::Number(n) => n.to_string(),
            rbx_script::mlua::Value::Integer(i) => i.to_string(),
            rbx_script::mlua::Value::Nil => "nil".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

pub fn step_until(
    clients: &mut [ClientSession],
    timeout: Duration,
    mut cond: impl FnMut(&mut [ClientSession]) -> bool,
) {
    let start = Instant::now();
    loop {
        if cond(clients) {
            return;
        }
        if start.elapsed() >= timeout {
            for (i, c) in clients.iter().enumerate() {
                let e = c.runtime.engine.lock();
                let names: Vec<String> =
                    e.dm.find_service("Players")
                        .map(|p| {
                            e.dm.children_of(p)
                                .iter()
                                .map(|&c| e.dm.name_of(c).to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                let output: Vec<String> = e
                    .output
                    .iter()
                    .rev()
                    .take(5)
                    .map(|m| m.text.clone())
                    .collect();
                eprintln!(
                    "client {i}: joined={} state={:?} players={names:?} output={output:?}",
                    c.joined(),
                    c.net.rep.state
                );
            }
            panic!("timed out");
        }
        for c in clients.iter_mut() {
            c.step(FRAME);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
