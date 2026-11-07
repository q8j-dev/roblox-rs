mod common;

use common::Fixture;

const CELL: f64 = 4.0;

fn fixture() -> Fixture {
    let f = Fixture::new();
    f.rt.engine.lock().dm.ensure_terrain();
    f.execute(
        "local t = workspace.Terrain \
         t:SetCell(0, 0, 0, Enum.CellMaterial.Grass, Enum.CellBlock.Solid, Enum.CellOrientation.NegZ) \
         t:SetCell(0, 0, 0, Enum.CellMaterial.Empty, Enum.CellBlock.Solid, Enum.CellOrientation.X)",
    )
    .expect("terrain");
    f
}

fn solid(f: &Fixture, c: [i32; 3]) {
    f.execute(&format!(
        "workspace.Terrain:SetCell({}, {}, {}, Enum.CellMaterial.Grass, Enum.CellBlock.Solid, Enum.CellOrientation.NegZ)",
        c[0], c[1], c[2]
    ))
    .expect("solid");
}

fn water(f: &Fixture, c: [i32; 3]) {
    f.execute(&format!(
        "workspace.Terrain:SetWaterCell({}, {}, {}, Enum.WaterForce.None, Enum.WaterDirection.NegY)",
        c[0], c[1], c[2]
    ))
    .expect("water");
}

fn min(c: [i32; 3]) -> [f64; 3] {
    c.map(|v| v as f64 * CELL)
}

fn max(c: [i32; 3]) -> [f64; 3] {
    c.map(|v| v as f64 * CELL + CELL)
}

fn center(c: [i32; 3]) -> [f64; 3] {
    c.map(|v| v as f64 * CELL + CELL / 2.0)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

struct Hit {
    terrain: bool,
    nil: bool,
    position: [f64; 3],
    normal: [f64; 3],
}

fn cast(f: &Fixture, origin: [f64; 3], dir: [f64; 3]) -> Hit {
    let s = f.string(&format!(
        "local h, p, n = workspace:FindPartOnRay(Ray.new(Vector3.new({}, {}, {}), Vector3.new({}, {}, {}))) \
         return table.concat({{tostring(h == workspace.Terrain), tostring(h == nil), \
         string.format('%.9g', p.x), string.format('%.9g', p.y), string.format('%.9g', p.z), \
         string.format('%.9g', n.x), string.format('%.9g', n.y), string.format('%.9g', n.z)}}, ' ')",
        origin[0], origin[1], origin[2], dir[0], dir[1], dir[2]
    ));
    let parts: Vec<&str> = s.split(' ').collect();
    let num = |i: usize| parts[i].parse::<f64>().expect("number");
    Hit {
        terrain: parts[0] == "true",
        nil: parts[1] == "true",
        position: [num(2), num(3), num(4)],
        normal: [num(5), num(6), num(7)],
    }
}

fn check(message: &str, hit: &Hit, terrain: bool, expected: [f64; 3]) {
    if terrain {
        assert!(hit.terrain, "{message}: expected terrain hit");
    } else {
        assert!(hit.nil, "{message}: expected no hit");
    }
    for i in 0..3 {
        assert!(
            (hit.position[i] - expected[i]).abs() < 1e-4,
            "{message}: hit {:?} != {:?}",
            hit.position,
            expected
        );
    }
}

#[test]
fn check_invalid_float_value() {
    let f = fixture();
    f.execute(
        "local zero = 0 \
         local v = {Vector3.new(0, 1, 0), Vector3.new(1 / zero, 0, 0), Vector3.new(0, -1 / zero, 0), Vector3.new(0, 0, zero / zero)} \
         for i = 1, 4 do for j = 1, 4 do workspace:FindPartOnRay(Ray.new(v[i], v[j])) end end",
    )
    .expect("invalid floats");
}

#[test]
fn origin_close_to_entry_point() {
    let f = fixture();
    cast(&f, [0.0, -1e-7, 0.0], [0.0, 3.0, 0.0]);
}

#[test]
fn check_cluster_boundary_empty_cluster() {
    let f = fixture();
    let outside = min([-1, -1, -1]);
    let inside = max([4, 4, 4]);
    let hit = cast(&f, outside, sub(inside, outside));
    check("outside => inside", &hit, false, inside);
    let hit = cast(&f, inside, sub(outside, inside));
    check("inside => outside", &hit, false, outside);
}

#[test]
fn check_cluster_boundary_with_water() {
    let f = fixture();
    water(&f, [0, 0, 0]);
    let outside = min([-1, -1, -1]);
    let inside = max([4, 4, 4]);
    let hit = cast(&f, outside, sub(inside, outside));
    check("outside => inside", &hit, true, min([0, 0, 0]));
    let hit = cast(&f, inside, sub(outside, inside));
    check("inside => outside", &hit, true, max([0, 0, 0]));
}

#[test]
fn check_cluster_boundary_with_water_through_to_origin() {
    let f = fixture();
    f.execute(
        "for y = 0, 5 do for z = 0, 5 do for x = 0, 5 do \
         workspace.Terrain:SetWaterCell(x, y, z, Enum.WaterForce.None, Enum.WaterDirection.NegY) \
         end end end",
    )
    .expect("water");
    let outside = min([-1, -1, -1]);
    let inside = max([4, 4, 4]);
    let boundary = min([0, 0, 0]);
    let hit = cast(&f, outside, sub(inside, outside));
    check("outside => inside", &hit, true, boundary);
    let hit = cast(&f, inside, sub(outside, inside));
    check("inside => outside", &hit, true, boundary);
}

#[test]
fn start_outside_water_hits_water_face() {
    let f = fixture();
    water(&f, [6, 4, 4]);
    let start = center([4, 4, 4]);
    let end = center([8, 4, 4]);
    let c = center([6, 4, 4]);
    let hit = cast(&f, start, sub(end, start));
    check("+X ray", &hit, true, [min([6, 4, 4])[0], c[1], c[2]]);
}

#[test]
fn start_inside_water_hits_water_air_interface() {
    let f = fixture();
    for x in 4..7 {
        water(&f, [x, 4, 4]);
    }
    let start = center([4, 4, 4]);
    let end = center([8, 4, 4]);
    let c = center([6, 4, 4]);
    let hit = cast(&f, start, sub(end, start));
    check("+X ray", &hit, true, [max([6, 4, 4])[0], c[1], c[2]]);
}

#[test]
fn start_inside_water_hits_water_block_interface() {
    let f = fixture();
    for x in 4..6 {
        water(&f, [x, 4, 4]);
    }
    solid(&f, [6, 4, 4]);
    let start = center([4, 4, 4]);
    let end = center([8, 4, 4]);
    let c = center([6, 4, 4]);
    let hit = cast(&f, start, sub(end, start));
    check("+X ray", &hit, true, [min([6, 4, 4])[0], c[1], c[2]]);
}

#[test]
fn hits_wedge_in_water_wedge_as_appropriate() {
    let f = fixture();
    for x in 4..=7 {
        water(&f, [x, 4, 4]);
    }
    f.execute(
        "workspace.Terrain:SetCell(6, 4, 4, Enum.CellMaterial.Grass, Enum.CellBlock.VerticalWedge, Enum.CellOrientation.NegZ)",
    )
    .expect("wedge");
    water(&f, [6, 4, 5]);
    let start_min = min([4, 4, 4]);
    let start_center = center([4, 4, 4]);
    let direction = sub(center([8, 4, 4]), start_center);
    let hit_start = [start_center[0], start_min[1] + 0.25 * CELL, start_center[2]];
    let miss_start = [start_center[0], start_min[1] + 0.75 * CELL, start_center[2]];
    let wedge_center = center([6, 4, 4]);
    let hit = cast(&f, hit_start, direction);
    check(
        "hit wedge",
        &hit,
        true,
        [min([6, 4, 4])[0], hit_start[1], wedge_center[2]],
    );
    let hit = cast(&f, miss_start, direction);
    check(
        "miss wedge",
        &hit,
        true,
        [max([7, 4, 4])[0], miss_start[1], wedge_center[2]],
    );
}

#[test]
fn get_surface_returns_cell_surface() {
    let f = fixture();
    let cell = [150, 45, 220];
    solid(&f, cell);
    let c = center(cell);
    let delta = 2.0 * CELL;
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut origin = c;
            origin[axis] += sign * delta;
            let mut dir = [0.0; 3];
            dir[axis] = -sign * 2.0 * delta;
            let hit = cast(&f, origin, dir);
            assert!(hit.terrain, "axis {axis} sign {sign}");
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            assert_eq!(hit.normal, normal, "axis {axis} sign {sign}");
        }
    }
}

#[test]
fn get_surface_does_not_hit_terrain_edge() {
    let f = fixture();
    let cell = [0, 20, 0];
    solid(&f, cell);
    let corner = min(cell);
    let start = [
        corner[0] + CELL / 2.0,
        corner[1] - CELL,
        corner[2] - CELL / 2.0,
    ];
    let hit = cast(&f, start, [0.0, 10.0 * CELL, 10.0 * CELL]);
    assert!(hit.terrain);
    assert_eq!(hit.normal, [0.0, -1.0, 0.0]);
}

#[test]
fn bounding_box_includes_terrain_but_ray_misses_terrain() {
    let f = fixture();
    solid(&f, [0, 0, 0]);
    let start = center([-2, 0, 0]);
    let end = center([0, -2, 0]);
    let hit = cast(&f, start, sub(end, start));
    check("outside cluster", &hit, false, end);
}

#[test]
fn ray_involves_three_chunks() {
    let f = fixture();
    solid(&f, [0, 0, 0]);
    let start = [0.0, 15.0, 15.0];
    let end = [3.5 * 32.0 * 4.0, 15.0, 15.0];
    let hit = cast(&f, start, sub(end, start));
    check("outside cluster", &hit, false, end);
}

#[test]
fn terrain_loop_repro() {
    let f = fixture();
    solid(&f, [0, 0, 0]);
    let start = [-5.8e-22f32 as f64, 253.4f32 as f64, -0.5];
    let end = [-5.6e-22f32 as f64, 258.343f32 as f64, -9.48f32 as f64];
    let hit = cast(&f, start, sub(end, start));
    check("outside cluster", &hit, false, end);
}

fn region_empty(f: &Fixture, lo: [f64; 3], hi: [f64; 3]) -> bool {
    f.bool(&format!(
        "return workspace:IsRegion3Empty(Region3.new(Vector3.new({}, {}, {}), Vector3.new({}, {}, {})))",
        lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
    ))
}

#[test]
fn is_region3_empty() {
    let inside = ([10.0, 10.0, 10.0], [30.0, 30.0, 30.0]);
    let envelope = ([-2000.0, -10.0, -2000.0], [2000.0, 1000.0, 2000.0]);
    let both = [5, 5, 5];
    let bigger = [0, 0, 0];

    let f = Fixture::new();
    assert!(region_empty(&f, inside.0, inside.1));
    assert!(region_empty(&f, envelope.0, envelope.1));

    let f = fixture();
    solid(&f, both);
    assert!(!region_empty(&f, inside.0, inside.1));
    assert!(!region_empty(&f, envelope.0, envelope.1));

    let f = fixture();
    water(&f, both);
    assert!(region_empty(&f, inside.0, inside.1));
    assert!(region_empty(&f, envelope.0, envelope.1));

    let f = fixture();
    solid(&f, bigger);
    assert!(region_empty(&f, inside.0, inside.1));
    assert!(!region_empty(&f, envelope.0, envelope.1));

    let f = fixture();
    water(&f, both);
    assert!(region_empty(&f, inside.0, inside.1));
    assert!(region_empty(&f, envelope.0, envelope.1));
}
