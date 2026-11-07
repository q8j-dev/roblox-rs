use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec3};
use rbx_physics::{PhysicsWorld, StepEvents};

fn setup() -> (DataModel, InstanceId) {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    (dm, ws)
}

fn part(
    dm: &mut DataModel,
    parent: InstanceId,
    size: Vec3,
    pos: Vec3,
    anchored: bool,
) -> InstanceId {
    let p = dm.create("Part").expect("part");
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

fn pos(dm: &DataModel, id: InstanceId) -> Vec3 {
    dm.get(id, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf")
        .translation
}

fn run(world: &mut PhysicsWorld, dm: &mut DataModel, seconds: f64) -> StepEvents {
    let mut all = StepEvents::default();
    let frames = (seconds * 60.0).round() as usize;
    for _ in 0..frames {
        let e = world.step(dm, 1.0 / 60.0);
        all.touched.extend(e.touched);
        all.touch_ended.extend(e.touch_ended);
        all.fallen.extend(e.fallen);
    }
    all
}

#[test]
fn free_fall_matches_gravity() {
    let (mut dm, ws) = setup();
    let p = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(0.0, 500.0, 0.0),
        false,
    );
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    run(&mut world, &mut dm, 1.0);
    let y = pos(&dm, p).y;
    let expected = 500.0 - 0.5 * 196.2;
    assert!(
        (y - expected).abs() < 2.0,
        "y = {y}, expected about {expected}"
    );
    let v = dm
        .get(p, "Velocity")
        .ok()
        .and_then(|v| v.as_vec3())
        .expect("vel");
    assert!((v.y + 196.2).abs() < 2.0, "vy = {}", v.y);
}

#[test]
fn block_rests_on_baseplate_and_sleeps() {
    let (mut dm, ws) = setup();
    let base = part(
        &mut dm,
        ws,
        Vec3::new(64.0, 4.0, 64.0),
        Vec3::new(0.0, -2.0, 0.0),
        true,
    );
    let p = part(
        &mut dm,
        ws,
        Vec3::new(4.0, 2.0, 4.0),
        Vec3::new(0.0, 4.0, 0.0),
        false,
    );
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    let events = run(&mut world, &mut dm, 3.0);
    let y = pos(&dm, p).y;
    assert!((y - 1.0).abs() < 0.15, "rest height {y}");
    assert!(
        events
            .touched
            .iter()
            .any(|&(a, b)| (a == p && b == base) || (a == base && b == p)),
        "touch fired"
    );
    assert_eq!(world.num_awake_parts(), 0, "block should be asleep");
}

#[test]
fn stack_of_boxes_stays_up() {
    let (mut dm, ws) = setup();
    part(
        &mut dm,
        ws,
        Vec3::new(64.0, 4.0, 64.0),
        Vec3::new(0.0, -2.0, 0.0),
        true,
    );
    let mut boxes = Vec::new();
    for i in 0..5 {
        boxes.push(part(
            &mut dm,
            ws,
            Vec3::new(2.0, 2.0, 2.0),
            Vec3::new(0.0, 1.0 + 2.0 * i as f32, 0.0),
            false,
        ));
    }
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    run(&mut world, &mut dm, 4.0);
    let top = pos(&dm, boxes[4]);
    assert!(top.y > 8.5, "top box fell to {}", top.y);
    assert!(
        top.x.abs() < 0.5 && top.z.abs() < 0.5,
        "top box drifted {top:?}"
    );
}

#[test]
fn welded_parts_move_together() {
    let (mut dm, ws) = setup();
    let a = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(0.0, 100.0, 0.0),
        false,
    );
    let b = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(2.0, 100.0, 0.0),
        false,
    );
    let w = dm.create("Weld").expect("weld");
    dm.set(w, "Part0", Variant::Ref(Some(a))).expect("p0");
    dm.set(w, "Part1", Variant::Ref(Some(b))).expect("p1");
    dm.set(
        w,
        "C0",
        Variant::CFrame(CFrame::from_translation(Vec3::new(2.0, 0.0, 0.0))),
    )
    .expect("c0");
    dm.set_parent(w, Some(a)).expect("parent");
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    run(&mut world, &mut dm, 0.5);
    let pa = pos(&dm, a);
    let pb = pos(&dm, b);
    assert!(
        ((pb - pa) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3,
        "offset {:?}",
        pb - pa
    );
    assert!(pa.y < 90.0, "fell {}", pa.y);
    assert_eq!(world.assembly_mass(a), world.assembly_mass(b));
}

#[test]
fn surface_join_creates_snap() {
    let (mut dm, ws) = setup();
    let base = part(
        &mut dm,
        ws,
        Vec3::new(8.0, 1.2, 8.0),
        Vec3::new(0.0, 0.6, 0.0),
        true,
    );
    dm.set(base, "TopSurface", Variant::Enum(3)).expect("studs");
    let top = part(
        &mut dm,
        ws,
        Vec3::new(4.0, 1.2, 2.0),
        Vec3::new(0.0, 1.8, 0.0),
        false,
    );
    dm.set(top, "BottomSurface", Variant::Enum(4))
        .expect("inlet");
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    let created = world.join(&mut dm, top);
    assert_eq!(created.len(), 1);
    assert_eq!(dm.class_of(created[0]), Some("Snap"));
    run(&mut world, &mut dm, 0.5);
    assert!(world.is_grounded(top));
    assert!((pos(&dm, top).y - 1.8).abs() < 1e-3);
}

#[test]
fn fallen_parts_are_reported() {
    let (mut dm, ws) = setup();
    let p = part(
        &mut dm,
        ws,
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(0.0, -490.0, 0.0),
        false,
    );
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    let e = run(&mut world, &mut dm, 1.0);
    assert!(e.fallen.contains(&p));
}

#[test]
fn hinge_lets_part_swing() {
    let (mut dm, ws) = setup();
    let anchor = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(0.0, 50.0, 0.0),
        true,
    );
    let arm = part(
        &mut dm,
        ws,
        Vec3::new(2.0, 2.0, 6.0),
        Vec3::new(2.0, 50.0, -2.0),
        false,
    );
    let r = dm.create("Rotate").expect("rotate");
    let c0 = CFrame::new(
        rbx_physics::joins::normal_id_to_matrix(rbx_math::NormalId::Right),
        Vec3::new(1.0, 0.0, 0.0),
    );
    let c1 = CFrame::new(
        rbx_physics::joins::normal_id_to_matrix(rbx_math::NormalId::Right),
        Vec3::new(-1.0, 0.0, 2.0),
    );
    dm.set(r, "Part0", Variant::Ref(Some(anchor))).expect("p0");
    dm.set(r, "Part1", Variant::Ref(Some(arm))).expect("p1");
    dm.set(r, "C0", Variant::CFrame(c0)).expect("c0");
    dm.set(r, "C1", Variant::CFrame(c1)).expect("c1");
    let js = dm.get_service("JointsService").expect("js");
    dm.set_parent(r, Some(js)).expect("parent");
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    run(&mut world, &mut dm, 1.0);
    let p = pos(&dm, arm);
    let pivot = Vec3::new(1.0, 50.0, 0.0);
    let d = p - pivot;
    assert!((d.x - 1.0).abs() < 0.1, "stays on axis plane {d:?}");
    assert!(p.y < 49.0, "arm swung down {p:?}");
    assert!(
        (Vec3::new(0.0, d.y, d.z).length() - 2.0).abs() < 0.2,
        "radius kept {d:?}"
    );
}
