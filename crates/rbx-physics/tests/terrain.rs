use rbx_dom::rbx_terrain::smooth::{Cell, CellBox, Region};
use rbx_dom::rbx_terrain::{LegacyGrid, SmoothGrid, legacy};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, IVec3, Vec3};
use rbx_physics::PhysicsWorld;

fn setup() -> (DataModel, InstanceId, InstanceId) {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    let t = dm.create("Terrain").expect("terrain");
    dm.set_parent(t, Some(ws)).expect("parent");
    (dm, ws, t)
}

fn part(dm: &mut DataModel, parent: InstanceId, class: &str, size: Vec3, pos: Vec3) -> InstanceId {
    let p = dm.create(class).expect("part");
    dm.set(p, "Size", Variant::Vector3(size)).expect("size");
    dm.set(p, "CFrame", Variant::CFrame(CFrame::from_translation(pos)))
        .expect("cframe");
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

fn simulate(dm: &mut DataModel, seconds: f64) -> Vec<(InstanceId, InstanceId)> {
    let mut world = PhysicsWorld::new();
    world.attach(dm);
    let mut touched = Vec::new();
    for _ in 0..(seconds * 60.0) as usize {
        let e = world.step(dm, 1.0 / 60.0);
        touched.extend(e.touched);
    }
    touched
}

#[test]
fn block_rests_on_legacy_terrain() {
    let (mut dm, ws, t) = setup();
    let mut g = LegacyGrid::new();
    for x in -4..4 {
        for z in -4..4 {
            g.set_cell(
                IVec3::new(x, 0, z),
                legacy::solid_cell(legacy::BLOCK_SOLID, 0),
                1,
            );
        }
    }
    dm.terrain_mut(t).legacy = Some(g);
    let p = part(
        &mut dm,
        ws,
        "Part",
        Vec3::new(4.0, 2.0, 4.0),
        Vec3::new(1.0, 12.0, 1.0),
    );
    let ball = part(
        &mut dm,
        ws,
        "Part",
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::new(-6.0, 20.0, -6.0),
    );
    dm.set(ball, "Shape", Variant::Enum(0)).expect("shape");
    let touched = simulate(&mut dm, 3.0);
    let y = pos(&dm, p).y;
    assert!((y - 5.2).abs() < 0.05, "block rests at {y}");
    let by = pos(&dm, ball).y;
    assert!((by - 5.0).abs() < 0.05, "ball rests at {by}");
    assert!(touched.iter().any(|(a, b)| *a == p && *b == t));
}

#[test]
fn block_slides_off_wedge_cell() {
    let (mut dm, ws, t) = setup();
    let mut g = LegacyGrid::new();
    g.set_cell(
        IVec3::new(0, 0, 0),
        legacy::solid_cell(legacy::BLOCK_VERTICAL_WEDGE, 0),
        1,
    );
    dm.terrain_mut(t).legacy = Some(g);
    let p = part(
        &mut dm,
        ws,
        "Part",
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(2.0, 6.0, 2.0),
    );
    simulate(&mut dm, 3.0);
    let at = pos(&dm, p);
    assert!(at.y < 3.0, "block should leave the wedge, at {at}");
}

#[test]
fn block_rests_on_smooth_terrain() {
    let (mut dm, ws, t) = setup();
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
    let p = part(
        &mut dm,
        ws,
        "Part",
        Vec3::new(4.0, 2.0, 4.0),
        Vec3::new(0.0, 10.0, 0.0),
    );
    simulate(&mut dm, 3.0);
    let cf = dm
        .get(p, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .expect("cf");
    let y = cf.translation.y;
    assert!((y - 3.2).abs() < 0.05, "block rests at {y}");
}

#[test]
fn plastic_block_floats_in_legacy_water() {
    let (mut dm, ws, t) = setup();
    let mut g = LegacyGrid::new();
    for x in -4..4 {
        for z in -4..4 {
            g.set_cell(
                IVec3::new(x, -1, z),
                legacy::solid_cell(legacy::BLOCK_SOLID, 0),
                1,
            );
            for y in 0..3 {
                g.set_cell(
                    IVec3::new(x, y, z),
                    legacy::water_cell(0, 0),
                    legacy::MATERIAL_WATER,
                );
            }
        }
    }
    dm.terrain_mut(t).legacy = Some(g);
    let p = part(
        &mut dm,
        ws,
        "Part",
        Vec3::new(4.0, 4.8, 4.0),
        Vec3::new(2.0, 9.0, 2.0),
    );
    dm.set(p, "FormFactor", Variant::Enum(3)).expect("ff");
    dm.set(p, "Size", Variant::Vector3(Vec3::new(4.0, 4.0, 4.0)))
        .expect("size");
    simulate(&mut dm, 8.0);
    let y = pos(&dm, p).y;
    assert!(y > 9.0 && y < 12.0, "block floats at {y}");
}
