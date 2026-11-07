use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub trait Server: Send {
    fn admin(&self) -> SocketAddr;
    fn alive(&mut self) -> bool;
    fn kill(&mut self);
}

pub trait Launcher: Send + Sync {
    fn launch(&self, port: u16) -> Result<Box<dyn Server>, String>;
}

pub struct ProcessLauncher {
    pub binary: PathBuf,
    pub args: Vec<String>,
}

struct ProcessServer {
    child: std::process::Child,
    admin: SocketAddr,
}

impl Server for ProcessServer {
    fn admin(&self) -> SocketAddr {
        self.admin
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self, port: u16) -> Result<Box<dyn Server>, String> {
        let admin = SocketAddr::from(([127, 0, 0, 1], port));
        let child = std::process::Command::new(&self.binary)
            .arg("--http")
            .arg(admin.to_string())
            .args(&self.args)
            .spawn()
            .map_err(|e| format!("{}: {e}", self.binary.display()))?;
        Ok(Box::new(ProcessServer { child, admin }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub multi_process: bool,
    pub recycle_process: bool,
    pub recycle_queue_size: usize,
    pub base_port: u16,
    pub max_processes: usize,
    pub monitor_interval_ms: u64,
    pub failures_before_restart: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            multi_process: true,
            recycle_process: true,
            recycle_queue_size: 1,
            base_port: 64990,
            max_processes: 64,
            monitor_interval_ms: 1000,
            failures_before_restart: 3,
        }
    }
}

struct Process {
    id: u64,
    server: Box<dyn Server>,
    port: u16,
    jobs: Vec<String>,
    failures: u32,
    started: Instant,
}

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub processes_started: u64,
    pub processes_restarted: u64,
    pub processes_recycled: u64,
    pub jobs_opened: u64,
    pub jobs_closed: u64,
    pub exceptions: Vec<String>,
}

struct Inner {
    settings: Settings,
    ready: VecDeque<Process>,
    active: Vec<Process>,
    jobs: HashMap<String, u64>,
    next_id: u64,
    stats: Stats,
}

pub struct Arbiter {
    launcher: Box<dyn Launcher>,
    inner: Mutex<Inner>,
    filling: Mutex<()>,
}

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn call(
    method: Method,
    addr: SocketAddr,
    path: &str,
    body: Option<&Value>,
) -> Result<(StatusCode, Value), String> {
    let url = format!("http://{addr}{path}");
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let response = match (method.clone(), body) {
        (Method::GET, _) => agent.get(&url).call(),
        (Method::DELETE, _) => agent.delete(&url).call(),
        (_, Some(b)) => agent.post(&url).send_json(b),
        (_, None) => agent.post(&url).send_empty(),
    };
    let mut r = response.map_err(|e| e.to_string())?;
    let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let value = if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        r.body_mut().read_json::<Value>().unwrap_or(Value::Null)
    };
    Ok((status, value))
}

fn hello(addr: SocketAddr) -> bool {
    let url = format!("http://{addr}/v1/hello");
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .build();
    let agent: ureq::Agent = config.into();
    agent
        .get(&url)
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .is_some_and(|s| s == "Hello World")
}

impl Arbiter {
    pub fn new(launcher: Box<dyn Launcher>, settings: Settings) -> Arbiter {
        Arbiter {
            launcher,
            inner: Mutex::new(Inner {
                settings,
                ready: VecDeque::new(),
                active: Vec::new(),
                jobs: HashMap::new(),
                next_id: 1,
                stats: Stats::default(),
            }),
            filling: Mutex::new(()),
        }
    }

    fn free_port(inner: &Inner) -> Option<u16> {
        let used: Vec<u16> = inner
            .ready
            .iter()
            .chain(inner.active.iter())
            .map(|p| p.port)
            .collect();
        (0..inner.settings.max_processes as u16)
            .map(|i| inner.settings.base_port.wrapping_add(i))
            .find(|p| !used.contains(p))
    }

    fn start_process(&self) -> Result<Process, String> {
        let (id, port) = {
            let mut inner = self.inner.lock();
            let port = Self::free_port(&inner).ok_or("no free ports for a new server process")?;
            let id = inner.next_id;
            inner.next_id += 1;
            inner.stats.processes_started += 1;
            (id, port)
        };
        let mut server = self.launcher.launch(port)?;
        let start = Instant::now();
        let admin = server.admin();
        while !hello(admin) {
            if !server.alive() || start.elapsed() > Duration::from_secs(20) {
                server.kill();
                return Err(format!("server on port {port} did not answer"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(Process {
            id,
            server,
            port,
            jobs: Vec::new(),
            failures: 0,
            started: Instant::now(),
        })
    }

    pub fn fill_pool(&self) {
        let _guard = self.filling.lock();
        loop {
            let need = {
                let inner = self.inner.lock();
                let total = inner.ready.len() + inner.active.len();
                inner.ready.len() < inner.settings.recycle_queue_size.max(1)
                    && total < inner.settings.max_processes
            };
            if !need {
                return;
            }
            match self.start_process() {
                Ok(p) => self.inner.lock().ready.push_back(p),
                Err(e) => {
                    tracing::error!("{e}");
                    self.inner.lock().stats.exceptions.push(e);
                    return;
                }
            }
        }
    }

    fn assign(&self, job: &str) -> Result<SocketAddr, String> {
        {
            let mut inner = self.inner.lock();
            if !inner.settings.multi_process
                && let Some(p) = inner.active.iter_mut().min_by_key(|p| p.jobs.len())
            {
                p.jobs.push(job.to_string());
                let (id, addr) = (p.id, p.server.admin());
                inner.jobs.insert(job.to_string(), id);
                return Ok(addr);
            }
        }
        let process = {
            let taken = self.inner.lock().ready.pop_front();
            match taken {
                Some(p) => p,
                None => self.start_process()?,
            }
        };
        let addr = process.server.admin();
        let mut inner = self.inner.lock();
        let mut process = process;
        process.jobs.push(job.to_string());
        inner.jobs.insert(job.to_string(), process.id);
        inner.active.push(process);
        Ok(addr)
    }

    fn job_addr(&self, job: &str) -> Option<SocketAddr> {
        let inner = self.inner.lock();
        let id = inner.jobs.get(job)?;
        inner
            .active
            .iter()
            .find(|p| p.id == *id)
            .map(|p| p.server.admin())
    }

    fn forget_job(&self, job: &str) {
        let mut inner = self.inner.lock();
        let Some(id) = inner.jobs.remove(job) else {
            return;
        };
        inner.stats.jobs_closed += 1;
        let recycle = inner.settings.recycle_process && inner.settings.multi_process;
        if let Some(i) = inner.active.iter().position(|p| p.id == id) {
            inner.active[i].jobs.retain(|j| j != job);
            if recycle && inner.active[i].jobs.is_empty() {
                let mut p = inner.active.remove(i);
                p.server.kill();
                inner.stats.processes_recycled += 1;
            }
        }
    }

    pub fn monitor(&self) {
        let threshold = self.inner.lock().settings.failures_before_restart;
        let checks: Vec<(u64, SocketAddr)> = {
            let inner = self.inner.lock();
            inner
                .ready
                .iter()
                .chain(inner.active.iter())
                .map(|p| (p.id, p.server.admin()))
                .collect()
        };
        let mut dead = Vec::new();
        for (id, addr) in checks {
            let ok = hello(addr);
            let mut inner = self.inner.lock();
            let Inner { ready, active, .. } = &mut *inner;
            if let Some(p) = ready
                .iter_mut()
                .chain(active.iter_mut())
                .find(|p| p.id == id)
            {
                let alive = p.server.alive();
                if ok && alive {
                    p.failures = 0;
                } else {
                    p.failures += 1;
                    if !alive || p.failures >= threshold {
                        dead.push(id);
                    }
                }
            }
        }
        for id in dead {
            let mut inner = self.inner.lock();
            let removed = if let Some(i) = inner.ready.iter().position(|p| p.id == id) {
                inner.ready.remove(i)
            } else if let Some(i) = inner.active.iter().position(|p| p.id == id) {
                Some(inner.active.remove(i))
            } else {
                None
            };
            if let Some(mut p) = removed {
                p.server.kill();
                for j in &p.jobs {
                    inner.jobs.remove(j);
                }
                inner.stats.processes_restarted += 1;
                let msg = format!(
                    "server process {} on port {} stopped answering after {:.0}s; {} job(s) lost",
                    p.id,
                    p.port,
                    p.started.elapsed().as_secs_f64(),
                    p.jobs.len()
                );
                tracing::warn!("{msg}");
                inner.stats.exceptions.push(msg);
            }
        }
        self.fill_pool();
    }

    pub fn shutdown(&self) {
        let mut inner = self.inner.lock();
        for mut p in inner.ready.drain(..) {
            p.server.kill();
        }
        for mut p in inner.active.drain(..) {
            p.server.kill();
        }
        inner.jobs.clear();
    }

    pub fn process_count(&self) -> usize {
        let inner = self.inner.lock();
        inner.ready.len() + inner.active.len()
    }

    pub fn kill_process_of(&self, job: &str) -> bool {
        let mut inner = self.inner.lock();
        let Some(id) = inner.jobs.get(job).copied() else {
            return false;
        };
        match inner.active.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                p.server.kill();
                true
            }
            None => false,
        }
    }
}

type Shared = Arc<Arbiter>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn relay(r: Result<(StatusCode, Value), String>) -> Result<Response, ApiError> {
    match r {
        Ok((StatusCode::NO_CONTENT, _)) => Ok(StatusCode::NO_CONTENT.into_response()),
        Ok((status, v)) => Ok((status, Json(v)).into_response()),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn hello_route() -> &'static str {
    "Hello World"
}

async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

async fn status(State(a): State<Shared>) -> Json<Value> {
    let count = a.inner.lock().jobs.len();
    Json(json!({ "version": env!("CARGO_PKG_VERSION"), "environmentCount": count }))
}

async fn open_job(State(a): State<Shared>, Json(body): Json<Value>) -> Result<Response, ApiError> {
    let id = body["job"]["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "job id is empty".into()))?
        .to_string();
    if a.inner.lock().jobs.contains_key(&id) {
        return Err(ApiError(StatusCode::CONFLICT, "job already exists".into()));
    }
    let r = blocking(move || {
        let addr = a.assign(&id)?;
        a.inner.lock().stats.jobs_opened += 1;
        let r = call(Method::POST, addr, "/v1/jobs", Some(&body));
        if !matches!(r, Ok((s, _)) if s.is_success()) {
            a.forget_job(&id);
        }
        a.fill_pool();
        r
    })
    .await?;
    relay(r)
}

async fn list_jobs(State(a): State<Shared>) -> Result<Json<Value>, ApiError> {
    let jobs = blocking(move || {
        let addrs: Vec<SocketAddr> = a
            .inner
            .lock()
            .active
            .iter()
            .map(|p| p.server.admin())
            .collect();
        let mut all = Vec::new();
        for addr in addrs {
            if let Ok((_, Value::Array(items))) = call(Method::GET, addr, "/v1/jobs", None) {
                all.extend(items);
            }
        }
        all
    })
    .await?;
    Ok(Json(Value::Array(jobs)))
}

async fn forward(
    a: Shared,
    job: String,
    method: Method,
    path: String,
    body: Option<Value>,
) -> Result<Response, ApiError> {
    let addr = a
        .job_addr(&job)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "job not found".into()))?;
    let closing = method == Method::DELETE;
    let r = blocking(move || {
        let r = call(method, addr, &path, body.as_ref());
        if closing {
            a.forget_job(&job);
            a.fill_pool();
        }
        r
    })
    .await?;
    relay(r)
}

async fn get_job(State(a): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}");
    forward(a, id, Method::GET, path, None).await
}

async fn close_job(State(a): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}");
    forward(a, id, Method::DELETE, path, None).await
}

async fn execute(
    State(a): State<Shared>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}/execute");
    forward(a, id, Method::POST, path, Some(b)).await
}

async fn renew(
    State(a): State<Shared>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}/renew");
    forward(a, id, Method::POST, path, Some(b)).await
}

async fn expiration(State(a): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}/expiration");
    forward(a, id, Method::GET, path, None).await
}

async fn diag(State(a): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let path = format!("/v1/jobs/{id}/diag");
    forward(a, id, Method::GET, path, None).await
}

async fn close_expired(State(a): State<Shared>) -> Result<Json<Value>, ApiError> {
    let n = blocking(move || {
        let jobs: Vec<(String, SocketAddr)> = {
            let inner = a.inner.lock();
            inner
                .jobs
                .iter()
                .filter_map(|(j, id)| {
                    inner
                        .active
                        .iter()
                        .find(|p| p.id == *id)
                        .map(|p| (j.clone(), p.server.admin()))
                })
                .collect()
        };
        let mut closed = 0;
        for (job, addr) in jobs {
            let expired = call(
                Method::GET,
                addr,
                &format!("/v1/jobs/{job}/expiration"),
                None,
            )
            .ok()
            .and_then(|(_, v)| v["expirationInSeconds"].as_f64())
            .is_none_or(|s| s <= 0.0);
            if expired {
                let _ = call(Method::DELETE, addr, &format!("/v1/jobs/{job}"), None);
                a.forget_job(&job);
                closed += 1;
            }
        }
        a.fill_pool();
        closed
    })
    .await?;
    Ok(Json(json!({ "closed": n })))
}

async fn close_all(State(a): State<Shared>) -> Result<Json<Value>, ApiError> {
    let n = blocking(move || {
        let jobs: Vec<String> = a.inner.lock().jobs.keys().cloned().collect();
        for job in &jobs {
            if let Some(addr) = a.job_addr(job) {
                let _ = call(Method::DELETE, addr, &format!("/v1/jobs/{job}"), None);
            }
            a.forget_job(job);
        }
        a.fill_pool();
        jobs.len()
    })
    .await?;
    Ok(Json(json!({ "closed": n })))
}

async fn batch(State(a): State<Shared>, Json(body): Json<Value>) -> Result<Response, ApiError> {
    let id = body["job"]["id"].as_str().unwrap_or_default().to_string();
    let r = blocking(move || {
        let addr = a.assign(&id)?;
        let r = call(Method::POST, addr, "/v1/batch", Some(&body));
        a.forget_job(&id);
        a.fill_pool();
        r
    })
    .await?;
    relay(r)
}

async fn arbiter_stats(State(a): State<Shared>) -> Json<Value> {
    let inner = a.inner.lock();
    Json(json!({
        "stats": inner.stats,
        "settings": inner.settings,
        "readyProcesses": inner.ready.len(),
        "activeProcesses": inner.active.len(),
        "jobs": inner.jobs.len(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    multi_process: Option<bool>,
    recycle_process: Option<bool>,
    recycle_queue_size: Option<usize>,
    clear_exceptions: Option<bool>,
}

async fn arbiter_settings(
    State(a): State<Shared>,
    Json(p): Json<SettingsPatch>,
) -> Result<Json<Value>, ApiError> {
    {
        let mut inner = a.inner.lock();
        if let Some(v) = p.multi_process {
            inner.settings.multi_process = v;
        }
        if let Some(v) = p.recycle_process {
            inner.settings.recycle_process = v;
        }
        if let Some(v) = p.recycle_queue_size {
            inner.settings.recycle_queue_size = v;
        }
        if p.clear_exceptions == Some(true) {
            inner.stats.exceptions.clear();
        }
    }
    let a2 = a.clone();
    blocking(move || a2.fill_pool()).await?;
    let settings = a.inner.lock().settings.clone();
    Ok(Json(json!(settings)))
}

pub fn router(arbiter: Shared) -> Router {
    Router::new()
        .route("/v1/hello", get(hello_route))
        .route("/v1/version", get(version))
        .route("/v1/status", get(status))
        .route("/v1/jobs", get(list_jobs).post(open_job).delete(close_all))
        .route("/v1/jobs/close-expired", post(close_expired))
        .route("/v1/jobs/{id}", get(get_job).delete(close_job))
        .route("/v1/jobs/{id}/execute", post(execute))
        .route("/v1/jobs/{id}/renew", post(renew))
        .route("/v1/jobs/{id}/expiration", get(expiration))
        .route("/v1/jobs/{id}/diag", get(diag))
        .route("/v1/batch", post(batch))
        .route("/v1/arbiter/stats", get(arbiter_stats))
        .route("/v1/arbiter/settings", post(arbiter_settings))
        .with_state(arbiter)
}

pub async fn run(
    arbiter: Shared,
    listener: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let a = arbiter.clone();
    tokio::task::spawn_blocking(move || a.fill_pool());
    let a = arbiter.clone();
    let interval = Duration::from_millis(a.inner.lock().settings.monitor_interval_ms.max(50));
    let monitor = tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        let mut sweeps = 0u64;
        loop {
            tick.tick().await;
            let a2 = a.clone();
            let _ = tokio::task::spawn_blocking(move || a2.monitor()).await;
            sweeps += 1;
            if sweeps.is_multiple_of((60_000 / interval.as_millis().max(1) as u64).max(1)) {
                let a3 = a.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let jobs: Vec<String> = a3.inner.lock().jobs.keys().cloned().collect();
                    for job in jobs {
                        if let Some(addr) = a3.job_addr(&job) {
                            let left = call(
                                Method::GET,
                                addr,
                                &format!("/v1/jobs/{job}/expiration"),
                                None,
                            )
                            .ok()
                            .and_then(|(_, v)| v["expirationInSeconds"].as_f64());
                            if left.is_none_or(|s| s <= 0.0) {
                                let _ =
                                    call(Method::DELETE, addr, &format!("/v1/jobs/{job}"), None);
                                a3.forget_job(&job);
                            }
                        }
                    }
                })
                .await;
            }
        }
    });
    let result = axum::serve(listener, router(arbiter.clone()))
        .with_graceful_shutdown(shutdown)
        .await;
    monitor.abort();
    let a = arbiter.clone();
    let _ = tokio::task::spawn_blocking(move || a.shutdown()).await;
    result
}
