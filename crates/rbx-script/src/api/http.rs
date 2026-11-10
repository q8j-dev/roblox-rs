use super::{Registry, Ret, ret};
use crate::api::remote::{from_remote, to_remote};
use crate::convert::{arg_bool, arg_number, arg_string};
use crate::runtime::{Engine, Resume, engine, make_ready_lua, with_state};
use crate::types::rt;
use mlua::{Lua, MultiValue, Result, Thread, Value};
use rbx_base::time::{Duration, Instant};
use rbx_dom::{InstanceId, RemoteValue, Variant};
use serde_json::{Map, Value as Json};
use std::sync::mpsc;

pub const DEFAULT_BASE_URL: &str = "http://localhost:64989/";
const REQUESTS_PER_MINUTE: usize = 500;
const LEGACY_HOSTS: &[&str] = &[
    "www.roblox.com",
    "api.roblox.com",
    "assetgame.roblox.com",
    "roblox.com",
    "www.api.roblox.com",
];

#[derive(Clone, Copy)]
pub enum Decode {
    Text,
    Json,
}

pub struct HttpWait {
    rx: mpsc::Receiver<std::result::Result<Vec<u8>, String>>,
    thread: Thread,
    decode: Decode,
}

#[derive(Default)]
pub struct HttpState {
    pub waits: Vec<HttpWait>,
    pub recent: std::collections::VecDeque<Instant>,
}

pub fn resolve(e: &Engine, url: &str) -> String {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    for scheme in ["http://", "https://"] {
        if let Some(rest) = lower.strip_prefix(scheme) {
            let host_end = rest.find('/').unwrap_or(rest.len());
            let host = &rest[..host_end];
            let local_subdomain = host
                .split(':')
                .next()
                .is_some_and(|h| h.ends_with(".localhost"));
            if LEGACY_HOSTS.contains(&host) || local_subdomain {
                let path = &url[scheme.len() + host_end..];
                let base = e.base_url.trim_end_matches('/');
                return format!("{base}{path}");
            }
        }
    }
    url.to_string()
}

pub fn site_base_url(base: &str) -> String {
    for scheme in ["http://", "https://"] {
        if let Some(rest) = base.strip_prefix(scheme) {
            let host_end = rest.find('/').unwrap_or(rest.len());
            let (host, tail) = rest.split_at(host_end);
            let name = host.split(':').next().unwrap_or(host);
            if !name.contains('.') {
                let tail = if tail.is_empty() { "/" } else { tail };
                return format!("{scheme}www.{host}{tail}");
            }
        }
    }
    base.to_string()
}

fn query_number(url: &str, key: &str) -> Option<u64> {
    let lower = url.to_ascii_lowercase();
    let q = lower.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

fn fixture(e: &Engine, url: &str) -> Option<Vec<u8>> {
    let lower = url.to_ascii_lowercase();
    let path = lower.split_once("://").map(|(_, r)| r).unwrap_or(&lower);
    let path = path.split_once('/').map(|(_, p)| p).unwrap_or("");
    let (dir, id) = if path.starts_with("marketplace/productinfo") {
        ("productinfo", query_number(url, "assetid")?)
    } else if path.starts_with("marketplace/productdetails") {
        ("productdetails", query_number(url, "productid")?)
    } else {
        return crate::content::fetch(e, url);
    };
    e.asset_dirs
        .iter()
        .find_map(|d| std::fs::read(d.join(dir).join(format!("{id}.json"))).ok())
}

#[cfg(not(target_arch = "wasm32"))]
fn request(url: String, body: Option<(String, String)>) -> std::result::Result<Vec<u8>, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent: ureq::Agent = config.into();
    let response = match body {
        Some((data, content_type)) => agent
            .post(&url)
            .header("Content-Type", &content_type)
            .send(data.as_bytes()),
        None => agent.get(&url).call(),
    };
    let mut r = response.map_err(|e| format!("HttpError: {e}"))?;
    r.body_mut()
        .read_to_vec()
        .map_err(|e| format!("HttpError: {e}"))
}

pub fn request_blocking(
    url: String,
    body: Option<(String, String)>,
) -> std::result::Result<Vec<u8>, String> {
    request(url, body)
}

#[cfg(target_arch = "wasm32")]
fn request(url: String, _body: Option<(String, String)>) -> std::result::Result<Vec<u8>, String> {
    Err(format!(
        "HttpError: {url} cannot be fetched from this build"
    ))
}

fn start(lua: &Lua, url: String, body: Option<(String, String)>, decode: Decode) -> Result<Ret> {
    let thread = lua.current_thread();
    let local = {
        let eng = engine(lua);
        let e = eng.lock();
        if body.is_none() {
            fixture(&e, &url)
        } else {
            None
        }
    };
    let (tx, rx) = mpsc::channel();
    match local {
        Some(data) => {
            let _ = tx.send(Ok(data));
        }
        None => {
            let spawned = std::thread::Builder::new()
                .name("http".into())
                .spawn(move || {
                    let _ = tx.send(request(url, body));
                });
            if let Err(e) = spawned {
                return Err(rt(format!("HttpError: {e}")));
            }
        }
    }
    with_state(lua, |s| s.http.waits.push(HttpWait { rx, thread, decode }));
    Ok(Ret::Yield)
}

pub fn poll(lua: &Lua) {
    let done: Vec<(HttpWait, std::result::Result<Vec<u8>, String>)> = with_state(lua, |s| {
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.http.waits.len() {
            match s.http.waits[i].rx.try_recv() {
                Ok(r) => out.push((s.http.waits.swap_remove(i), r)),
                Err(mpsc::TryRecvError::Disconnected) => out.push((
                    s.http.waits.swap_remove(i),
                    Err("HttpError: request was abandoned".into()),
                )),
                Err(mpsc::TryRecvError::Empty) => i += 1,
            }
        }
        out
    });
    for (wait, result) in done {
        let value = result.and_then(|bytes| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            match wait.decode {
                Decode::Text => lua
                    .create_string(&text)
                    .map(Value::String)
                    .map_err(|e| e.to_string()),
                Decode::Json => decode_json(lua, &text).map_err(|e| e.to_string()),
            }
        });
        match value {
            Ok(v) => make_ready_lua(lua, wait.thread, MultiValue::from_vec(vec![v])),
            Err(msg) => with_state(lua, |s| {
                s.ready.push_back((wait.thread, Resume::Error(msg)))
            }),
        }
    }
}

fn json_to_remote(v: &Json) -> RemoteValue {
    match v {
        Json::Null => RemoteValue::Value(Variant::Nil),
        Json::Bool(b) => RemoteValue::Value(Variant::Bool(*b)),
        Json::Number(n) => RemoteValue::Value(Variant::Double(n.as_f64().unwrap_or(0.0))),
        Json::String(s) => RemoteValue::Value(Variant::String(s.clone())),
        Json::Array(a) => RemoteValue::Array(a.iter().map(json_to_remote).collect()),
        Json::Object(m) => RemoteValue::Map(
            m.iter()
                .map(|(k, v)| (k.clone(), json_to_remote(v)))
                .collect(),
        ),
    }
}

fn remote_to_json(v: &RemoteValue) -> std::result::Result<Json, String> {
    Ok(match v {
        RemoteValue::Value(v) => match v {
            Variant::Nil => Json::Null,
            Variant::Bool(b) => Json::Bool(*b),
            Variant::Double(f) => {
                if f.fract() == 0.0 && f.abs() < 9.0e15 {
                    Json::from(*f as i64)
                } else {
                    serde_json::Number::from_f64(*f)
                        .map(Json::Number)
                        .ok_or("Can't convert to JSON")?
                }
            }
            Variant::String(s) => Json::String(s.clone()),
            _ => return Err("Can't convert to JSON".into()),
        },
        RemoteValue::Enum { .. } => return Err("Can't convert to JSON".into()),
        RemoteValue::Array(items) => Json::Array(
            items
                .iter()
                .map(remote_to_json)
                .collect::<std::result::Result<_, _>>()?,
        ),
        RemoteValue::Map(items) => {
            let mut m = Map::new();
            for (k, v) in items {
                m.insert(k.clone(), remote_to_json(v)?);
            }
            Json::Object(m)
        }
    })
}

pub fn decode_json(lua: &Lua, text: &str) -> Result<Value> {
    let parsed: Json = serde_json::from_str(text).map_err(|_| rt("Can't parse JSON"))?;
    from_remote(lua, &json_to_remote(&parsed))
}

fn check(lua: &Lua, id: InstanceId, url: &str) -> Result<String> {
    if url.trim().is_empty() {
        return Err(rt("Empty URL"));
    }
    let eng = engine(lua);
    let e = eng.lock();
    if !e.dm.get_bool(id, "HttpEnabled") {
        return Err(rt("Http requests are not enabled"));
    }
    if !e.backend_processing() {
        return Err(rt("Http requests can only be executed by game server"));
    }
    let resolved = resolve(&e, url);
    drop(e);
    let now = Instant::now();
    let allowed = with_state(lua, |s| {
        while s
            .http
            .recent
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            s.http.recent.pop_front();
        }
        if s.http.recent.len() >= REQUESTS_PER_MINUTE {
            return false;
        }
        s.http.recent.push_back(now);
        true
    });
    if !allowed {
        return Err(rt("Number of requests exceeded limit"));
    }
    Ok(resolved)
}

fn content_type(lua: &Lua, v: Option<&Value>) -> String {
    let n = match v {
        Some(Value::UserData(ud)) => ud.borrow::<crate::types::LEnumItem>().ok().map(|i| i.value),
        Some(Value::Number(n)) => Some(*n as u32),
        Some(Value::Integer(n)) => Some(*n as u32),
        _ => None,
    };
    let _ = lua;
    match n.unwrap_or(0) {
        1 => "application/xml",
        2 => "application/x-www-form-urlencoded",
        3 => "text/plain",
        4 => "text/xml",
        _ => "application/json",
    }
    .to_string()
}

fn get_async(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let resolved = check(lua, id, &url)?;
    start(lua, resolved, None, Decode::Text)
}

fn post_async(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let data = arg_string(args.get(1)).unwrap_or_default();
    let resolved = check(lua, id, &url)?;
    let ct = content_type(lua, args.get(2));
    start(lua, resolved, Some((data, ct)), Decode::Text)
}

fn json_decode(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).ok_or_else(|| rt("Can't parse JSON"))?;
    ret(lua, decode_json(lua, &text)?)
}

fn json_encode(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let v = args.first().cloned().unwrap_or(Value::Nil);
    if let Value::String(text) = &v {
        let text = String::from_utf8_lossy(&text.as_bytes()).into_owned();
        return ret(lua, Json::String(text).to_string());
    }
    if !matches!(v, Value::Table(_)) {
        return Err(rt("Can't convert to JSON"));
    }
    let remote = to_remote(&v, 0)?;
    let json = remote_to_json(&remote).map_err(rt)?;
    ret(lua, json.to_string())
}

fn url_encode(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    ret(lua, out)
}

fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        rbx_base::time::SystemTime::now()
            .duration_since(rbx_base::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

fn generate_guid(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let wrap = args.first().is_none_or(|v| arg_bool(Some(v)));
    let a = random_u64();
    let b = random_u64();
    let s = format!(
        "{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        (a as u16) & 0x0FFF,
        ((b >> 48) as u16 & 0x3FFF) | 0x8000,
        b & 0xFFFF_FFFF_FFFF
    );
    ret(lua, if wrap { format!("{{{s}}}") } else { s })
}

fn game_http_get(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let resolved = resolve(&engine(lua).lock(), &url);
    start(lua, resolved, None, Decode::Text)
}

fn game_http_post(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let data = arg_string(args.get(1)).unwrap_or_default();
    let ct = arg_string(args.get(3)).unwrap_or_else(|| "*/*".into());
    let resolved = resolve(&engine(lua).lock(), &url);
    start(lua, resolved, Some((data, ct)), Decode::Text)
}

fn set_base_url(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let eng = engine(lua);
    let mut e = eng.lock();
    let normalized = if url.ends_with('/') {
        url
    } else {
        format!("{url}/")
    };
    e.base_url = resolve(&e, &normalized);
    let base = e.base_url.clone();
    e.dm.set_unchecked(id, "BaseUrl", Variant::String(base));
    super::none()
}

fn get_product_info(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let id = arg_number(args.first()).unwrap_or(0.0);
    let info = match args.get(1) {
        Some(Value::UserData(ud)) => ud
            .borrow::<crate::types::LEnumItem>()
            .map(|i| i.value)
            .unwrap_or(0),
        Some(v) => arg_number(Some(v)).unwrap_or(0.0) as u32,
        None => 0,
    };
    if id <= 0.0 {
        return Err(rt(
            "MarketplaceService:GetProductInfo() argument is not a valid assetId (supplied assetId was less than 0)",
        ));
    }
    let base = engine(lua).lock().base_url.clone();
    let base = base.trim_end_matches('/');
    let url = match info {
        1 => format!("{base}/marketplace/productDetails?productId={}", id as u64),
        _ => format!("{base}/marketplace/productinfo?assetId={}", id as u64),
    };
    start(lua, url, None, Decode::Json)
}

pub fn register(r: &mut Registry) {
    r.add("HttpService", &["GetAsync"], get_async);
    r.add("HttpService", &["PostAsync"], post_async);
    r.add("HttpService", &["JSONDecode"], json_decode);
    r.add("HttpService", &["JSONEncode"], json_encode);
    r.add("HttpService", &["UrlEncode"], url_encode);
    r.add("HttpService", &["GenerateGUID"], generate_guid);
    r.add("DataModel", &["HttpGet", "HttpGetAsync"], game_http_get);
    r.add("DataModel", &["HttpPost", "HttpPostAsync"], game_http_post);
    r.add("ContentProvider", &["SetBaseUrl"], set_base_url);
    r.add("MarketplaceService", &["GetProductInfo"], get_product_info);
}

#[cfg(test)]
mod site_tests {
    use super::site_base_url;

    #[test]
    fn dotless_hosts_get_a_www_label() {
        assert_eq!(
            site_base_url("http://localhost:64989/"),
            "http://www.localhost:64989/"
        );
        assert_eq!(
            site_base_url("https://www.example.test/"),
            "https://www.example.test/"
        );
    }
}
