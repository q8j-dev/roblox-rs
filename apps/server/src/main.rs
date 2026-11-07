use clap::Parser;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "server",
    version,
    about = "Headless game server with an HTTP/JSON admin API"
)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:64989")]
    http: SocketAddr,
    #[arg(long, default_value = "0.0.0.0")]
    game_bind: IpAddr,
    #[arg(long, env = "RBX_SERVER_TOKEN")]
    token: Option<String>,
    #[arg(long)]
    content: Option<PathBuf>,
    #[arg(long = "assets")]
    asset_dirs: Vec<PathBuf>,
    #[arg(long = "places")]
    place_dirs: Vec<PathBuf>,
    #[arg(long, default_value = "info")]
    log: String,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    rbx_base::log::init(&args.log);
    let config = server::Config {
        http: args.http,
        game_bind: args.game_bind,
        token: args.token,
        content_dir: args.content,
        asset_dirs: args.asset_dirs,
        place_dirs: args.place_dirs,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(server::serve(config, async {
        let _ = tokio::signal::ctrl_c().await;
    }))
}
