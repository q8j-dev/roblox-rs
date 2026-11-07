use arbiter::{Arbiter, Launcher, Server, Settings};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

struct InProcess {
    handle: Option<server::ServerHandle>,
    admin: SocketAddr,
}

impl Server for InProcess {
    fn admin(&self) -> SocketAddr {
        self.admin
    }

    fn alive(&mut self) -> bool {
        self.handle.is_some()
    }

    fn kill(&mut self) {
        if let Some(h) = self.handle.take() {
            h.stop();
        }
    }
}

struct InProcessLauncher;

impl Launcher for InProcessLauncher {
    fn launch(&self, _port: u16) -> Result<Box<dyn Server>, String> {
        let handle = server::spawn(server::Config {
            http: "127.0.0.1:0".parse().unwrap(),
            game_bind: "127.0.0.1".parse().unwrap(),
            ..server::Config::default()
        })
        .map_err(|e| e.to_string())?;
        let admin = handle.http;
        Ok(Box::new(InProcess {
            handle: Some(handle),
            admin,
        }))
    }
}

struct Running {
    arbiter: Arc<Arbiter>,
    base: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn start(settings: Settings) -> Running {
    let arbiter = Arc::new(Arbiter::new(Box::new(InProcessLauncher), settings));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let a = arbiter.clone();
    let thread = std::thread::spawn(move || {
        rt.block_on(arbiter::run(a, listener, async move {
            let _ = rx.await;
        }))
        .unwrap();
    });
    Running {
        arbiter,
        base,
        stop: Some(tx),
        thread: Some(thread),
    }
}

fn get(base: &str, path: &str) -> Value {
    ureq::get(format!("{base}{path}"))
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap()
}

fn post(base: &str, path: &str, body: Value) -> Value {
    ureq::post(format!("{base}{path}"))
        .send_json(body)
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap()
}

fn wait(mut cond: impl FnMut() -> bool) {
    let start = std::time::Instant::now();
    while !cond() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn routes_jobs_to_pooled_processes_and_recycles_them() {
    let r = start(Settings {
        monitor_interval_ms: 100,
        ..Settings::default()
    });
    wait(|| r.arbiter.process_count() == 1);
    post(
        &r.base,
        "/v1/jobs",
        json!({ "job": { "id": "one", "expirationInSeconds": 60 } }),
    );
    post(
        &r.base,
        "/v1/jobs",
        json!({ "job": { "id": "two", "expirationInSeconds": 60 } }),
    );
    assert_eq!(get(&r.base, "/v1/status")["environmentCount"], 2);
    let out = post(
        &r.base,
        "/v1/jobs/one/execute",
        json!({ "source": "return 6 * 7" }),
    );
    assert_eq!(out["results"], json!([42]));
    assert_eq!(get(&r.base, "/v1/jobs").as_array().map(Vec::len), Some(2));
    wait(|| r.arbiter.process_count() == 3);
    ureq::delete(format!("{}/v1/jobs/one", r.base))
        .call()
        .unwrap();
    let stats = get(&r.base, "/v1/arbiter/stats");
    assert_eq!(stats["stats"]["processesRecycled"], 1);
    assert_eq!(stats["jobs"], 1);
    let batch = post(
        &r.base,
        "/v1/batch",
        json!({ "job": { "id": "b" }, "script": { "source": "return 'done'" } }),
    );
    assert_eq!(batch["results"], json!(["done"]));
}

#[test]
fn restarts_a_process_that_dies() {
    let r = start(Settings {
        monitor_interval_ms: 100,
        failures_before_restart: 2,
        ..Settings::default()
    });
    wait(|| r.arbiter.process_count() == 1);
    post(
        &r.base,
        "/v1/jobs",
        json!({ "job": { "id": "doomed", "expirationInSeconds": 60 } }),
    );
    assert!(r.arbiter.kill_process_of("doomed"));
    wait(|| get(&r.base, "/v1/arbiter/stats")["stats"]["processesRestarted"] == 1);
    assert_eq!(get(&r.base, "/v1/status")["environmentCount"], 0);
    wait(|| r.arbiter.process_count() >= 1);
    post(
        &r.base,
        "/v1/jobs",
        json!({ "job": { "id": "next", "expirationInSeconds": 60 } }),
    );
    let out = post(
        &r.base,
        "/v1/jobs/next/execute",
        json!({ "source": "return 'alive'" }),
    );
    assert_eq!(out["results"], json!(["alive"]));
}
