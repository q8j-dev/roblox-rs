use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId};
use rbx_script::{Engine, ScriptRuntime};
use std::sync::Arc;

fn dump(dm: &DataModel, id: InstanceId, depth: usize, max: usize) {
    if depth > max {
        return;
    }
    println!(
        "{}{} ({})",
        "  ".repeat(depth),
        dm.name_of(id),
        dm.class_of(id).unwrap_or("?")
    );
    for &c in dm.children_of(id) {
        dump(dm, c, depth + 1, max);
    }
}

fn main() {
    let depth: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let frames: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let mut dm = DataModel::new();
    for s in [
        "Workspace",
        "Players",
        "Lighting",
        "StarterGui",
        "StarterPack",
        "ReplicatedStorage",
    ] {
        let _ = dm.get_service(s);
    }
    if let Some(ws) = dm.find_service("Workspace")
        && let Ok(cam) = dm.create("Camera")
    {
        let _ = dm.set_parent(cam, Some(ws));
        let _ = dm.set(ws, "CurrentCamera", rbx_dom::Variant::Ref(Some(cam)));
    }
    let mut engine = Engine::new(dm);
    engine.content_root = Some("assets/content".into());
    engine.core_package = Some("assets/inexperience/InExperience.rbxm".into());
    engine.running = true;
    let rt = ScriptRuntime::new(Arc::new(Mutex::new(engine))).expect("runtime");
    let players = rt.engine.lock().dm.get_service("Players").expect("players");
    let p =
        rbx_script::api::players::make_local_player(&rt.lua, players, 1, false, Some("Player1"))
            .expect("player");
    rbx_script::api::players::load_character(&rt.lua, p, true, "").expect("character");
    rt.start_server_core_scripts().expect("server core scripts");
    rt.start_core_scripts("StarterScript")
        .expect("core scripts");
    let keys: Vec<(usize, u32)> = std::env::var("KEYS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| {
            let (k, f) = s.split_once('@')?;
            Some((f.parse().ok()?, k.parse().ok()?))
        })
        .collect();
    for frame in 0..frames {
        for &(_, code) in keys.iter().filter(|(f, _)| *f == frame) {
            use rbx_script::api::user_input::{InputEvent, InputKind, KEYBOARD};
            for kind in [InputKind::Began, InputKind::Ended] {
                rt.input(InputEvent {
                    kind,
                    input_type: KEYBOARD,
                    key_code: code,
                    position: rbx_math::Vec3::ZERO,
                    delta: rbx_math::Vec3::ZERO,
                    text: None,
                });
            }
        }
        rt.step(1.0 / 60.0);
    }
    if let Ok(src) = std::env::var("EXEC") {
        let r = rt.execute(
            &src,
            "exec",
            rbx_script::security::Identity::RobloxGameScript,
        );
        println!("EXEC {r:?}");
        rt.step(1.0 / 60.0);
    }
    if let Ok(path) = std::env::var("FIND") {
        let e = rt.engine.lock();
        let root = e.dm.root();
        for d in e.dm.descendants(root) {
            let full = e.dm.full_name(d);
            if full.ends_with(&path) {
                let props: Vec<String> = [
                    "Visible",
                    "Enabled",
                    "BackgroundTransparency",
                    "BackgroundColor3",
                    "Size",
                    "TextSize",
                    "Text",
                    "TextTransparency",
                    "GroupTransparency",
                ]
                .iter()
                .filter_map(|p| e.dm.get(d, p).ok().map(|v| format!("{p}={v:?}")))
                .collect();
                println!("FOUND {full} {}", props.join(" "));
            }
        }
    }
    for w in rt.describe_waiting() {
        println!("WAIT {w}");
    }
    let e = rt.engine.lock();
    for m in &e.output {
        println!("[{:?}] {}", m.kind, m.text);
    }
    println!("inset {:?}", e.dm.gui_view.inset);
    if std::env::var("GUI_ROOTS").is_ok() {
        let root = e.dm.root();
        for d in e.dm.descendants(root) {
            if e.dm.class_of(d) == Some("ScreenGui") {
                println!(
                    "ROOT {} Enabled={:?} DisplayOrder={:?}",
                    e.dm.full_name(d),
                    e.dm.get(d, "Enabled"),
                    e.dm.get(d, "DisplayOrder")
                );
            }
        }
    }
    if std::env::var("GUI_ITEMS").is_ok() {
        for item in rbx_render::gui::extract(&e.dm) {
            let s = format!("{:?}", item.cmd);
            println!("ITEM clip={:?} {}", item.clip, &s[..s.len().min(220)]);
        }
    }
    if let Some(core) = e.dm.find_service("CoreGui") {
        dump(&e.dm, core, 0, depth);
    }
}
