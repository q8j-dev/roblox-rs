use std::io;
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            out.push(entry?.path());
        }
        Ok(out)
    }

    pub fn is_file(path: &Path) -> bool {
        path.is_file()
    }

    pub fn is_dir(path: &Path) -> bool {
        path.is_dir()
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::collections::{BTreeSet, HashSet};
    use std::sync::OnceLock;

    pub const INDEX_URL: &str = "content-index.txt";

    struct Index {
        files: HashSet<String>,
        dirs: BTreeSet<String>,
    }

    static INDEX: OnceLock<Option<Index>> = OnceLock::new();
    static BASE: OnceLock<String> = OnceLock::new();

    fn base() -> &'static str {
        BASE.get_or_init(|| {
            web_sys::window()
                .and_then(|w| w.location().href().ok())
                .map(|href| {
                    let href = href.split(['?', '#']).next().unwrap_or("").to_string();
                    match href.rfind('/') {
                        Some(i) => href[..=i].to_string(),
                        None => href,
                    }
                })
                .unwrap_or_default()
        })
    }

    fn key(path: &Path) -> String {
        let s = path.to_string_lossy().replace('\\', "/");
        let mut parts: Vec<&str> = Vec::new();
        for p in s.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                p => parts.push(p),
            }
        }
        parts.join("/")
    }

    fn fetch(path: &str) -> Option<Vec<u8>> {
        let url = format!("{}{path}", base());
        let xhr = web_sys::XmlHttpRequest::new().ok()?;
        xhr.open_with_async("GET", &url, false).ok()?;
        xhr.override_mime_type("text/plain; charset=x-user-defined")
            .ok()?;
        xhr.send().ok()?;
        if xhr.status().ok()? != 200 {
            return None;
        }
        let text = xhr.response_text().ok()??;
        Some(text.chars().map(|c| (c as u32 & 0xff) as u8).collect())
    }

    fn index() -> Option<&'static Index> {
        INDEX
            .get_or_init(|| {
                let data = fetch(INDEX_URL)?;
                let text = String::from_utf8(data).ok()?;
                let mut files = HashSet::new();
                let mut dirs = BTreeSet::new();
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let mut dir = String::new();
                    let segments: Vec<&str> = line.split('/').collect();
                    for seg in &segments[..segments.len() - 1] {
                        if !dir.is_empty() {
                            dir.push('/');
                        }
                        dir.push_str(seg);
                        dirs.insert(dir.clone());
                    }
                    files.insert(line.to_string());
                }
                Some(Index { files, dirs })
            })
            .as_ref()
    }

    fn not_found() -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, "not in the content index")
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        let k = key(path);
        if let Some(i) = index()
            && !i.files.contains(&k)
        {
            return Err(not_found());
        }
        fetch(&k).ok_or_else(not_found)
    }

    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let k = key(path);
        let i = index().ok_or_else(not_found)?;
        if !i.dirs.contains(&k) {
            return Err(not_found());
        }
        let prefix = format!("{k}/");
        let mut out = BTreeSet::new();
        for f in i.files.iter().chain(i.dirs.iter()) {
            if let Some(rest) = f.strip_prefix(&prefix)
                && !rest.is_empty()
            {
                let child = rest.split('/').next().unwrap_or(rest);
                out.insert(format!("{prefix}{child}"));
            }
        }
        Ok(out.into_iter().map(PathBuf::from).collect())
    }

    pub fn is_file(path: &Path) -> bool {
        index().is_some_and(|i| i.files.contains(&key(path)))
    }

    pub fn is_dir(path: &Path) -> bool {
        index().is_some_and(|i| i.dirs.contains(&key(path)))
    }
}

pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    imp::read(path.as_ref())
}

pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = imp::read(path.as_ref())?;
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn read_dir(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    imp::read_dir(path.as_ref())
}

pub fn is_file(path: impl AsRef<Path>) -> bool {
    imp::is_file(path.as_ref())
}

pub fn is_dir(path: impl AsRef<Path>) -> bool {
    imp::is_dir(path.as_ref())
}

pub fn exists(path: impl AsRef<Path>) -> bool {
    is_file(path.as_ref()) || is_dir(path.as_ref())
}
