use crate::runtime::Engine;
use rbx_dom::InstanceId;
use std::path::PathBuf;

pub fn normalize_asset_id(url: &str) -> String {
    url.trim().to_string()
}

fn asset_number(url: &str) -> Option<u64> {
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rbxassetid://") {
        return rest.trim().parse().ok();
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let idx = lower.find("id=")?;
        let digits: String = lower[idx + 3..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return digits.parse().ok();
    }
    None
}

pub fn cache_path(e: &Engine, id: u64) -> Option<PathBuf> {
    let root = e.content_root.as_ref()?;
    Some(
        root.parent()
            .unwrap_or(root)
            .join("cache")
            .join(id.to_string()),
    )
}

pub fn fetch(e: &Engine, url: &str) -> Option<Vec<u8>> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    if let Some(data) = e.registered_content.get(url) {
        return Some(data.clone());
    }
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rbxasset://") {
        let root = e.content_root.as_ref()?;
        let rel = &url[url.len() - rest.len()..];
        let path = rel
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "..")
            .fold(root.clone(), |p, s| p.join(s));
        return std::fs::read(path).ok();
    }
    if let Some(id) = asset_number(url) {
        for dir in &e.asset_dirs {
            if let Ok(data) = std::fs::read(dir.join(id.to_string())) {
                return Some(data);
            }
        }
        let path = cache_path(e, id)?;
        return std::fs::read(path).ok();
    }
    None
}

pub fn register(e: &mut Engine, data: Vec<u8>) -> String {
    let id = format!("rbxtemp://{}", e.registered_content.len() + 1);
    e.registered_content.insert(id.clone(), data);
    id
}

pub fn load_instances(e: &mut Engine, url: &str) -> Vec<InstanceId> {
    let Some(data) = fetch(e, url) else {
        return Vec::new();
    };
    match e.dm.load_model(&data) {
        Ok((roots, _)) => roots,
        Err(_) => Vec::new(),
    }
}
