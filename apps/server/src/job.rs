use parking_lot::Mutex;
use rbx_dom::{DataModel, RemoteValue, Variant};
use rbx_net::quic::{Certificate, QuicServer};
use rbx_script::net::NetServer;
use rbx_script::{Engine, Identity, MessageType, NetworkRequest, ScriptRuntime};
use serde::Serialize;
use serde_json::{Map, Value as Json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const FRAME: f64 = 1.0 / 60.0;
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(60);
const STANDARD_SERVICES: &[&str] = &[
    "Workspace",
    "Players",
    "Lighting",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "SoundService",
    "Teams",
    "Chat",
];
const OUTPUT_TAIL: usize = 200;

#[derive(Clone, Debug)]
pub struct JobSpec {
    pub id: String,
    pub expiration: f64,
    pub category: i32,
    pub cores: f64,
    pub place: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub category: i32,
    pub cores: f64,
    pub expiration_in_seconds: f64,
    pub port: Option<u16>,
    pub cert_hash: Option<String>,
    pub players: usize,
    pub game_time: f64,
    pub closed: bool,
}

pub enum Command {
    Execute {
        name: String,
        source: String,
        args: Vec<Json>,
        reply: mpsc::Sender<Result<Vec<Json>, String>>,
    },
    Renew(f64),
    Diag(mpsc::Sender<Json>),
    Close,
}

pub struct JobHandle {
    pub spec: JobSpec,
    tx: mpsc::Sender<Command>,
    pub info: Arc<Mutex<JobInfo>>,
    expires: Arc<Mutex<Instant>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn deadline(seconds: f64) -> Instant {
    Instant::now() + Duration::from_secs_f64(seconds.clamp(0.0, 1e9))
}

impl JobHandle {
    pub fn open(
        spec: JobSpec,
        bind: std::net::IpAddr,
        content: Option<PathBuf>,
    ) -> Result<JobHandle, String> {
        let dm = match &spec.place {
            Some(path) => {
                let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let mut dm = DataModel::new();
                dm.load_place(&data)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                dm
            }
            None => DataModel::new(),
        };
        let mut dm = dm;
        for service in STANDARD_SERVICES {
            dm.get_service(service).map_err(|e| e.to_string())?;
        }
        let info = Arc::new(Mutex::new(JobInfo {
            id: spec.id.clone(),
            category: spec.category,
            cores: spec.cores,
            expiration_in_seconds: spec.expiration,
            ..JobInfo::default()
        }));
        let expires = Arc::new(Mutex::new(deadline(spec.expiration)));
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_info = info.clone();
        let thread_expires = expires.clone();
        let name = format!("job-{}", spec.id);
        let thread = std::thread::Builder::new()
            .name(name)
            .stack_size(64 << 20)
            .spawn(move || {
                let mut job = match Job::new(dm, bind, content) {
                    Ok(j) => {
                        let _ = ready_tx.send(Ok(()));
                        j
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                job.run(rx, thread_info, thread_expires);
            })
            .map_err(|e| e.to_string())?;
        ready_rx
            .recv()
            .map_err(|_| "job thread exited during startup".to_string())??;
        Ok(JobHandle {
            spec,
            tx,
            info,
            expires,
            thread: Some(thread),
        })
    }

    pub fn execute(&self, name: &str, source: &str, args: Vec<Json>) -> Result<Vec<Json>, String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Command::Execute {
                name: name.to_string(),
                source: source.to_string(),
                args,
                reply,
            })
            .map_err(|_| "job is closed".to_string())?;
        rx.recv().map_err(|_| "job is closed".to_string())?
    }

    pub fn renew(&self, seconds: f64) {
        *self.expires.lock() = deadline(seconds);
        let _ = self.tx.send(Command::Renew(seconds));
    }

    pub fn expiration(&self) -> f64 {
        self.expires
            .lock()
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= *self.expires.lock() || self.info.lock().closed
    }

    pub fn diag(&self) -> Option<Json> {
        let (reply, rx) = mpsc::channel();
        self.tx.send(Command::Diag(reply)).ok()?;
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }

    pub fn close(mut self) {
        let _ = self.tx.send(Command::Close);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Job {
    runtime: ScriptRuntime,
    net: Option<(NetServer, QuicServer)>,
    bind: std::net::IpAddr,
}

pub fn remote_to_json(v: &RemoteValue) -> Json {
    match v {
        RemoteValue::Value(v) => match v {
            Variant::Nil => Json::Null,
            Variant::Bool(b) => Json::Bool(*b),
            Variant::Int(i) => Json::from(*i),
            Variant::Int64(i) => Json::from(*i),
            Variant::Float(f) => Json::from(*f as f64),
            Variant::Double(f) => {
                if f.fract() == 0.0 && f.abs() < 9.0e15 {
                    Json::from(*f as i64)
                } else {
                    Json::from(*f)
                }
            }
            Variant::String(s) | Variant::ProtectedString(s) | Variant::Content(s) => {
                Json::String(s.clone())
            }
            other => Json::String(format!("{other:?}")),
        },
        RemoteValue::Enum { ty, value } => Json::String(format!("Enum.{ty}.{value}")),
        RemoteValue::Array(items) => Json::Array(items.iter().map(remote_to_json).collect()),
        RemoteValue::Map(items) => {
            let mut m = Map::new();
            for (k, v) in items {
                m.insert(k.clone(), remote_to_json(v));
            }
            Json::Object(m)
        }
    }
}

fn json_to_lua(lua: &mlua::Lua, v: &Json) -> mlua::Result<mlua::Value> {
    Ok(match v {
        Json::Null => mlua::Value::Nil,
        Json::Bool(b) => mlua::Value::Boolean(*b),
        Json::Number(n) => mlua::Value::Number(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => mlua::Value::String(lua.create_string(s)?),
        Json::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, json_to_lua(lua, item)?)?;
            }
            mlua::Value::Table(t)
        }
        Json::Object(m) => {
            let t = lua.create_table()?;
            for (k, item) in m {
                t.raw_set(k.as_str(), json_to_lua(lua, item)?)?;
            }
            mlua::Value::Table(t)
        }
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Job {
    fn new(dm: DataModel, bind: std::net::IpAddr, content: Option<PathBuf>) -> Result<Job, String> {
        let mut engine = Engine::new(dm);
        let has_content = content.is_some();
        engine.content_root = content;
        engine.animate_fallback = true;
        engine.process_after_load();
        engine.running = true;
        let runtime = ScriptRuntime::new(Arc::new(parking_lot::Mutex::new(engine)))
            .map_err(|e| e.to_string())?;
        runtime.scan_scripts();
        if has_content && let Err(e) = runtime.start_server_core_scripts() {
            tracing::warn!("server core scripts: {e}");
        }
        Ok(Job {
            runtime,
            net: None,
            bind,
        })
    }

    fn execute(&mut self, name: &str, source: &str, args: &[Json]) -> Result<Vec<Json>, String> {
        let lua = &self.runtime.lua;
        let mut values = Vec::with_capacity(args.len());
        for a in args {
            values.push(json_to_lua(lua, a).map_err(|e| e.to_string())?);
        }
        let key = self.runtime.execute_captured(
            source,
            name,
            Identity::WebService,
            mlua::MultiValue::from_vec(values),
        )?;
        let started = Instant::now();
        let result = loop {
            if let Some(r) = self.runtime.take_capture(key) {
                break r?;
            }
            if started.elapsed() > EXECUTE_TIMEOUT {
                self.runtime.drop_capture(key);
                return Err("script did not finish in time".into());
            }
            self.frame();
            std::thread::sleep(Duration::from_secs_f64(FRAME));
        };
        let mut out = Vec::new();
        for v in result.into_vec() {
            let rv = rbx_script::api::remote::to_remote(&v, 0).map_err(|e| e.to_string())?;
            out.push(remote_to_json(&rv));
        }
        Ok(out)
    }

    fn handle_network(&mut self) {
        let requests = std::mem::take(&mut self.runtime.engine.lock().network_requests);
        for req in requests {
            match req {
                NetworkRequest::StartServer { port } => {
                    if self.net.is_some() {
                        continue;
                    }
                    let started = Certificate::self_signed(&["localhost"])
                        .map_err(|e| e.to_string())
                        .and_then(|cert| {
                            QuicServer::bind(SocketAddr::new(self.bind, port), cert)
                                .map_err(|e| e.to_string())
                        });
                    match started {
                        Ok(quic) => {
                            let net = NetServer::start(&self.runtime);
                            self.net = Some((net, quic));
                        }
                        Err(e) => self.runtime.engine.lock().print(
                            MessageType::Error,
                            format!("NetworkServer failed to start: {e}"),
                        ),
                    }
                }
                NetworkRequest::StopServer => {
                    if let Some((mut net, quic)) = self.net.take() {
                        net.close_all(&self.runtime);
                        quic.close();
                    }
                }
                NetworkRequest::Connect { .. } | NetworkRequest::Disconnect => {}
            }
        }
    }

    fn frame(&mut self) {
        self.runtime.step(FRAME);
        self.handle_network();
        if let Some((net, quic)) = &mut self.net {
            while let Some(conn) = quic.accept() {
                net.accept(Box::new(conn));
            }
            net.tick(&self.runtime);
        }
    }

    fn update_info(&self, info: &Mutex<JobInfo>, expires: &Mutex<Instant>) {
        let mut i = info.lock();
        i.expiration_in_seconds = expires
            .lock()
            .saturating_duration_since(Instant::now())
            .as_secs_f64();
        i.game_time = self.runtime.engine.lock().game_time;
        match &self.net {
            Some((net, quic)) => {
                i.port = quic.local_addr().ok().map(|a| a.port());
                i.cert_hash = Some(hex(&quic.cert_hash()));
                i.players = net.peer_count();
            }
            None => {
                i.port = None;
                i.cert_hash = None;
                i.players = 0;
            }
        }
    }

    fn diag(&self) -> Json {
        let e = self.runtime.engine.lock();
        let output: Vec<Json> = e
            .output
            .iter()
            .rev()
            .take(OUTPUT_TAIL)
            .rev()
            .map(|m| Json::String(format!("{:?}: {}", m.kind, m.text)))
            .collect();
        serde_json::json!({
            "instances": e.dm.len(),
            "gameTime": e.game_time,
            "clients": e.client_count,
            "output": output,
        })
    }

    fn run(
        &mut self,
        rx: mpsc::Receiver<Command>,
        info: Arc<Mutex<JobInfo>>,
        expires: Arc<Mutex<Instant>>,
    ) {
        let frame = Duration::from_secs_f64(FRAME);
        let mut next = Instant::now();
        loop {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Command::Execute {
                        name,
                        source,
                        args,
                        reply,
                    } => {
                        let r = self.execute(&name, &source, &args);
                        self.handle_network();
                        let _ = reply.send(r);
                    }
                    Command::Renew(_) => {}
                    Command::Diag(reply) => {
                        let _ = reply.send(self.diag());
                    }
                    Command::Close => {
                        self.shutdown(&info);
                        return;
                    }
                }
            }
            if Instant::now() >= *expires.lock() {
                self.shutdown(&info);
                return;
            }
            self.frame();
            self.update_info(&info, &expires);
            next += frame;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }

    fn shutdown(&mut self, info: &Mutex<JobInfo>) {
        if let Some((mut net, quic)) = self.net.take() {
            net.close_all(&self.runtime);
            quic.close();
        }
        info.lock().closed = true;
    }
}
