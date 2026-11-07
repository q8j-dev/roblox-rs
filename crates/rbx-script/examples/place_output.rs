use rbx_script::harness::run_place_file;
use std::path::PathBuf;

fn main() {
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: place_output <place file>");
        std::process::exit(2);
    };
    let content = PathBuf::from("assets/content");
    match run_place_file(&path, Some(&content)) {
        Ok(run) => {
            for line in &run.output {
                println!("{line}");
            }
            println!(
                "completed={} tests={} warns={} errors={} game_time={:.2}",
                run.completed, run.tests, run.warns, run.errors, run.game_time
            );
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
