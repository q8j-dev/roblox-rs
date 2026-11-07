use std::collections::BTreeMap;
use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("rbxl" | "rbxlx" | "clienttest")
            ) {
                out.push(p);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    walk(Path::new(&args[1]), &mut files);
    files.sort();
    let mut by_dir: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let Ok(tree) = rbx_format::read(&data) else { continue };
        let ts = tree.instances.iter().position(|i| i.class == "TestService");
        let has_ts_scripts = ts.is_some_and(|t| {
            tree.instances[t]
                .children
                .iter()
                .any(|&c| tree.instances[c].class == "Script")
        });
        let dir = f
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let e = by_dir.entry(dir).or_default();
        e.0 += 1;
        if ts.is_some() {
            e.1 += 1;
        }
        if has_ts_scripts {
            e.2 += 1;
        }
    }
    let mut total = (0, 0, 0);
    for (d, (n, t, s)) in &by_dir {
        println!("{n:4} {t:4} {s:4}  {d}");
        total.0 += n;
        total.1 += t;
        total.2 += s;
    }
    println!("{:4} {:4} {:4}  total (places, with TestService, with test scripts)", total.0, total.1, total.2);
}
