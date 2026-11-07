use rbx_script::harness::{PlaceRun, run_place_file};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rbxl" | "rbxlx")
        ) {
            out.push(path);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(
        args.get(1)
            .cloned()
            .unwrap_or_else(|| "tests/places".into()),
    );
    let filter = args.get(2).cloned().unwrap_or_default();
    let content = PathBuf::from("assets/content");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    files.retain(|f| f.to_string_lossy().contains(&filter));
    let results: Mutex<Vec<(PathBuf, Result<PlaceRun, String>)>> = Mutex::new(Vec::new());
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
                    let content = content.clone();
                    let started = std::time::Instant::now();
                    let r = std::thread::Builder::new()
                        .stack_size(64 << 20)
                        .spawn(move || run_place_file(&f2, Some(&content)))
                        .map_err(|e| e.to_string())
                        .and_then(|h| h.join().map_err(|_| "panicked".to_string()))
                        .and_then(|r| r);
                    eprintln!("{:6.1}s {}", started.elapsed().as_secs_f64(), f.display());
                    if let Ok(mut v) = results.lock() {
                        v.push((f.clone(), r));
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_default();
    results.sort_by(|a, b| a.0.cmp(&b.0));
    let mut pass_tests = 0;
    let mut total_tests = 0;
    let mut pass_empty = 0;
    let mut total_empty = 0;
    for (path, r) in &results {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        match r {
            Ok(run) => {
                if run.has_tests {
                    total_tests += 1;
                } else {
                    total_empty += 1;
                }
                if run.passed() {
                    if run.has_tests {
                        pass_tests += 1;
                    } else {
                        pass_empty += 1;
                    }
                    println!("PASS {name} tests={} warns={}", run.tests, run.warns);
                } else {
                    println!(
                        "FAIL {name} players={} completed={} tests={} errors={} failures={:?}",
                        run.players,
                        run.completed,
                        run.tests,
                        run.errors,
                        run.failures.iter().take(3).collect::<Vec<_>>()
                    );
                    for e in run.script_errors.iter().take(4) {
                        println!("     | {}", e.lines().next().unwrap_or(""));
                    }
                }
            }
            Err(e) => {
                total_tests += 1;
                println!("ERR  {name} {e}");
            }
        }
    }
    println!(
        "test places: {pass_tests}/{total_tests} passed; places without tests: {pass_empty}/{total_empty} ran"
    );
}
