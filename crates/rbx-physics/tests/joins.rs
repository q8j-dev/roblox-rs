use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_physics::PhysicsWorld;

const WELD: u32 = 2;

fn part(dm: &mut DataModel, class: &str, size: Vec3, cf: CFrame, weld_face: &str) -> InstanceId {
    let ws = dm.get_service("Workspace").expect("ws");
    let p = dm.create(class).expect("part");
    dm.set(p, "formFactorRaw", Variant::Enum(3))
        .expect("form factor");
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(cf)).expect("cframe");
    for face in ["Top", "Bottom", "Left", "Right", "Front", "Back"] {
        dm.set(p, &format!("{face}Surface"), Variant::Enum(0))
            .expect("surface");
    }
    dm.set(p, &format!("{weld_face}Surface"), Variant::Enum(WELD))
        .expect("weld");
    dm.set_parent(p, Some(ws)).expect("parent");
    p
}

#[test]
fn block_welds_to_the_sloped_face_of_a_wedge() {
    let mut dm = DataModel::new();
    let wedge = part(
        &mut dm,
        "WedgePart",
        Vec3::splat(4.0),
        CFrame::IDENTITY,
        "Top",
    );
    let slope = Vec3::new(0.0, 1.0, -1.0).normalize();
    let block = part(
        &mut dm,
        "Part",
        Vec3::splat(2.0),
        CFrame::new(Mat3::from_rotation_x(-std::f32::consts::FRAC_PI_4), slope),
        "Bottom",
    );
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    world.join_all(&mut dm);
    let joints = world.joints_of(block);
    assert_eq!(joints.len(), 1, "the block should weld to the slope");
    assert!(world.joints_of(wedge).contains(&joints[0]));
}

#[test]
fn rotated_wedges_do_not_weld_across_the_slope() {
    let mut dm = DataModel::new();
    part(
        &mut dm,
        "WedgePart",
        Vec3::splat(4.0),
        CFrame::IDENTITY,
        "Front",
    );
    let block = part(
        &mut dm,
        "Part",
        Vec3::splat(2.0),
        CFrame::new(Mat3::IDENTITY, Vec3::new(0.0, 0.0, -3.0)),
        "Back",
    );
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    world.join_all(&mut dm);
    assert!(
        world.joints_of(block).is_empty(),
        "a wedge's front is its slope, so a block behind it has nothing to weld to"
    );
}
