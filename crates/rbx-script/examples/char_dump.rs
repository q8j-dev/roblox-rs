use parking_lot::Mutex;
use rbx_dom::DataModel;
use rbx_script::{Engine, ScriptRuntime};
use std::sync::Arc;

fn main() {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").unwrap();
    let base = dm.create("Part").unwrap();
    dm.set(base, "Anchored", rbx_dom::Variant::Bool(true))
        .unwrap();
    dm.set(
        base,
        "Size",
        rbx_dom::Variant::Vector3(rbx_math::Vec3::new(50.0, 1.2, 50.0)),
    )
    .unwrap();
    dm.set_parent(base, Some(ws)).unwrap();
    let mut engine = Engine::new(dm);
    engine.content_root = Some("assets/content".into());
    engine.running = true;
    let rt = ScriptRuntime::new(Arc::new(Mutex::new(engine))).unwrap();
    let players = rt.engine.lock().dm.get_service("Players").unwrap();
    let p =
        rbx_script::api::players::make_local_player(&rt.lua, players, 1, false, Some("P")).unwrap();
    rbx_script::api::players::load_character(&rt.lua, p, true, "").unwrap();
    for _ in 0..120 {
        rt.step(1.0 / 60.0);
    }
    let e = rt.engine.lock();
    let ch = e.dm.get_ref(p, "Character").unwrap();
    for &c in e.dm.children_of(ch) {
        if let Some(cf) = e.dm.get_raw(c, "CFrame").and_then(|v| v.as_cframe()) {
            println!(
                "{} det={:.3} pos={:?} rot={:?}",
                e.dm.name_of(c),
                cf.rotation.determinant(),
                cf.translation,
                cf.rotation
            );
        }
    }
}
