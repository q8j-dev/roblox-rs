pub mod job;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use job::{JobHandle, JobInfo, JobSpec};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, PathBuf};
use std::sync::Arc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Config {
    pub http: SocketAddr,
    pub game_bind: IpAddr,
    pub token: Option<String>,
    pub content_dir: Option<PathBuf>,
    pub asset_dirs: Vec<PathBuf>,
    pub place_dirs: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            http: SocketAddr::from(([127, 0, 0, 1], 64989)),
            game_bind: IpAddr::from([0, 0, 0, 0]),
            token: None,
            content_dir: None,
            asset_dirs: Vec::new(),
            place_dirs: Vec::new(),
        }
    }
}

pub struct AppState {
    config: Config,
    jobs: Mutex<HashMap<String, Arc<JobHandle>>>,
}

type Shared = Arc<AppState>;

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn not_found(what: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("{what} not found"))
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub id: String,
    #[serde(default = "default_expiration")]
    pub expiration_in_seconds: f64,
    #[serde(default)]
    pub category: i32,
    #[serde(default = "default_cores")]
    pub cores: f64,
    #[serde(default)]
    pub place: Option<String>,
}

fn default_expiration() -> f64 {
    600.0
}

fn default_cores() -> f64 {
    1.0
}

#[derive(Deserialize, Default)]
pub struct ScriptRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub source: String,
    #[serde(default)]
    pub arguments: Vec<Value>,
}

#[derive(Deserialize)]
pub struct OpenJob {
    pub job: JobRequest,
    #[serde(default)]
    pub script: Option<ScriptRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Renew {
    pub expiration_in_seconds: f64,
}

#[derive(Deserialize)]
pub struct DiagQuery {
    #[serde(default, rename = "type")]
    pub ty: i32,
}

#[derive(Deserialize)]
pub struct AssetQuery {
    pub id: u64,
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let p = std::path::Path::new(path);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

impl AppState {
    fn job(&self, id: &str) -> Result<Arc<JobHandle>, ApiError> {
        self.jobs
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| not_found("job"))
    }

    fn resolve_place(&self, place: &str) -> Result<PathBuf, ApiError> {
        let rel = safe_relative(place).ok_or_else(|| bad("place must be a relative path"))?;
        for dir in &self.config.place_dirs {
            let candidate = dir.join(&rel);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(not_found("place"))
    }

    fn open(&self, req: OpenJob) -> Result<(JobInfo, Vec<Value>), ApiError> {
        if req.job.id.is_empty() {
            return Err(bad("job id is empty"));
        }
        if self.jobs.lock().contains_key(&req.job.id) {
            return Err(ApiError(StatusCode::CONFLICT, "job already exists".into()));
        }
        let place = match &req.job.place {
            Some(p) => Some(self.resolve_place(p)?),
            None => None,
        };
        let spec = JobSpec {
            id: req.job.id.clone(),
            expiration: req.job.expiration_in_seconds,
            category: req.job.category,
            cores: req.job.cores,
            place,
        };
        let handle = JobHandle::open(spec, self.config.game_bind, self.config.content_dir.clone())
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let handle = Arc::new(handle);
        self.jobs.lock().insert(req.job.id.clone(), handle.clone());
        let results = match req.script {
            Some(s) => handle
                .execute(
                    s.name.as_deref().unwrap_or("Script"),
                    &s.source,
                    s.arguments,
                )
                .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?,
            None => Vec::new(),
        };
        let info = handle.info.lock().clone();
        Ok((info, results))
    }

    fn close(&self, id: &str) -> bool {
        let handle = self.jobs.lock().remove(id);
        match handle {
            Some(h) => {
                if let Ok(h) = Arc::try_unwrap(h) {
                    h.close();
                }
                true
            }
            None => false,
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn hello() -> &'static str {
    "Hello World"
}

async fn version() -> Json<Value> {
    Json(json!({ "version": VERSION }))
}

async fn status(State(s): State<Shared>) -> Json<Value> {
    let count = s.jobs.lock().len();
    Json(json!({ "version": VERSION, "environmentCount": count }))
}

async fn list_jobs(State(s): State<Shared>) -> Json<Value> {
    let jobs: Vec<JobInfo> = s
        .jobs
        .lock()
        .values()
        .map(|j| j.info.lock().clone())
        .collect();
    Json(json!(jobs))
}

async fn open_job(
    State(s): State<Shared>,
    Json(req): Json<OpenJob>,
) -> Result<Json<Value>, ApiError> {
    let (info, results) = blocking(move || s.open(req)).await??;
    Ok(Json(json!({ "job": info, "results": results })))
}

async fn get_job(
    State(s): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<JobInfo>, ApiError> {
    Ok(Json(s.job(&id)?.info.lock().clone()))
}

async fn execute(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Json(req): Json<ScriptRequest>,
) -> Result<Json<Value>, ApiError> {
    let job = s.job(&id)?;
    let results = blocking(move || {
        job.execute(
            req.name.as_deref().unwrap_or("Script"),
            &req.source,
            req.arguments,
        )
    })
    .await?
    .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    Ok(Json(json!({ "results": results })))
}

async fn renew(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Json(req): Json<Renew>,
) -> Result<Json<Value>, ApiError> {
    let job = s.job(&id)?;
    job.renew(req.expiration_in_seconds);
    Ok(Json(json!({ "expirationInSeconds": job.expiration() })))
}

async fn expiration(
    State(s): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "expirationInSeconds": s.job(&id)?.expiration() }),
    ))
}

async fn diag(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<DiagQuery>,
) -> Result<Json<Value>, ApiError> {
    let job = s.job(&id)?;
    let mut d = blocking(move || job.diag()).await?.unwrap_or(Value::Null);
    if let Value::Object(m) = &mut d {
        m.insert("type".into(), json!(q.ty));
    }
    Ok(Json(d))
}

async fn close_job(
    State(s): State<Shared>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if blocking(move || s.close(&id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("job"))
    }
}

async fn close_expired(State(s): State<Shared>) -> Result<Json<Value>, ApiError> {
    let n = blocking(move || {
        let expired: Vec<String> = s
            .jobs
            .lock()
            .iter()
            .filter(|(_, j)| j.expired())
            .map(|(k, _)| k.clone())
            .collect();
        expired.iter().filter(|id| s.close(id)).count()
    })
    .await?;
    Ok(Json(json!({ "closed": n })))
}

async fn close_all(State(s): State<Shared>) -> Result<Json<Value>, ApiError> {
    let n = blocking(move || {
        let ids: Vec<String> = s.jobs.lock().keys().cloned().collect();
        ids.iter().filter(|id| s.close(id)).count()
    })
    .await?;
    Ok(Json(json!({ "closed": n })))
}

async fn batch(State(s): State<Shared>, Json(req): Json<OpenJob>) -> Result<Json<Value>, ApiError> {
    let id = req.job.id.clone();
    let r = blocking(move || {
        let r = s.open(req);
        s.close(&id);
        r
    })
    .await??;
    Ok(Json(json!({ "results": r.1 })))
}

fn file_response(path: &std::path::Path) -> Option<Response> {
    let bytes = std::fs::read(path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "lua" | "txt" => "text/plain; charset=utf-8",
        "xml" | "rbxmx" | "rbxlx" => "application/xml",
        "json" => "application/json",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    };
    Some(([(header::CONTENT_TYPE, mime)], Body::from(bytes)).into_response())
}

async fn asset(State(s): State<Shared>, Query(q): Query<AssetQuery>) -> Result<Response, ApiError> {
    for dir in &s.config.asset_dirs {
        let base = dir.join(q.id.to_string());
        if base.is_file()
            && let Some(r) = file_response(&base)
        {
            return Ok(r);
        }
        if base.is_dir()
            && let Ok(entries) = std::fs::read_dir(&base)
        {
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            files.sort();
            if let Some(r) = files.first().and_then(|f| file_response(f)) {
                return Ok(r);
            }
        }
    }
    Err(not_found("asset"))
}

async fn content(State(s): State<Shared>, Path(path): Path<String>) -> Result<Response, ApiError> {
    let dir = s
        .config
        .content_dir
        .as_ref()
        .ok_or_else(|| not_found("content"))?;
    let rel = safe_relative(&path).ok_or_else(|| bad("bad path"))?;
    file_response(&dir.join(rel)).ok_or_else(|| not_found("content"))
}

async fn auth(State(s): State<Shared>, headers: HeaderMap, req: Request, next: Next) -> Response {
    if let Some(token) = &s.config.token {
        let ok = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|t| t == token);
        if !ok {
            return ApiError(StatusCode::UNAUTHORIZED, "missing or wrong token".into())
                .into_response();
        }
    }
    next.run(req).await
}

pub fn router(config: Config) -> Router {
    let state: Shared = Arc::new(AppState {
        config,
        jobs: Mutex::new(HashMap::new()),
    });
    let admin = Router::new()
        .route("/v1/hello", get(hello))
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
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    let public = Router::new()
        .route("/asset", get(asset))
        .route("/content/{*path}", get(content));
    admin.merge(public).with_state(state)
}

pub struct ServerHandle {
    pub http: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ServerHandle {
    pub fn stop(mut self) {
        self.shutdown_now();
    }

    fn shutdown_now(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

pub async fn serve(
    config: Config,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.http).await?;
    tracing::info!("admin API listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(config))
        .with_graceful_shutdown(shutdown)
        .await
}

pub fn spawn(config: Config) -> std::io::Result<ServerHandle> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let listener = rt.block_on(tokio::net::TcpListener::bind(config.http))?;
    let http = listener.local_addr()?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("server-http".into())
        .spawn(move || {
            rt.block_on(async move {
                let _ = axum::serve(listener, router(config))
                    .with_graceful_shutdown(async move {
                        let _ = rx.await;
                    })
                    .await;
            });
        })?;
    Ok(ServerHandle {
        http,
        shutdown: Some(tx),
        thread: Some(thread),
    })
}
