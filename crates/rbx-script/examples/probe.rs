use rbx_dom::{DataModel, Variant};
use rbx_script::harness::run_datamodel;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: probe <place file> <lua file>");
        std::process::exit(2);
    }
    let path = PathBuf::from(&args[1]);
    let source = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {e}", args[2]);
            std::process::exit(1);
        }
    };
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let mut dm = DataModel::new();
    let warnings = match dm.load_place(&data) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    dm.ensure_terrain();
    let Some(ws) = dm.find_service("Workspace") else {
        eprintln!("no workspace");
        std::process::exit(1);
    };
    let Ok(script) = dm.create("Script") else {
        eprintln!("cannot create script");
        std::process::exit(1);
    };
    dm.set_name(script, "Probe");
    let _ = dm.set(script, "Source", Variant::ProtectedString(source));
    let _ = dm.set_parent(script, Some(ws));
    let content = PathBuf::from("assets/content");
    match run_datamodel(dm, &path, Some(&content), warnings) {
        Ok(run) => {
            for line in &run.output {
                println!("{line}");
            }
            println!(
                "completed={} tests={} errors={} game_time={:.2}",
                run.completed, run.tests, run.errors, run.game_time
            );
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
