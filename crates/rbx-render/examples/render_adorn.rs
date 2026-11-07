use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Color3, Vec2, Vec3};
use rbx_render::Offscreen;
use std::path::PathBuf;

fn part(dm: &mut DataModel, parent: InstanceId, size: Vec3, pos: Vec3) -> InstanceId {
    let p = dm.create("Part").expect("part");
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(CFrame::from_translation(pos)))
        .expect("cframe");
    dm.set(p, "Anchored", Variant::Bool(true))
        .expect("anchored");
    dm.set_parent(p, Some(parent)).expect("parent");
    p
}

fn adorn(dm: &mut DataModel, parent: InstanceId, class: &str, target: InstanceId) -> InstanceId {
    let a = dm.create(class).expect("adornment");
    dm.set(a, "Adornee", Variant::Ref(Some(target)))
        .expect("adornee");
    dm.set_parent(a, Some(parent)).expect("parent");
    a
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "adorn.png".into());
    let (width, height) = (1280u32, 720u32);
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    part(
        &mut dm,
        ws,
        Vec3::new(80.0, 1.0, 80.0),
        Vec3::new(0.0, -0.5, 0.0),
    );
    let a = part(
        &mut dm,
        ws,
        Vec3::new(4.0, 4.0, 4.0),
        Vec3::new(-10.0, 2.0, 0.0),
    );
    let b = part(
        &mut dm,
        ws,
        Vec3::new(4.0, 4.0, 4.0),
        Vec3::new(-3.0, 2.0, 0.0),
    );
    let c = part(
        &mut dm,
        ws,
        Vec3::new(4.0, 2.0, 4.0),
        Vec3::new(4.0, 1.0, 0.0),
    );
    let d = part(
        &mut dm,
        ws,
        Vec3::new(3.0, 3.0, 3.0),
        Vec3::new(11.0, 1.5, 0.0),
    );
    let e = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(-6.0, 1.0, 9.0),
    );
    let f = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(2.0, 1.0, 9.0),
    );
    adorn(&mut dm, ws, "SelectionBox", a);
    let sphere = adorn(&mut dm, ws, "SelectionSphere", b);
    dm.set(sphere, "SurfaceTransparency", Variant::Float(0.6))
        .expect("surface");
    adorn(&mut dm, ws, "Handles", c);
    adorn(&mut dm, ws, "ArcHandles", d);
    let surface = adorn(&mut dm, ws, "SurfaceSelection", e);
    dm.set(surface, "TargetSurface", Variant::Enum(1))
        .expect("surface");
    let boxed = adorn(&mut dm, ws, "BoxHandleAdornment", f);
    dm.set(boxed, "Size", Variant::Vector3(Vec3::new(1.0, 1.0, 1.0)))
        .expect("size");
    dm.set(
        boxed,
        "SizeRelativeOffset",
        Variant::Vector3(Vec3::new(0.0, 2.0, 0.0)),
    )
    .expect("offset");
    dm.set(boxed, "Color3", Variant::Color3(Color3::new(1.0, 0.6, 0.0)))
        .expect("color");
    let g = part(
        &mut dm,
        ws,
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(10.0, 0.5, 9.0),
    );
    let line = adorn(&mut dm, ws, "LineHandleAdornment", g);
    dm.set(line, "Thickness", Variant::Float(4.0))
        .expect("thickness");
    dm.set(line, "Color3", Variant::Color3(Color3::new(1.0, 0.0, 1.0)))
        .expect("color");
    let moved = adorn(&mut dm, ws, "Handles", f);
    dm.set(moved, "Style", Variant::Enum(1)).expect("style");
    let cam = dm.create("Camera").expect("camera");
    dm.set(
        cam,
        "CFrame",
        Variant::CFrame(CFrame::look_at(
            Vec3::new(0.0, 16.0, 28.0),
            Vec3::new(0.0, 1.0, 3.0),
            Vec3::Y,
        )),
    )
    .expect("cf");
    dm.set_parent(cam, Some(ws)).expect("parent");
    dm.set(ws, "CurrentCamera", Variant::Ref(Some(cam)))
        .expect("current");
    dm.gui_view.screen = Vec2::new(width as f32, height as f32);
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    let adorns = rbx_render::scene::extract(&dm, 1.0).adorns;
    for m in adorns
        .meshes
        .iter()
        .filter(|m| m.material == rbx_render::adorn::Material::AaLine)
    {
        println!("line {:?} {:?} {:?}", m.rows(), m.color, m.thickness);
    }
    println!("adorns {}", adorns.meshes.len());
    off.save_png(&dm, std::path::Path::new(&out))
        .expect("render");
    println!("wrote {out}");
}
