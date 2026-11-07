use rbx_dom::DataModel;
use rbx_render::Offscreen;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: render_place <place> <out.png> [width] [height] [eye_x,eye_y,eye_z,target_x,target_y,target_z]"
        );
        std::process::exit(2);
    }
    let width: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280);
    let height: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(720);
    let data = std::fs::read(&args[1]).expect("read place");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    dm.gui_view.screen = rbx_math::Vec2::new(width as f32, height as f32);
    if let Some(cam) = args.get(5) {
        let v: Vec<f32> = cam.split(',').filter_map(|s| s.parse().ok()).collect();
        if v.len() == 6
            && let Some(ws) = dm.find_service("Workspace")
            && let Some(c) = dm
                .get_ref(ws, "CurrentCamera")
                .or_else(|| dm.find_first_child_of_class(ws, "Camera"))
        {
            let eye = rbx_math::Vec3::new(v[0], v[1], v[2]);
            let target = rbx_math::Vec3::new(v[3], v[4], v[5]);
            let cf = rbx_math::CFrame::look_at(eye, target, rbx_math::Vec3::Y);
            let _ = dm.set(c, "CFrame", rbx_dom::Variant::CFrame(cf));
        }
    }
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    off.save_png(&dm, std::path::Path::new(&args[2]))
        .expect("render");
    println!("wrote {}", args[2]);
}
