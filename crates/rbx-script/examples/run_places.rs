use rbx_script::harness::{PlaceRun, run_place_file};
use std::io::Write;
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
    if let Some(list) = filter.strip_prefix('@') {
        let wanted: Vec<String> = std::fs::read_to_string(list)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        files.retain(|f| {
            wanted
                .iter()
                .any(|w| f.to_string_lossy().ends_with(w.as_str()))
        });
    } else {
        files.retain(|f| f.to_string_lossy().contains(&filter));
    }
    let counts: Mutex<[usize; 4]> = Mutex::new([0; 4]);
    let next = AtomicUsize::new(0);
    let threads = std::env::var("RBX_PLACE_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
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
                    let name = f.strip_prefix(&root).unwrap_or(f).display().to_string();
                    let (line, slot) = report(&name, &r);
                    if let Ok(mut c) = counts.lock() {
                        c[slot] += 1;
                        let mut out = std::io::stdout().lock();
                        let _ = writeln!(out, "{line}");
                        let _ = out.flush();
                    }
                }
            });
        }
    });
    let c = counts.into_inner().unwrap_or_default();
    println!(
        "test places: {}/{} passed; places without tests: {}/{} ran",
        c[0],
        c[0] + c[1],
        c[2],
        c[2] + c[3]
    );
}

fn report(name: &str, r: &Result<PlaceRun, String>) -> (String, usize) {
    match r {
        Ok(run) => {
            let slot = match (run.has_tests, run.passed()) {
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (false, false) => 3,
            };
            if run.passed() {
                return (
                    format!("PASS {name} tests={} warns={}", run.tests, run.warns),
                    slot,
                );
            }
            let mut line = format!(
                "FAIL {name}{} players={} completed={} tests={} errors={} failures={:?}",
                if run.timed_out {
                    " (wall-clock limit)"
                } else {
                    ""
                },
                run.players,
                run.completed,
                run.tests,
                run.errors,
                run.failures.iter().take(3).collect::<Vec<_>>()
            );
            for e in run.script_errors.iter().take(4) {
                line.push_str("\n     | ");
                line.push_str(e.lines().next().unwrap_or(""));
            }
            (line, slot)
        }
        Err(e) => (format!("ERR  {name} {e}"), 1),
    }
}
