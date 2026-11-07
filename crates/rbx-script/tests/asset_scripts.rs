use rbx_dom::{DataModel, LoadOptions, Variant};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn compile(lua: &mlua::Lua, name: &str, source: &str) -> Result<(), String> {
    lua.load(source)
        .set_name(format!("={name}"))
        .into_function()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn every_asset_script_compiles() {
    let root = repo_root().join("assets");
    let excluded: Vec<String> =
        std::fs::read_to_string(repo_root().join("tests").join("excluded_places.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.retain(|f| {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !excluded.iter().any(|e| e == name)
    });
    let lua = mlua::Lua::new();
    let mut failures = Vec::new();
    let mut count = 0;
    for path in &files {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        match ext.as_str() {
            "lua" => {
                let source = match std::fs::read(path) {
                    Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                    Err(e) => {
                        failures.push(format!("{rel}: {e}"));
                        continue;
                    }
                };
                count += 1;
                if let Err(e) = compile(&lua, &rel, &source) {
                    failures.push(format!("{rel}: {e}"));
                }
            }
            "rbxm" | "rbxmx" => {
                let Ok(data) = std::fs::read(path) else {
                    continue;
                };
                let tree = match rbx_format::read(&data) {
                    Ok(t) => t,
                    Err(e) => {
                        failures.push(format!("{rel}: {e}"));
                        continue;
                    }
                };
                let mut dm = DataModel::new();
                let tops = match dm.load_tree(&tree, None, LoadOptions::default()) {
                    Ok((tops, _)) => tops,
                    Err(e) => {
                        failures.push(format!("{rel}: {e}"));
                        continue;
                    }
                };
                for top in tops {
                    let mut ids = vec![top];
                    ids.extend(dm.descendants(top));
                    for id in ids {
                        if !dm.is_a(id, "LuaSourceContainer") {
                            continue;
                        }
                        let source = match dm.get(id, "Source") {
                            Ok(Variant::ProtectedString(s)) | Ok(Variant::String(s)) => s,
                            _ => continue,
                        };
                        count += 1;
                        let name = format!("{rel}:{}", dm.full_name(id));
                        if let Err(e) = compile(&lua, &name, &source) {
                            failures.push(format!("{name}: {e}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    eprintln!("{count} asset scripts compiled");
    assert!(
        failures.is_empty(),
        "{} scripts failed to compile:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
