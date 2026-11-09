use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_physics::PhysicsWorld;

fn part(dm: &mut DataModel, class: &str, size: Vec3, cf: CFrame, anchored: bool, friction: f32) -> InstanceId {
    let ws = dm.get_service("Workspace").expect("ws");
    let p = dm.create(class).expect("part");
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(cf)).expect("cframe");
    dm.set(p, "Anchored", Variant::Bool(anchored)).expect("anchored");
    dm.set(p, "Friction", Variant::Float(friction)).expect("friction");
    if class == "Part" {
        dm.set(p, "TopSurface", Variant::Enum(0)).expect("top");
        dm.set(p, "BottomSurface", Variant::Enum(0)).expect("bottom");
    }
    dm.set_parent(p, Some(ws)).expect("parent");
    p
}

fn position(dm: &DataModel, id: InstanceId) -> CFrame {
    dm.get(id, "CFrame").ok().and_then(|v| v.as_cframe()).expect("cframe")
}

fn simulate(dm: &mut DataModel, steps: usize) {
    let mut world = PhysicsWorld::new();
    world.attach(dm);
    for _ in 0..steps {
        world.step(dm, 1.0 / 60.0);
    }
}

#[test]
fn sliding_blocks_stop_where_legacy_friction_puts_them() {
    for (fa, fb, mu) in [(2.0f32, 0.3f32, 1.3f32), (1.5, 0.3, 0.8), (0.3, 0.3, 0.3), (1.0, 1.0, 1.0)] {
        let mut dm = DataModel::new();
        part(&mut dm, "Part", Vec3::new(200.0, 1.2, 40.0), CFrame::IDENTITY, true, fb);
        let p = part(&mut dm, "Part", Vec3::new(4.0, 1.0, 4.0), CFrame::from_translation(Vec3::new(-50.0, 1.1, 0.0)), false, fa);
        dm.set(p, "Velocity", Variant::Vector3(Vec3::new(70.0, 0.0, 0.0))).expect("velocity");
        simulate(&mut dm, 300);
        let travel = position(&dm, p).translation.x + 50.0;
        let expected = 70.0 * 70.0 / (2.0 * mu * 196.2);
        assert!((travel - expected).abs() < 0.5, "friction {fa}/{fb}: travelled {travel}, expected {expected}");
    }
}

#[test]
fn block_rests_on_a_gentle_wedge() {
    let mut dm = DataModel::new();
    let rot = Mat3::from_cols(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(-1.0, 0.0, 0.0)).transpose();
    part(&mut dm, "WedgePart", Vec3::new(6.0, 27.0, 7.0), CFrame::new(rot, Vec3::new(-27.5, 4.0, -13.0)), true, 0.6);
    let start = Vec3::new(-38.126, 8.869, -13.5);
    let b = part(&mut dm, "Part", Vec3::new(4.0, 4.0, 4.0), CFrame::new(Mat3::from_rotation_z((-13.231f32).to_radians()), start), false, 0.3);
    simulate(&mut dm, 240);
    let moved = (position(&dm, b).translation - start).length();
    assert!(moved < 1.5, "block slid {moved} studs down a 14.5 degree wedge with friction 0.3");
}
