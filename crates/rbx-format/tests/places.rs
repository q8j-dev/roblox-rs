use rbx_format::{BinaryWriteOptions, Tree, read, write_binary, write_xml};
use rbx_reflection::Variant;
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
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if matches!(ext.as_str(), "rbxl" | "rbxm" | "rbxlx" | "rbxmx") {
                out.push(path);
            }
        }
    }
}

pub fn place_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["tests", "assets", "legacy"] {
        collect(&root.join(dir), &mut files);
    }
    files.sort();
    files
}

fn normalize_value(v: &mut Variant, quantize_colors: bool) {
    let replacement = match v {
        Variant::ProtectedString(s) | Variant::Content(s) => {
            Some(Variant::String(std::mem::take(s)))
        }
        Variant::BinaryString(b) => std::str::from_utf8(b)
            .ok()
            .map(|s| Variant::String(s.to_string())),
        Variant::Color3(c) if quantize_colors => Some(Variant::Color3(c.to_u8().to_color3())),
        Variant::BrickColor(b) if quantize_colors => Some(Variant::Int(b.number() as i32)),
        Variant::Float(f) if f.is_nan() => Some(Variant::Float(0.0)),
        _ => None,
    };
    if let Some(r) = replacement {
        *v = r;
    }
}

fn normalize_with(tree: &Tree, quantize_colors: bool) -> Tree {
    let mut t = tree.clone();
    for inst in t.instances.iter_mut() {
        inst.properties.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, v) in inst.properties.iter_mut() {
            normalize_value(v, quantize_colors);
        }
        inst.service = false;
        inst.service_rooted = false;
    }
    t
}

fn normalize(tree: &Tree) -> Tree {
    normalize_with(tree, false)
}

fn canonical(tree: &Tree) -> Tree {
    let order = tree.preorder();
    let mut remap = vec![usize::MAX; tree.instances.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    let mut out = Tree::new();
    for &old in &order {
        let src = &tree.instances[old];
        let mut inst = src.clone();
        inst.children = src.children.iter().map(|&c| remap[c]).collect();
        inst.parent = src.parent.map(|p| remap[p]);
        for (_, v) in inst.properties.iter_mut() {
            if let rbx_reflection::Variant::Ref(Some(id)) = v {
                let i = Tree::index_of(*id);
                *v = rbx_reflection::Variant::Ref(
                    remap
                        .get(i)
                        .filter(|&&n| n != usize::MAX)
                        .map(|&n| Tree::id_of(n)),
                );
            }
        }
        out.instances.push(inst);
    }
    out.roots = tree.roots.iter().map(|&r| remap[r]).collect();
    normalize(&out)
}

fn subset_difference(original: &Tree, back: &Tree) -> Option<String> {
    if original.instances.len() != back.instances.len() || original.roots != back.roots {
        return Some(format!(
            "instance count {} vs {}",
            original.instances.len(),
            back.instances.len()
        ));
    }
    for (i, (x, y)) in original.instances.iter().zip(&back.instances).enumerate() {
        if x.class != y.class || x.children != y.children {
            return Some(format!("#{i} {} structure differs", x.class));
        }
        for (name, v) in &x.properties {
            match y.get(name) {
                Some(w) if w == v => {}
                other => return Some(format!("#{i} {}.{name}: {v:?} vs {other:?}", x.class)),
            }
        }
    }
    None
}

fn first_difference(a: &Tree, b: &Tree) -> String {
    if a.instances.len() != b.instances.len() {
        return format!(
            "instance count {} vs {}",
            a.instances.len(),
            b.instances.len()
        );
    }
    for (i, (x, y)) in a.instances.iter().zip(&b.instances).enumerate() {
        if x.class != y.class {
            return format!("#{i} class {} vs {}", x.class, y.class);
        }
        if x.children != y.children {
            return format!("#{i} {} children differ", x.class);
        }
        for (name, v) in &x.properties {
            match y.get(name) {
                Some(w) if w == v => {}
                other => return format!("#{i} {}.{name}: {v:?} vs {other:?}", x.class),
            }
        }
        for (name, _) in &y.properties {
            if x.get(name).is_none() {
                return format!("#{i} {}.{name} missing", x.class);
            }
        }
    }
    "roots differ".to_string()
}

#[test]
fn every_place_file_round_trips() {
    let files = place_files();
    assert!(!files.is_empty(), "no place files found");
    let mut failures = Vec::new();
    let mut instances = 0usize;
    for path in &files {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: read: {e}", path.display()));
                continue;
            }
        };
        let tree = match read(&data) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: load: {e}", path.display()));
                continue;
            }
        };
        instances += tree.instances.len();
        let binary = match write_binary(&tree, BinaryWriteOptions::default()) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{}: write binary: {e}", path.display()));
                continue;
            }
        };
        match read(&binary) {
            Ok(back) => {
                let a = canonical(&back);
                let b = canonical(&tree);
                if let Some(diff) = subset_difference(&b, &a) {
                    failures.push(format!(
                        "{}: binary round trip differs: {diff}",
                        path.display()
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: reload binary: {e}", path.display())),
        }
        let xml = write_xml(&tree);
        match read(xml.as_bytes()) {
            Ok(back) => {
                let a = normalize_with(&canonical(&back), true);
                let b = normalize_with(&canonical(&tree), true);
                if a != b {
                    failures.push(format!(
                        "{}: xml round trip differs: {}",
                        path.display(),
                        first_difference(&a, &b)
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: reload xml: {e}", path.display())),
        }
    }
    eprintln!("{} files, {} instances", files.len(), instances);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
