use rbx_dom::DataModel;
use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("rbxl" | "rbxlx")
            ) {
                out.push(p);
            }
        }
    }
}

fn main() {
    let mut files = Vec::new();
    walk(Path::new("tests/places"), &mut files);
    let (mut on, mut total, mut lights) = (0, 0, 0);
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        let mut dm = DataModel::new();
        if dm.load_place(&data).is_err() {
            continue;
        }
        total += 1;
        if let Some(l) = dm.find_service("Lighting")
            && dm.get_bool(l, "GlobalShadows")
        {
            on += 1;
            if std::env::var("LIST").is_ok() {
                println!("shadows {}", f.display());
            }
        }
        let root = dm.root();
        if dm.descendants(root).iter().any(|d| dm.is_a(*d, "Light")) {
            lights += 1;
            if std::env::var("LIST").is_ok() {
                println!("lights {}", f.display());
            }
        }
    }
    println!("places={total} global_shadows={on} with_lights={lights}");
}
