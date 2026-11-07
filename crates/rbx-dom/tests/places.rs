use rbx_dom::{DataModel, Variant, coerce_to, database};
use rbx_format::Tree;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(here)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && matches!(
                ext.to_ascii_lowercase().as_str(),
                "rbxl" | "rbxm" | "rbxlx" | "rbxmx"
            )
        {
            out.push(path);
        }
    }
}

fn place_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["tests", "assets", "legacy"] {
        collect(&root.join(dir), &mut files);
    }
    let excluded: Vec<String> =
        std::fs::read_to_string(root.join("tests").join("excluded_places.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').next())
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect();
    files.retain(|f| {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !excluded.iter().any(|e| e == name)
    });
    files.sort();
    files
}

fn is_place(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("rbxl" | "rbxlx")
    )
}

type CanonicalNode = (String, String, Vec<(String, Variant)>, Vec<usize>);

fn canonical(tree: &Tree) -> Vec<CanonicalNode> {
    let order = tree.preorder();
    let mut remap = vec![usize::MAX; tree.instances.len()];
    for (n, &o) in order.iter().enumerate() {
        remap[o] = n;
    }
    order
        .iter()
        .map(|&i| {
            let inst = &tree.instances[i];
            let mut props: Vec<(String, Variant)> = inst
                .properties
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Variant::Ref(Some(r)) => Variant::Ref(
                            remap
                                .get(Tree::index_of(*r))
                                .filter(|&&n| n != usize::MAX)
                                .map(|&n| Tree::id_of(n)),
                        ),
                        Variant::Float(f) if f.is_nan() => Variant::Float(0.0),
                        v => v.clone(),
                    };
                    (k.clone(), v)
                })
                .collect();
            props.sort_by(|a, b| a.0.cmp(&b.0));
            (
                inst.class.clone(),
                inst.name().to_string(),
                props,
                inst.children.iter().map(|&c| remap[c]).collect(),
            )
        })
        .collect()
}

fn load(data: &[u8], place: bool) -> Result<(DataModel, Tree), String> {
    let mut dm = DataModel::new();
    let tree = rbx_format::read(data).map_err(|e| e.to_string())?;
    if place {
        let root = dm.root();
        dm.load_tree(
            &tree,
            Some(root),
            rbx_dom::LoadOptions {
                merge_services: true,
            },
        )
        .map_err(|e| e.to_string())?;
        let saved = dm.save_place_tree();
        Ok((dm, saved))
    } else {
        let (tops, _) = dm
            .load_tree(&tree, None, rbx_dom::LoadOptions::default())
            .map_err(|e| e.to_string())?;
        for &t in &tops {
            dm.retain(t);
        }
        let saved = dm.save_tree(&tops, rbx_dom::SaveOptions::default());
        Ok((dm, saved))
    }
}

fn lost_properties(original: &Tree, saved: &Tree) -> Option<String> {
    let mut by_path: std::collections::HashMap<Vec<(String, String)>, Vec<usize>> =
        std::collections::HashMap::new();
    fn path(tree: &Tree, mut i: usize) -> Vec<(String, String)> {
        let mut p = Vec::new();
        loop {
            p.push((
                tree.instances[i].class.clone(),
                tree.instances[i].name().to_string(),
            ));
            match tree.instances[i].parent {
                Some(par) => i = par,
                None => break,
            }
        }
        p
    }
    for i in saved.preorder() {
        by_path.entry(path(saved, i)).or_default().push(i);
    }
    let db = database();
    let orig_order = original.preorder();
    let saved_order = saved.preorder();
    let aligned = orig_order.len() == saved_order.len()
        && orig_order
            .iter()
            .zip(&saved_order)
            .all(|(&a, &b)| original.instances[a].class == saved.instances[b].class);
    for (k, &i) in orig_order.iter().enumerate() {
        let o = &original.instances[i];
        let s_index = if aligned {
            saved_order[k]
        } else {
            let Some(candidates) = by_path.get(&path(original, i)) else {
                continue;
            };
            if candidates.len() != 1 {
                continue;
            }
            candidates[0]
        };
        let s = &saved.instances[s_index];
        for (name, value) in &o.properties {
            if matches!(value, Variant::Ref(_)) || name == "Name" {
                continue;
            }
            let Some(desc) = db.find_property(&o.class, name) else {
                continue;
            };
            if !desc.serialized() || desc.read_only {
                continue;
            }
            let Ok(expected) = coerce_to(&desc.ty, value.clone()) else {
                continue;
            };
            let canonical_name = db
                .canonical_property(&o.class, name)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| name.clone());
            let got = s.get(name).or_else(|| s.get(&canonical_name));
            let same = match (got, &expected) {
                (Some(Variant::Float(a)), Variant::Float(b)) => {
                    a == b || (a.is_nan() && b.is_nan())
                }
                (Some(Variant::CFrame(a)), Variant::CFrame(b)) => {
                    (a.translation - b.translation).length() < 1e-4
                        && (a.rotation - b.rotation).abs_diff_eq(rbx_math::Mat3::ZERO, 1e-3)
                }
                (Some(g), e) => g == e,
                (None, _) => false,
            };
            if !same {
                return Some(format!(
                    "{}.{name}: file {expected:?} vs saved {got:?}",
                    o.class
                ));
            }
        }
    }
    None
}

#[test]
fn every_place_loads_and_saves_without_loss() {
    let files = place_files();
    let mut failures = Vec::new();
    let mut loaded = 0;
    for path in &files {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let place = is_place(path);
        let original = match rbx_format::read(&data) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let (_, first) = match load(&data, place) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{}: load: {e}", path.display()));
                continue;
            }
        };
        if let Some(diff) = lost_properties(&original, &first) {
            failures.push(format!("{}: lost on load: {diff}", path.display()));
        }
        let bytes =
            match rbx_format::write_binary(&first, rbx_format::BinaryWriteOptions::default()) {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("{}: save: {e}", path.display()));
                    continue;
                }
            };
        let (_, second) = match load(&bytes, place) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{}: reload: {e}", path.display()));
                continue;
            }
        };
        let a = canonical(&first);
        let b = canonical(&second);
        if a != b {
            let detail = if a.len() != b.len() {
                format!("instance count {} vs {}", a.len(), b.len())
            } else {
                a.iter()
                    .zip(&b)
                    .find(|(x, y)| x != y)
                    .map(|(x, y)| {
                        let p = x.2.iter().find(|(k, v)| {
                            y.2.iter().find(|(k2, _)| k2 == k).map(|(_, v2)| v2) != Some(v)
                        });
                        format!(
                            "{} {}: {:?} vs {:?}",
                            x.0,
                            x.1,
                            p,
                            p.and_then(|(k, _)| y.2.iter().find(|(k2, _)| k2 == k))
                        )
                    })
                    .unwrap_or_default()
            };
            failures.push(format!(
                "{}: save and reload differs: {detail}",
                path.display()
            ));
        }
        loaded += 1;
    }
    eprintln!("{loaded} of {} files loaded", files.len());
    let shown: Vec<&String> = failures.iter().take(60).collect();
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        shown
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
