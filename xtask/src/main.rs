mod audit;
mod ci;
mod serve;
mod wasi;

use std::path::PathBuf;
use std::process::ExitCode;

pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(PathBuf::from).unwrap_or(manifest)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    let result = match command {
        "audit" => audit::run(&repo_root()),
        "ci" => ci::run(&repo_root(), rest),
        "web" => ci::build_web(&repo_root(), rest.iter().any(|a| a == "--release")),
        "serve" => serve::run(&repo_root(), rest),
        _ => {
            eprintln!("usage: cargo xtask <audit|ci|web|serve> [options]");
            eprintln!("  ci options: --skip-docker --skip-cross --skip-web --quick");
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}
