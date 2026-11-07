use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{Color3, Rect2D, UDim2, Vec2};
use rbx_render::Offscreen;
use std::path::PathBuf;

fn add(
    dm: &mut DataModel,
    class: &str,
    parent: InstanceId,
    props: &[(&str, Variant)],
) -> InstanceId {
    let id = dm.create(class).expect("create");
    for (k, v) in props {
        dm.set(id, k, v.clone()).expect(k);
    }
    dm.set_parent(id, Some(parent)).expect("parent");
    id
}

fn udim2(xs: f32, xo: i32, ys: f32, yo: i32) -> Variant {
    Variant::UDim2(UDim2::new(xs, xo, ys, yo))
}

fn color(r: f32, g: f32, b: f32) -> Variant {
    Variant::Color3(Color3::new(r, g, b))
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "gui.png".into());
    let (width, height) = (1280u32, 720u32);
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    let base = add(
        &mut dm,
        "Part",
        ws,
        &[
            ("Anchored", Variant::Bool(true)),
            (
                "Size",
                Variant::Vector3(rbx_math::Vec3::new(64.0, 1.0, 64.0)),
            ),
        ],
    );
    let _ = base;
    dm.gui_view.screen = Vec2::new(width as f32, height as f32);
    dm.gui_view.inset = [0.0, 36.0, 0.0, 0.0];
    let sg = dm.get_service("StarterGui").expect("starter gui");
    let screen = add(&mut dm, "ScreenGui", sg, &[]);
    add(
        &mut dm,
        "Frame",
        screen,
        &[
            ("Size", udim2(1.0, 0, 0.0, 36)),
            ("Position", udim2(0.0, 0, 0.0, -36)),
            ("BackgroundColor3", color(0.12, 0.12, 0.12)),
            ("BackgroundTransparency", Variant::Float(0.5)),
            ("BorderSizePixel", Variant::Int(0)),
        ],
    );
    let panel = add(
        &mut dm,
        "Frame",
        screen,
        &[
            ("Size", udim2(0.0, 360, 0.0, 300)),
            ("Position", udim2(0.0, 20, 0.0, 20)),
            ("BackgroundColor3", color(0.2, 0.3, 0.5)),
            ("BorderColor3", color(1.0, 1.0, 1.0)),
            ("BorderSizePixel", Variant::Int(2)),
            ("ClipsDescendants", Variant::Bool(true)),
        ],
    );
    add(
        &mut dm,
        "TextLabel",
        panel,
        &[
            ("Size", udim2(1.0, 0, 0.0, 40)),
            ("BackgroundTransparency", Variant::Float(1.0)),
            ("Text", Variant::String("Players".into())),
            ("Font", Variant::Enum(4)),
            ("FontSize", Variant::Enum(7)),
            ("TextColor3", color(1.0, 1.0, 1.0)),
        ],
    );
    add(&mut dm, "TextLabel", panel, &[
        ("Size", udim2(1.0, -20, 0.0, 120)),
        ("Position", udim2(0.0, 10, 0.0, 50)),
        ("BackgroundColor3", color(0.95, 0.95, 0.95)),
        ("Text", Variant::String("The quick brown fox jumps over the lazy dog. Wrapped text keeps whole words together on each line.".into())),
        ("Font", Variant::Enum(3)),
        ("FontSize", Variant::Enum(6)),
        ("TextWrapped", Variant::Bool(true)),
        ("TextXAlignment", Variant::Enum(0)),
        ("TextYAlignment", Variant::Enum(0)),
        ("TextColor3", color(0.1, 0.1, 0.1)),
    ]);
    add(
        &mut dm,
        "TextButton",
        panel,
        &[
            ("Size", udim2(0.0, 150, 0.0, 40)),
            ("Position", udim2(0.0, 10, 0.0, 190)),
            ("Style", Variant::Enum(4)),
            ("Text", Variant::String("Play".into())),
            ("Font", Variant::Enum(4)),
            ("FontSize", Variant::Enum(6)),
            ("TextColor3", color(1.0, 1.0, 1.0)),
        ],
    );
    add(
        &mut dm,
        "TextButton",
        panel,
        &[
            ("Size", udim2(0.0, 150, 0.0, 40)),
            ("Position", udim2(0.0, 180, 0.0, 190)),
            ("Style", Variant::Enum(2)),
            ("Text", Variant::String("Settings".into())),
            ("Font", Variant::Enum(2)),
            ("FontSize", Variant::Enum(5)),
            ("TextColor3", color(1.0, 1.0, 1.0)),
        ],
    );
    add(
        &mut dm,
        "Frame",
        panel,
        &[
            ("Size", udim2(0.0, 200, 0.0, 100)),
            ("Position", udim2(1.0, -100, 1.0, -50)),
            ("BackgroundColor3", color(0.9, 0.3, 0.2)),
        ],
    );
    let round = add(
        &mut dm,
        "Frame",
        screen,
        &[
            ("Size", udim2(0.0, 300, 0.0, 160)),
            ("Position", udim2(1.0, -320, 0.0, 20)),
            ("Style", Variant::Enum(3)),
        ],
    );
    add(
        &mut dm,
        "TextLabel",
        round,
        &[
            ("Size", udim2(1.0, 0, 1.0, 0)),
            ("BackgroundTransparency", Variant::Float(1.0)),
            ("Text", Variant::String("Scaled".into())),
            ("TextScaled", Variant::Bool(true)),
            ("TextWrapped", Variant::Bool(true)),
            ("Font", Variant::Enum(1)),
            ("TextColor3", color(1.0, 0.85, 0.2)),
            ("TextStrokeTransparency", Variant::Float(0.0)),
        ],
    );
    add(
        &mut dm,
        "ImageLabel",
        screen,
        &[
            ("Size", udim2(0.0, 96, 0.0, 96)),
            ("Position", udim2(0.5, -48, 1.0, -116)),
            ("BackgroundTransparency", Variant::Float(1.0)),
            (
                "Image",
                Variant::Content("rbxasset://textures/ui/ErrorIcon.png".into()),
            ),
        ],
    );
    add(
        &mut dm,
        "ImageLabel",
        screen,
        &[
            ("Size", udim2(0.0, 240, 0.0, 60)),
            ("Position", udim2(1.0, -260, 1.0, -80)),
            ("BackgroundTransparency", Variant::Float(1.0)),
            (
                "Image",
                Variant::Content("rbxasset://textures/ui/dialog_blue.png".into()),
            ),
            ("ScaleType", Variant::Enum(1)),
            (
                "SliceCenter",
                Variant::Rect2D(Rect2D::xyxy(7.0, 7.0, 33.0, 33.0)),
            ),
        ],
    );
    add(
        &mut dm,
        "Frame",
        screen,
        &[
            ("Size", udim2(0.0, 120, 0.0, 120)),
            ("Position", udim2(0.0, 500, 0.0, 300)),
            ("Rotation", Variant::Float(20.0)),
            ("BackgroundColor3", color(0.3, 0.8, 0.4)),
        ],
    );
    let hint = add(&mut dm, "Hint", ws, &[]);
    dm.set(hint, "Text", Variant::String("This is a Hint".into()))
        .expect("hint");
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    off.save_png(&dm, std::path::Path::new(&out))
        .expect("render");
    println!("wrote {out}");
}
