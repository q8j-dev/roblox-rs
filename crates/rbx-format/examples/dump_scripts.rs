use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("rbxl" | "rbxm" | "rbxlx" | "rbxmx" | "clienttest")
            ) {
                out.push(p);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    for a in &args[1..args.len() - 1] {
        walk(Path::new(a), &mut files);
    }
    let out = Path::new(&args[args.len() - 1]);
    let _ = std::fs::create_dir_all(out);
    let mut n = 0;
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        let Ok(tree) = rbx_format::read(&data) else {
            continue;
        };
        for inst in &tree.instances {
            if let Some(src) = inst.get("Source").and_then(|v| v.as_str()) {
                n += 1;
                let name = format!(
                    "{:05}_{}_{}.lua",
                    n,
                    inst.class,
                    f.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("x")
                        .replace(' ', "_")
                );
                let _ = std::fs::write(out.join(name), format!("-- {}\n{}", f.display(), src));
            }
        }
    }
    println!("{n} scripts");
}
