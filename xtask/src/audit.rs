use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[".git", "target", "docs", "dist", "node_modules"];

const WORD_ANY_CASE: &[&str] = &[
    "NetPmc",
    "VMProtect",
    "ProgramMemoryChecker",
    "goldhash",
    "golden hash",
    "golden_hash",
    "QTitan",
    "RakNet",
    "gSOAP",
    "ScriptSigner",
];

const WORD_EXACT_CASE: &[&str] = &["Qt", "Q_OBJECT", "FMOD"];

const SUBSTRINGS: &[&str] = &["todo!(", "unimplemented!("];

pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub pattern: &'static str,
}

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn find_words(haystack: &[u8], needle: &[u8], fold: bool) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    let eq = |a: u8, b: u8| {
        if fold {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    };
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if haystack[i..i + needle.len()]
            .iter()
            .zip(needle)
            .all(|(&a, &b)| eq(a, b))
        {
            let before_ok = i == 0 || !is_word_char(haystack[i - 1]);
            let after = i + needle.len();
            let after_ok = after >= haystack.len() || !is_word_char(haystack[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_binary(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return true;
    };
    let mut buf = [0u8; 8192];
    let Ok(n) = file.read(&mut buf) else {
        return true;
    };
    buf[..n].contains(&0)
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if dir == root && SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            if name == ".git" || name == "target" {
                continue;
            }
            walk(&path, root, out);
        } else if kind.is_file() {
            out.push(path);
        }
    }
}

pub fn scan(root: &Path) -> Vec<Hit> {
    let own = root.join("xtask").join("src").join("audit.rs");
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    let mut hits = Vec::new();
    for path in files {
        if path == own || is_binary(&path) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        for (index, line) in bytes.split(|&b| b == b'\n').enumerate() {
            let checks = WORD_ANY_CASE
                .iter()
                .map(|p| (*p, true, true))
                .chain(WORD_EXACT_CASE.iter().map(|p| (*p, false, true)))
                .chain(SUBSTRINGS.iter().map(|p| (*p, false, false)));
            for (pattern, fold, word) in checks {
                let found = if word {
                    find_words(line, pattern.as_bytes(), fold)
                } else {
                    line.windows(pattern.len()).any(|w| w == pattern.as_bytes())
                };
                if found {
                    hits.push(Hit {
                        path: path.clone(),
                        line: index + 1,
                        pattern,
                    });
                }
            }
        }
    }
    hits
}

pub fn run(root: &Path) -> Result<(), String> {
    let hits = scan(root);
    if hits.is_empty() {
        println!("audit: 0 hits");
        return Ok(());
    }
    let shown = hits.len().min(200);
    for hit in &hits[..shown] {
        let rel = hit.path.strip_prefix(root).unwrap_or(&hit.path);
        println!("{}:{}: {}", rel.display(), hit.line, hit.pattern);
    }
    if hits.len() > shown {
        println!("... {} more", hits.len() - shown);
    }
    Err(format!("audit: {} hits", hits.len()))
}

#[cfg(test)]
mod tests {
    use super::find_words;

    #[test]
    fn whole_words_only() {
        assert!(find_words(b"use Qt here", b"Qt", false));
        assert!(!find_words(b"QtCore", b"Qt", false));
        assert!(!find_words(b"math.fmod(x)", b"FMOD", false));
        assert!(find_words(b"// fmod sound", b"FMOD", true));
        assert!(find_words(b"a golden hash b", b"golden hash", true));
    }
}
