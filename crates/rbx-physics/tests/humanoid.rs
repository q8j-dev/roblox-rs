use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec3};
use rbx_physics::PhysicsWorld;
use rbx_physics::humanoid::{HState, HumanoidEvent};

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
    dm.set(p, "formFactorRaw", Variant::Enum(3))
        .expect("form factor");
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(CFrame::from_translation(pos)))
        .expect("cframe");
    dm.set(p, "Anchored", Variant::Bool(anchored))
        .expect("anchored");
    dm.set(p, "TopSurface", Variant::Enum(0)).expect("top");
    dm.set(p, "BottomSurface", Variant::Enum(0))
        .expect("bottom");
    dm.set_parent(p, Some(parent)).expect("parent");
    p
}

fn weld(dm: &mut DataModel, a: InstanceId, b: InstanceId, class: &str) {
    let ca = dm
        .get(a, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf");
    let cb = dm
        .get(b, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf");
    let w = dm.create(class).expect("joint");
    dm.set(w, "Part0", Variant::Ref(Some(a))).expect("p0");
    dm.set(w, "Part1", Variant::Ref(Some(b))).expect("p1");
    dm.set(w, "C0", Variant::CFrame(ca.inverse() * cb))
        .expect("c0");
    dm.set_parent(w, Some(a)).expect("parent");
}

fn character(
    dm: &mut DataModel,
    ws: InstanceId,
    base_y: f32,
) -> (InstanceId, InstanceId, InstanceId) {
    let model = dm.create("Model").expect("model");
    dm.set_name(model, "Dummy");
    dm.set_parent(model, Some(ws)).expect("parent");
    let y = base_y;
    let root = part(
        dm,
        model,
        "HumanoidRootPart",
        Vec3::new(2.0, 2.0, 1.0),
        Vec3::new(0.0, y + 3.0, 0.0),
        false,
    );
    let torso = part(
        dm,
        model,
        "Torso",
        Vec3::new(2.0, 2.0, 1.0),
        Vec3::new(0.0, y + 3.0, 0.0),
        false,
    );
    let head = part(
        dm,
        model,
        "Head",
        Vec3::new(2.0, 1.0, 1.0),
        Vec3::new(0.0, y + 4.5, 0.0),
        false,
    );
    let ll = part(
        dm,
        model,
        "Left Leg",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(-0.5, y + 1.0, 0.0),
        false,
    );
    let rl = part(
        dm,
        model,
        "Right Leg",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(0.5, y + 1.0, 0.0),
        false,
    );
    let la = part(
        dm,
        model,
        "Left Arm",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(-1.5, y + 3.0, 0.0),
        false,
    );
    let ra = part(
        dm,
        model,
        "Right Arm",
        Vec3::new(1.0, 2.0, 1.0),
        Vec3::new(1.5, y + 3.0, 0.0),
        false,
    );
    weld(dm, root, torso, "Motor6D");
    weld(dm, torso, head, "Weld");
    for limb in [ll, rl, la, ra] {
        weld(dm, torso, limb, "Motor6D");
    }
    let h = dm.create("Humanoid").expect("humanoid");
    dm.set_parent(h, Some(model)).expect("parent");
    (model, h, torso)
}

#[test]
fn humanoid_stands_and_walks() {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("ws");
    part(
        &mut dm,
        ws,
        "Base",
        Vec3::new(200.0, 2.0, 200.0),
        Vec3::new(0.0, -1.0, 0.0),
        true,
    );
    let (_, h, torso) = character(&mut dm, ws, 2.0);
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    let mut events = Vec::new();
    for _ in 0..180 {
        events.extend(world.step(&mut dm, 1.0 / 60.0).humanoid);
    }
    let y = dm
        .get(torso, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf")
        .translation
        .y;
    assert!((y - 3.0).abs() < 0.6, "torso height {y}");
    let state = world.humanoid(h).expect("sim").state;
    assert!(
        matches!(state, HState::Running | HState::RunningNoPhysics),
        "state {state:?}"
    );
    dm.set(
        h,
        "WalkToPoint",
        Variant::Vector3(Vec3::new(20.0, 0.0, 0.0)),
    )
    .expect("walk");
    for _ in 0..240 {
        events.extend(world.step(&mut dm, 1.0 / 60.0).humanoid);
    }
    let p = dm
        .get(torso, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf")
        .translation;
    assert!(p.x > 15.0, "walked to {p:?}");
    assert!(
        events
            .iter()
            .any(|(_, e)| matches!(e, HumanoidEvent::MoveToFinished(true))),
        "move finished"
    );
}

#[test]
fn humanoid_dies_without_health() {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("ws");
    part(
        &mut dm,
        ws,
        "Base",
        Vec3::new(200.0, 2.0, 200.0),
        Vec3::new(0.0, -1.0, 0.0),
        true,
    );
    let (model, h, _) = character(&mut dm, ws, 2.0);
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    for _ in 0..30 {
        world.step(&mut dm, 1.0 / 60.0);
    }
    dm.set(h, "Health", Variant::Float(0.0)).expect("health");
    let mut died = false;
    for _ in 0..30 {
        died |= world
            .step(&mut dm, 1.0 / 60.0)
            .humanoid
            .iter()
            .any(|(_, e)| matches!(e, HumanoidEvent::Died));
    }
    assert!(died);
    assert_eq!(world.humanoid(h).expect("sim").state, HState::Dead);
    let joints = dm
        .descendants(model)
        .into_iter()
        .filter(|d| dm.is_a(*d, "JointInstance"))
        .count();
    assert_eq!(joints, 0, "joints broken on death");
}

#[test]
fn resting_humanoid_stays_kinematic_across_assembly_rebuilds() {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("ws");
    part(
        &mut dm,
        ws,
        "Base",
        Vec3::new(200.0, 2.0, 200.0),
        Vec3::new(0.0, -1.0, 0.0),
        true,
    );
    let (_, h, torso) = character(&mut dm, ws, 2.0);
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    for _ in 0..240 {
        world.step(&mut dm, 1.0 / 60.0);
    }
    assert_eq!(
        world.humanoid(h).expect("sim").state,
        HState::RunningNoPhysics
    );
    assert!(world.assembly_of(torso).expect("assembly").kinematic);
    let a = part(
        &mut dm,
        ws,
        "A",
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(50.0, 5.0, 0.0),
        false,
    );
    let b = part(
        &mut dm,
        ws,
        "B",
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(51.0, 5.0, 0.0),
        false,
    );
    weld(&mut dm, a, b, "Weld");
    world.step(&mut dm, 1.0 / 240.0);
    assert_eq!(
        world.humanoid(h).expect("sim").state,
        HState::RunningNoPhysics
    );
    assert!(world.assembly_of(torso).expect("assembly").kinematic);
}

#[test]
fn humanoid_stands_on_smooth_terrain() {
    use rbx_dom::rbx_terrain::SmoothGrid;
    use rbx_dom::rbx_terrain::smooth::{Cell, CellBox, Region};
    use rbx_math::IVec3;
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("ws");
    let t = dm.create("Terrain").expect("terrain");
    dm.set_parent(t, Some(ws)).expect("parent");
    let mut g = SmoothGrid::new();
    let region = Region::new(IVec3::new(-8, -4, -8), IVec3::new(8, 0, 8));
    let mut b = CellBox::sized(region.size());
    let s = region.size();
    for y in 0..s.y {
        for z in 0..s.z {
            for x in 0..s.x {
                b.set(x, y, z, Cell::new(2, 255));
            }
        }
    }
    g.write(&region, &b);
    let data = dm.terrain_mut(t);
    data.legacy = None;
    data.smooth = Some(g);
    let (_, h, torso) = character(&mut dm, ws, 2.0);
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    for _ in 0..240 {
        world.step(&mut dm, 1.0 / 60.0);
    }
    let y = dm
        .get(torso, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf")
        .translation
        .y;
    let state = world.humanoid(h).expect("sim").state;
    assert!(
        matches!(state, HState::Running | HState::RunningNoPhysics),
        "state {state:?} at torso height {y}"
    );
    assert!((y - 5.2).abs() < 0.4, "torso height {y}");
}
