use rbx_dom::DataModel;
use rbx_render::Offscreen;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render_place <place> <out.png> [width] [height]");
        std::process::exit(2);
    }
    let width: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280);
    let height: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(720);
    let data = std::fs::read(&args[1]).expect("read place");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    off.save_png(&dm, std::path::Path::new(&args[2]))
        .expect("render");
    println!("wrote {}", args[2]);
}
