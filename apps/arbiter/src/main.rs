use arbiter::{Arbiter, ProcessLauncher, Settings};
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "arbiter",
    version,
    about = "Starts, monitors and restarts game server processes"
)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:64989")]
    http: SocketAddr,
    #[arg(long)]
    server: Option<PathBuf>,
    #[arg(long, default_value_t = 64990)]
    base_port: u16,
    #[arg(long, default_value_t = 64)]
    max_processes: usize,
    #[arg(long, default_value_t = 1)]
    pool: usize,
    #[arg(long)]
    single_process: bool,
    #[arg(long, default_value = "info")]
    log: String,
    #[arg(last = true)]
    server_args: Vec<String>,
}

fn default_server() -> PathBuf {
    let name = if cfg!(windows) {
        "server.exe"
    } else {
        "server"
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    rbx_base::log::init(&args.log);
    let settings = Settings {
        multi_process: !args.single_process,
        recycle_queue_size: args.pool,
        base_port: args.base_port,
        max_processes: args.max_processes,
        ..Settings::default()
    };
    let launcher = ProcessLauncher {
        binary: args.server.unwrap_or_else(default_server),
        args: args.server_args,
    };
    let arbiter = Arc::new(Arbiter::new(Box::new(launcher), settings));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(args.http).await?;
        tracing::info!("arbiter listening on http://{}", listener.local_addr()?);
        arbiter::run(arbiter, listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    })
}
