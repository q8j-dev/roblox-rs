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
    let (mut legacy, mut smooth, mut gui, mut total) = (0, 0, 0, 0);
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        let mut dm = DataModel::new();
        if dm.load_place(&data).is_err() {
            continue;
        }
        total += 1;
        if let Some(t) = dm.workspace_terrain().and_then(|t| dm.terrain(t)) {
            if t.legacy
                .as_ref()
                .is_some_and(|g| g.non_empty_cell_count() > 0)
            {
                legacy += 1;
                if std::env::var("LIST").is_ok() {
                    println!(
                        "legacy {} {}",
                        t.legacy.as_ref().map_or(0, |g| g.non_empty_cell_count()),
                        f.display()
                    );
                }
            }
            if t.smooth
                .as_ref()
                .is_some_and(|g| !g.non_empty_regions().is_empty())
            {
                smooth += 1;
            }
        }
        if let Some(sg) = dm.find_service("StarterGui")
            && !dm.descendants(sg).is_empty()
        {
            gui += 1;
            if std::env::var("LIST_GUI").is_ok() {
                println!("gui {} {}", dm.descendants(sg).len(), f.display());
            }
        }
    }
    println!("places={total} legacy_terrain={legacy} smooth_terrain={smooth} starter_gui={gui}");
}
