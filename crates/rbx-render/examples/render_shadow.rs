use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec2, Vec3};
use rbx_render::Offscreen;
use std::path::PathBuf;

fn part(
    dm: &mut DataModel,
    parent: InstanceId,
    name: &str,
    size: Vec3,
    pos: Vec3,
    anchored: bool,
) -> InstanceId {
    let p = dm.create("Part").expect("part");
    dm.set_name(p, name);
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(CFrame::from_translation(pos)))
        .expect("cframe");
    dm.set(p, "Anchored", Variant::Bool(anchored))
        .expect("anchored");
    dm.set_parent(p, Some(parent)).expect("parent");
    p
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "shadow.png".into());
    let time = args.get(2).cloned().unwrap_or_else(|| "16:30:00".into());
    let (width, height) = (1280u32, 720u32);
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    let lighting = dm.get_service("Lighting").expect("lighting");
    dm.set(lighting, "TimeOfDay", Variant::String(time))
        .expect("time");
    dm.set(
        lighting,
        "GlobalShadows",
        Variant::Bool(args.get(3).is_some()),
    )
    .expect("shadows");
    part(
        &mut dm,
        ws,
        "Base",
        Vec3::new(80.0, 1.0, 80.0),
        Vec3::new(0.0, -0.5, 0.0),
        true,
    );
    let model = dm.create("Model").expect("model");
    dm.set_parent(model, Some(ws)).expect("parent");
    let h = dm.create("Humanoid").expect("humanoid");
    dm.set_parent(h, Some(model)).expect("parent");
    part(
        &mut dm,
        model,
        "Torso",
        Vec3::new(2.0, 2.0, 1.0),
        Vec3::new(0.0, 3.0, 0.0),
        true,
    );
    part(
        &mut dm,
        model,
        "Head",
        Vec3::new(2.0, 1.0, 1.0),
        Vec3::new(0.0, 4.5, 0.0),
        true,
    );
    part(
        &mut dm,
        model,
        "Left Leg",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(-0.5, 1.0, 0.0),
        true,
    );
    part(
        &mut dm,
        model,
        "Right Leg",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(0.5, 1.0, 0.0),
        true,
    );
    part(
        &mut dm,
        model,
        "Left Arm",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(-1.5, 3.0, 0.0),
        true,
    );
    part(
        &mut dm,
        model,
        "Right Arm",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(1.5, 3.0, 0.0),
        true,
    );
    part(
        &mut dm,
        ws,
        "Wall",
        Vec3::new(1.0, 8.0, 10.0),
        Vec3::new(10.0, 4.0, 0.0),
        true,
    );
    let cam = dm.create("Camera").expect("camera");
    dm.set(
        cam,
        "CFrame",
        Variant::CFrame(CFrame::look_at(
            Vec3::new(14.0, 14.0, 22.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::Y,
        )),
    )
    .expect("cf");
    dm.set(
        cam,
        "Focus",
        Variant::CFrame(CFrame::from_translation(Vec3::new(0.0, 2.0, 0.0))),
    )
    .expect("focus");
    dm.set_parent(cam, Some(ws)).expect("parent");
    dm.set(ws, "CurrentCamera", Variant::Ref(Some(cam)))
        .expect("current");
    dm.gui_view.screen = Vec2::new(width as f32, height as f32);
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    off.save_png(&dm, std::path::Path::new(&out))
        .expect("render");
    println!("wrote {out}");
}
