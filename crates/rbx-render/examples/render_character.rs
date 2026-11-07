use rbx_dom::{DataModel, Variant};
use rbx_math::{CFrame, Vec2, Vec3};
use rbx_render::Offscreen;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "character.png".into());
    let place = args.get(2).cloned();
    let (width, height) = (1280u32, 720u32);
    let mut dm = DataModel::new();
    if let Some(p) = place {
        dm.load_place(&std::fs::read(p).expect("read"))
            .expect("load");
    }
    let ws = dm.get_service("Workspace").expect("workspace");
    if let Some(v) = args.get(4) {
        let l = dm.get_service("Lighting").expect("lighting");
        dm.set(l, "GlobalShadows", Variant::Bool(v == "1"))
            .expect("shadows");
    }
    let data = std::fs::read("assets/content/fonts/character3.rbxm").expect("character");
    let (roots, _) = dm.load_model(&data).expect("model");
    let model = roots[0];
    for d in dm.descendants(model) {
        if dm.is_a(d, "BasePart") {
            let _ = dm.set(d, "Anchored", Variant::Bool(true));
        }
    }
    dm.set_parent(model, Some(ws)).expect("parent");
    let head = dm.find_first_child(model, "Head", false).expect("head");
    let hp = dm
        .get_raw(head, "CFrame")
        .and_then(|v| v.as_cframe())
        .expect("cf")
        .translation;
    let cam = dm.create("Camera").expect("camera");
    let side: f32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let eye = hp + Vec3::new(3.0, 0.5, 7.0 * side);
    dm.set(
        cam,
        "CFrame",
        Variant::CFrame(CFrame::look_at(eye, hp - Vec3::new(0.0, 1.5, 0.0), Vec3::Y)),
    )
    .expect("cf");
    dm.set(cam, "Focus", Variant::CFrame(CFrame::from_translation(hp)))
        .expect("focus");
    dm.set_parent(cam, Some(ws)).expect("parent");
    dm.set(ws, "CurrentCamera", Variant::Ref(Some(cam)))
        .expect("current");
    dm.gui_view.screen = Vec2::new(width as f32, height as f32);
    let mut off = Offscreen::new(
        width,
        height,
        &PathBuf::from("assets/content"),
        &PathBuf::from("assets/platform-content/pc"),
    )
    .expect("gpu");
    off.save_png(&dm, std::path::Path::new(&out))
        .expect("render");
    println!("wrote {out}");
}
