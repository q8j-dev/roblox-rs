use crate::geometry::{Mesh, NO_OUTLINES, Vertex};
use rbx_math::{IVec3, Mat3, Vec2, Vec3};
use rbx_terrain::LegacyGrid;
use rbx_terrain::legacy::{self, Face};
use std::sync::LazyLock;

const CELL: f32 = 4.0;
const ATLAS: f32 = 2048.0;
const MATERIAL_PIXELS: i32 = 512;

#[derive(Clone, Copy)]
struct Region {
    start: (i32, i32),
    end: (i32, i32),
    cells: (i32, i32),
}

const fn r(sx: i32, sy: i32, ex: i32, ey: i32, cx: i32, cy: i32) -> Region {
    Region {
        start: (sx, sy),
        end: (ex, ey),
        cells: (cx, cy),
    }
}

const TOP: [Region; 2] = [r(8, 8, 248, 248, 4, 4), r(8, 8, 248, 248, 16, 16)];
const TOP_SIDE: [Region; 2] = [r(264, 8, 504, 68, 4, 1), r(264, 8, 504, 68, 16, 4)];
const SIDE: [Region; 2] = [r(264, 68, 504, 188, 4, 2), r(264, 68, 504, 188, 16, 8)];
const WEDGE_VERTICAL: [Region; 2] = [r(8, 332, 248, 504, 4, 2), r(8, 332, 248, 504, 16, 8)];
const WEDGE_HORIZONTAL: [Region; 2] = [r(332, 384, 504, 504, 2, 2), r(332, 384, 504, 504, 8, 8)];
const BOTTOM: [Region; 2] = [r(8, 264, 216, 316, 4, 1), r(8, 264, 216, 316, 16, 4)];

const ATLAS_OFFSET: [(i32, i32); 16] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (0, 1),
    (3, 1),
    (1, 1),
    (2, 1),
    (2, 2),
    (3, 2),
    (0, 3),
    (1, 3),
    (2, 3),
    (3, 3),
    (0, 2),
    (1, 2),
];

struct ShapeFace {
    normal: Vec3,
    points: Vec<Vec3>,
    axis: Option<usize>,
}

fn hull_faces(points: &[[f32; 3]]) -> Vec<ShapeFace> {
    let pts: Vec<Vec3> = points
        .iter()
        .map(|p| Vec3::from_array(*p) * (CELL * 0.5))
        .collect();
    let center = pts.iter().fold(Vec3::ZERO, |a, b| a + *b) / pts.len() as f32;
    let mut faces: Vec<ShapeFace> = Vec::new();
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            for k in j + 1..pts.len() {
                let mut n = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                if n.length_squared() < 1e-6 {
                    continue;
                }
                n = n.normalize();
                if n.dot(pts[i] - center) < 0.0 {
                    n = -n;
                }
                let d = n.dot(pts[i]);
                if pts.iter().any(|p| n.dot(*p) > d + 1e-4) {
                    continue;
                }
                if faces.iter().any(|f| f.normal.dot(n) > 0.9999) {
                    continue;
                }
                let mut on: Vec<Vec3> = pts
                    .iter()
                    .copied()
                    .filter(|p| (n.dot(*p) - d).abs() < 1e-4)
                    .collect();
                let c = on.iter().fold(Vec3::ZERO, |a, b| a + *b) / on.len() as f32;
                let u = (on[0] - c).normalize_or(Vec3::X);
                let v = n.cross(u);
                on.sort_by(|a, b| {
                    let aa = (*a - c).dot(v).atan2((*a - c).dot(u));
                    let bb = (*b - c).dot(v).atan2((*b - c).dot(u));
                    aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
                });
                let axis = [
                    (Vec3::X, 0usize),
                    (Vec3::Z, 1),
                    (-Vec3::X, 2),
                    (-Vec3::Z, 3),
                    (Vec3::Y, 4),
                    (-Vec3::Y, 5),
                ]
                .iter()
                .find(|(a, _)| a.dot(n) > 0.9999 && (d - CELL * 0.5).abs() < 1e-3)
                .map(|(_, f)| *f);
                faces.push(ShapeFace {
                    normal: n,
                    points: on,
                    axis,
                });
            }
        }
    }
    faces
}

static SHAPE_FACES: LazyLock<Vec<Vec<ShapeFace>>> =
    LazyLock::new(|| legacy::BLOCK_POINTS.iter().map(|p| hull_faces(p)).collect());

const FACES: [Face; 6] = [
    Face::PlusX,
    Face::PlusZ,
    Face::MinusX,
    Face::MinusZ,
    Face::PlusY,
    Face::MinusY,
];
const OFFSETS: [IVec3; 6] = [
    IVec3::new(1, 0, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(-1, 0, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 1, 0),
    IVec3::new(0, -1, 0),
];
const OPPOSITE: [usize; 6] = [2, 3, 0, 1, 5, 4];

fn rotate_axis(axis: usize, orientation: u8) -> usize {
    if axis >= 4 {
        return axis;
    }
    let rot = [0usize, 3, 2, 1];
    (axis + rot[orientation as usize % 4]) % 4
}

fn region_uv(region: &Region, cell: (i32, i32), material: u8, corner: Vec2) -> Vec2 {
    let (ox, oy) = ATLAS_OFFSET[(material.saturating_sub(1) as usize).min(15)];
    let one = (
        (region.end.0 - region.start.0) / region.cells.0,
        (region.end.1 - region.start.1) / region.cells.1,
    );
    let mx = cell.0.rem_euclid(region.cells.0);
    let my = cell.1.rem_euclid(region.cells.1);
    let minx = region.start.0 + mx * one.0 + ox * MATERIAL_PIXELS;
    let miny = region.start.1 + my * one.1 + oy * MATERIAL_PIXELS;
    Vec2::new(
        (minx as f32 + corner.x * one.0 as f32) / ATLAS,
        (miny as f32 + corner.y * one.1 as f32) / ATLAS,
    )
}

pub fn legacy_chunk(grid: &LegacyGrid, chunk: IVec3) -> Mesh {
    let mut mesh = Mesh::default();
    let lo = legacy::chunk_min(chunk);
    let hi = legacy::chunk_max(chunk);
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let pos = IVec3::new(x, y, z);
                let cell = grid.get_cell(pos);
                let block = legacy::block(cell);
                if block >= legacy::BLOCK_EMPTY {
                    continue;
                }
                let material = grid.get_material(pos);
                let orientation = legacy::orientation(cell);
                let rot = Mat3::from_rotation_y(orientation as f32 * std::f32::consts::FRAC_PI_2);
                let center = legacy::cell_center(pos);
                let above = grid.get_cell(pos + IVec3::Y);
                let open_above = legacy::block(above) >= legacy::BLOCK_EMPTY;
                for face in &SHAPE_FACES[block as usize] {
                    let world_axis = face.axis.map(|a| rotate_axis(a, orientation));
                    if let Some(a) = world_axis {
                        let neighbor = grid.get_cell(pos + OFFSETS[a]);
                        if legacy::block(neighbor) < legacy::BLOCK_EMPTY
                            && legacy::oriented_face(neighbor, FACES[OPPOSITE[a]])
                                == legacy::SKIP_NONE
                        {
                            continue;
                        }
                    }
                    let n = rot * face.normal;
                    let pts: Vec<Vec3> = face.points.iter().map(|p| center + rot * *p).collect();
                    let regions = match world_axis {
                        Some(4) => TOP,
                        Some(5) => BOTTOM,
                        Some(_) => {
                            if open_above {
                                TOP_SIDE
                            } else {
                                SIDE
                            }
                        }
                        None => {
                            if block == legacy::BLOCK_HORIZONTAL_WEDGE {
                                WEDGE_HORIZONTAL
                            } else {
                                WEDGE_VERTICAL
                            }
                        }
                    };
                    let (u_axis, v_axis) = if n.y.abs() > 0.7 {
                        (Vec3::X, Vec3::Z)
                    } else {
                        let side = Vec3::Y.cross(n).normalize_or(Vec3::X);
                        (side, -Vec3::Y)
                    };
                    let cell_uv = (
                        (center.dot(u_axis) / CELL).floor() as i32,
                        (center.dot(v_axis) / CELL).floor() as i32,
                    );
                    let base = mesh.vertices.len() as u32;
                    for p in &pts {
                        let local = *p - center;
                        let corner = Vec2::new(
                            (local.dot(u_axis) / CELL + 0.5).clamp(0.0, 1.0),
                            (local.dot(v_axis) / CELL + 0.5).clamp(0.0, 1.0),
                        );
                        let high = region_uv(&regions[0], cell_uv, material, corner);
                        let low = region_uv(&regions[1], cell_uv, material, corner);
                        mesh.vertices.push(Vertex {
                            pos: p.to_array(),
                            normal: n.to_array(),
                            uv: high.to_array(),
                            uv_studs: low.to_array(),
                            tangent: u_axis.to_array(),
                            edge: [NO_OUTLINES; 4],
                        });
                    }
                    let first = pts.len();
                    for i in 1..first.saturating_sub(1) {
                        let (a, b, c) = (pts[0], pts[i], pts[i + 1]);
                        if (b - a).cross(c - a).dot(n) >= 0.0 {
                            mesh.indices.extend_from_slice(&[
                                base,
                                base + i as u32,
                                base + i as u32 + 1,
                            ]);
                        } else {
                            mesh.indices.extend_from_slice(&[
                                base,
                                base + i as u32 + 1,
                                base + i as u32,
                            ]);
                        }
                    }
                }
            }
        }
    }
    mesh
}

pub fn legacy_water(grid: &LegacyGrid, chunk: IVec3) -> crate::smooth::SmoothMesh {
    let mut mesh = crate::smooth::SmoothMesh::default();
    let lo = legacy::chunk_min(chunk);
    let hi = legacy::chunk_max(chunk);
    let has_water = |p: IVec3| {
        legacy::block(grid.get_water_cell(p)) == legacy::BLOCK_EMPTY
            && !legacy::is_empty(grid.get_water_cell(p))
    };
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let pos = IVec3::new(x, y, z);
                if !has_water(pos) {
                    continue;
                }
                let center = legacy::cell_center(pos);
                for (a, offset) in OFFSETS.iter().enumerate() {
                    let n = pos + *offset;
                    if has_water(n) {
                        continue;
                    }
                    let neighbor = grid.get_cell(n);
                    if legacy::block(neighbor) < legacy::BLOCK_EMPTY
                        && legacy::oriented_face(neighbor, FACES[OPPOSITE[a]]) == legacy::SKIP_NONE
                    {
                        continue;
                    }
                    let normal = offset.as_vec3();
                    let (u, v) = if normal.y.abs() > 0.5 {
                        (Vec3::X, Vec3::Z * normal.y)
                    } else {
                        (Vec3::Y.cross(normal), Vec3::Y)
                    };
                    let face = center + normal * (CELL * 0.5);
                    let base = mesh.vertices.len() as u32;
                    for (cu, cv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                        let p = face + (u * cu + v * cv) * (CELL * 0.5);
                        mesh.vertices.push(crate::smooth::SmoothVertex {
                            pos: p.to_array(),
                            normal: normal.to_array(),
                            weights: [1.0, 0.0, 0.0],
                            ..Default::default()
                        });
                    }
                    let (p0, p1, p2) = (
                        Vec3::from_array(mesh.vertices[base as usize].pos),
                        Vec3::from_array(mesh.vertices[base as usize + 1].pos),
                        Vec3::from_array(mesh.vertices[base as usize + 2].pos),
                    );
                    if (p1 - p0).cross(p2 - p0).dot(normal) >= 0.0 {
                        mesh.water.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                    } else {
                        mesh.water.extend_from_slice(&[
                            base,
                            base + 2,
                            base + 1,
                            base,
                            base + 3,
                            base + 2,
                        ]);
                    }
                }
            }
        }
    }
    mesh
}
