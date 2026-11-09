use rbx_reflection::Variant;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    if s.is_empty() || s == "." || s == ".." {
        format!("_{s}")
    } else {
        s
    }
}

fn source_of(v: &Variant) -> Option<Vec<u8>> {
    match v {
        Variant::String(s) | Variant::ProtectedString(s) => Some(s.as_bytes().to_vec()),
        Variant::BinaryString(b) => Some(b.clone()),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("read");
    let out = PathBuf::from(&args[2]);
    let tree = rbx_format::read(&data).expect("parse");
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); tree.instances.len()];
    for (i, inst) in tree.instances.iter().enumerate() {
        if let Some(p) = inst.parent {
            children[p].push(i);
        }
    }
    let mut props: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    let mut bytecode = 0usize;
    let mut source = 0usize;
    let mut listing = String::new();
    fn name_of(inst: &rbx_format::TreeInstance) -> String {
        inst.get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
    let mut stack: Vec<(usize, PathBuf, usize)> = tree
        .roots
        .iter()
        .rev()
        .map(|&r| (r, out.clone(), 0))
        .collect();
    while let Some((i, dir, depth)) = stack.pop() {
        let inst = &tree.instances[i];
        for (n, v) in &inst.properties {
            *props
                .entry((inst.class.clone(), n.clone()))
                .or_default()
                .entry(format!("{:?}", v.ty()))
                .or_default() += 1;
        }
        let name = safe(&name_of(inst));
        listing.push_str(&format!(
            "{}{} [{}]\n",
            "  ".repeat(depth),
            name,
            inst.class
        ));
        let here = dir.join(&name);
        let is_script = inst.class.ends_with("Script");
        if !children[i].is_empty() || !is_script {
            std::fs::create_dir_all(&here).expect("mkdir");
        }
        if is_script {
            if let Some(src) = inst.get("Source").and_then(source_of) {
                if src.contains(&0) || std::str::from_utf8(&src).is_err() {
                    bytecode += 1;
                } else {
                    source += 1;
                }
                let path: &Path = &if children[i].is_empty() {
                    dir.join(format!("{name}.lua"))
                } else {
                    here.join("init.lua")
                };
                std::fs::write(path, &src).expect("write");
            }
        } else if inst.class == "StringValue" {
            if let Some(v) = inst.get("Value").and_then(|v| v.as_str()) {
                std::fs::write(here.join("Value.txt"), v).expect("write");
            }
        }
        for &c in children[i].iter().rev() {
            stack.push((c, here.clone(), depth + 1));
        }
    }
    std::fs::write(out.join("_tree.txt"), listing).expect("tree");
    let mut p = String::new();
    for ((class, name), tys) in &props {
        p.push_str(&format!("{class}.{name} {tys:?}\n"));
    }
    std::fs::write(out.join("_properties.txt"), p).expect("props");
    println!("scripts with source: {source}, bytecode: {bytecode}");
}
