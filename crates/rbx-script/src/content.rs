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
        if let Ok(data) = std::fs::read(&path) {
            return Some(data);
        }
        let key = format!("rbxasset://{}", rest.replace('\\', "/"));
        if let Some((_, id)) = crate::legacy_content::LEGACY_CONTENT
            .iter()
            .find(|(k, _)| *k == key)
            && let Some(data) = fetch_asset(e, *id)
        {
            return Some(data);
        }
        if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("wav"))
        {
            for ext in ["mp3", "ogg"] {
                if let Ok(data) = std::fs::read(path.with_extension(ext)) {
                    return Some(data);
                }
            }
        }
        return None;
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

fn fetch_asset(e: &Engine, id: u64) -> Option<Vec<u8>> {
    for dir in &e.asset_dirs {
        if let Ok(data) = std::fs::read(dir.join(id.to_string())) {
            return Some(data);
        }
    }
    std::fs::read(cache_path(e, id)?).ok()
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
