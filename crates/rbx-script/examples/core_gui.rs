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
    let mut engine = Engine::new(dm);
    engine.content_root = Some("assets/content".into());
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
    for _ in 0..180 {
        rt.step(1.0 / 60.0);
    }
    for w in rt.describe_waiting() {
        println!("WAIT {w}");
    }
    let e = rt.engine.lock();
    for m in &e.output {
        println!("[{:?}] {}", m.kind, m.text);
    }
    println!("inset {:?}", e.dm.gui_view.inset);
    if let Some(core) = e.dm.find_service("CoreGui") {
        dump(&e.dm, core, 0, depth);
    }
}
