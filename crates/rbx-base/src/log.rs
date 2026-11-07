use std::path::Path;
use std::sync::Once;

static INIT: Once = Once::new();

fn with_quiet_crates(filter: &str) -> String {
    format!("{filter},symphonia_core=error,symphonia_bundle_mp3=error,symphonia_format_ogg=error")
}

pub fn init(default_filter: &str) {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(with_quiet_crates(default_filter))
        });
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .try_init();
    });
}

pub fn init_with_file(default_filter: &str, path: &Path) {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(with_quiet_crates(default_filter))
        });
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::File::create(path) {
            Ok(file) => {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_ansi(false)
                    .with_writer(std::sync::Mutex::new(file))
                    .try_init();
            }
            Err(_) => {
                let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
            }
        }
    });
}
