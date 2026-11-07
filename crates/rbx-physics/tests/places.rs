use rbx_dom::DataModel;
use rbx_physics::PhysicsWorld;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        if name.to_string_lossy().starts_with('.') {
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
    for dir in ["tests", "assets"] {
        collect(&root.join(dir), &mut files);
    }
    let excluded: Vec<String> =
        std::fs::read_to_string(root.join("tests").join("excluded_places.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    files.retain(|f| {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !excluded.iter().any(|e| e == name)
    });
    files.sort();
    files
}

fn simulate(path: &Path) -> Result<usize, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut dm = DataModel::new();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext.starts_with("rbxm") {
        let ws = dm.get_service("Workspace").map_err(|e| e.to_string())?;
        let (roots, _) = dm.load_model(&data).map_err(|e| e.to_string())?;
        for r in roots {
            let _ = dm.set_parent(r, Some(ws));
        }
    } else {
        dm.load_place(&data).map_err(|e| e.to_string())?;
    }
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    world.rebuild_assemblies();
    for _ in 0..600 {
        world.world_step(&dm);
    }
    world.write_back(&mut dm);
    let _ = world.take_events();
    for p in &world.parts {
        if !p.cframe.is_finite() {
            return Err(format!("non-finite CFrame on {:?}", p.id));
        }
    }
    Ok(world.parts.len())
}

#[test]
fn every_place_simulates_600_steps() {
    let files = place_files();
    assert!(files.len() > 500, "found only {} place files", files.len());
    let failures = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(f) = files.get(i) else {
                        break;
                    };
                    let f2 = f.clone();
                    let started = std::time::Instant::now();
                    let r = std::thread::Builder::new()
                        .stack_size(32 << 20)
                        .spawn(move || simulate(&f2))
                        .map_err(|e| e.to_string())
                        .and_then(|h| h.join().map_err(|_| "panicked".to_string()))
                        .and_then(|r| r);
                    let secs = started.elapsed().as_secs_f64();
                    if secs > 5.0 {
                        eprintln!(
                            "slow {:.1}s {} parts={:?}",
                            secs,
                            f.display(),
                            r.as_ref().ok()
                        );
                    }
                    if let Err(e) = r
                        && let Ok(mut v) = failures.lock()
                    {
                        v.push(format!("{}: {e}", f.display()));
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap_or_default();
    assert!(
        failures.is_empty(),
        "{} places failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
