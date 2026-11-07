use crate::shape::ShapeKind;
use crate::world::{Part, PhysicsWorld};
use rbx_dom::{DataModel, InstanceId};
use rbx_math::{CFrame, IVec3, Vec2, Vec3};

const ANGLE_MAX: f32 = 0.01;
const PLANAR_MAX: f32 = 0.02;
const OVERLAP_AREA_MIN: f32 = 1e-4;
const SEARCH_MARGIN: f32 = 0.05;
const TERRAIN_CELL: f32 = 4.0;

const SMOOTH: u32 = 0;
const GLUE: u32 = 1;
const WELD: u32 = 2;
const STUDS: u32 = 3;
const INLET: u32 = 4;
const UNIVERSAL: u32 = 5;
const ROTATE: u32 = 6;
const ROTATE_V: u32 = 7;
const ROTATE_P: u32 = 8;
const NO_JOIN: u32 = 9;

#[derive(Clone, Debug)]
pub struct ManualJoint {
    pub part0: InstanceId,
    pub part1: InstanceId,
    pub c0: CFrame,
    pub c1: CFrame,
    pub surface0: u32,
    pub surface1: u32,
    pub parent: InstanceId,
    pub terrain: bool,
}

#[derive(Clone, Debug)]
struct Polygon {
    verts: Vec<Vec3>,
    normal: Vec3,
    surface: usize,
}

impl Polygon {
    fn origin(&self) -> Vec3 {
        (self.verts[0] + self.verts[1]) * 0.5
    }
}

fn local_faces(kind: ShapeKind, size: Vec3) -> Vec<(Vec<Vec3>, usize)> {
    let h = size * 0.5;
    let v = Vec3::new;
    match kind {
        ShapeKind::Block => vec![
            (
                vec![
                    v(h.x, -h.y, -h.z),
                    v(h.x, h.y, -h.z),
                    v(h.x, h.y, h.z),
                    v(h.x, -h.y, h.z),
                ],
                0,
            ),
            (
                vec![
                    v(-h.x, h.y, -h.z),
                    v(-h.x, h.y, h.z),
                    v(h.x, h.y, h.z),
                    v(h.x, h.y, -h.z),
                ],
                1,
            ),
            (
                vec![
                    v(-h.x, -h.y, h.z),
                    v(h.x, -h.y, h.z),
                    v(h.x, h.y, h.z),
                    v(-h.x, h.y, h.z),
                ],
                2,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(-h.x, -h.y, h.z),
                    v(-h.x, h.y, h.z),
                    v(-h.x, h.y, -h.z),
                ],
                3,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(h.x, -h.y, -h.z),
                    v(h.x, -h.y, h.z),
                    v(-h.x, -h.y, h.z),
                ],
                4,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(-h.x, h.y, -h.z),
                    v(h.x, h.y, -h.z),
                    v(h.x, -h.y, -h.z),
                ],
                5,
            ),
        ],
        ShapeKind::Wedge => vec![
            (
                vec![v(h.x, -h.y, -h.z), v(h.x, h.y, h.z), v(h.x, -h.y, h.z)],
                0,
            ),
            (
                vec![v(-h.x, -h.y, -h.z), v(-h.x, -h.y, h.z), v(-h.x, h.y, h.z)],
                3,
            ),
            (
                vec![
                    v(-h.x, -h.y, h.z),
                    v(h.x, -h.y, h.z),
                    v(h.x, h.y, h.z),
                    v(-h.x, h.y, h.z),
                ],
                2,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(h.x, -h.y, -h.z),
                    v(h.x, -h.y, h.z),
                    v(-h.x, -h.y, h.z),
                ],
                4,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(-h.x, h.y, h.z),
                    v(h.x, h.y, h.z),
                    v(h.x, -h.y, -h.z),
                ],
                1,
            ),
        ],
        ShapeKind::CornerWedge => vec![
            (
                vec![v(h.x, -h.y, h.z), v(h.x, -h.y, -h.z), v(h.x, h.y, -h.z)],
                0,
            ),
            (
                vec![v(h.x, -h.y, -h.z), v(-h.x, -h.y, -h.z), v(h.x, h.y, -h.z)],
                5,
            ),
            (
                vec![
                    v(-h.x, -h.y, -h.z),
                    v(h.x, -h.y, -h.z),
                    v(h.x, -h.y, h.z),
                    v(-h.x, -h.y, h.z),
                ],
                4,
            ),
            (
                vec![v(-h.x, -h.y, -h.z), v(-h.x, -h.y, h.z), v(h.x, h.y, -h.z)],
                3,
            ),
            (
                vec![v(-h.x, -h.y, h.z), v(h.x, -h.y, h.z), v(h.x, h.y, -h.z)],
                2,
            ),
        ],
        _ => Vec::new(),
    }
}

fn world_faces(part: &Part) -> Vec<Polygon> {
    let cf = part.cframe;
    let centroid: Vec3 = Vec3::ZERO;
    local_faces(part.shape.kind, part.shape.size)
        .into_iter()
        .map(|(verts, surface)| {
            let n = (verts[1] - verts[0])
                .cross(verts[2] - verts[0])
                .normalize_or(Vec3::Y);
            let mid = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
            let n = if n.dot(mid - centroid) < 0.0 { -n } else { n };
            Polygon {
                verts: verts.iter().map(|p| cf * *p).collect(),
                normal: cf.rotation * n,
                surface,
            }
        })
        .collect()
}

fn plane_basis(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = helper.cross(n).normalize_or(Vec3::X);
    (u, n.cross(u))
}

fn project(points: &[Vec3], origin: Vec3, u: Vec3, v: Vec3) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = points
        .iter()
        .map(|p| Vec2::new((*p - origin).dot(u), (*p - origin).dot(v)))
        .collect();
    if signed_area(&out) < 0.0 {
        out.reverse();
    }
    out
}

fn signed_area(poly: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

fn clip(subject: &[Vec2], clipper: &[Vec2]) -> Vec<Vec2> {
    let mut out = subject.to_vec();
    for i in 0..clipper.len() {
        if out.is_empty() {
            break;
        }
        let a = clipper[i];
        let b = clipper[(i + 1) % clipper.len()];
        let inside = |p: Vec2| (b - a).perp_dot(p - a) >= 0.0;
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let p = input[j];
            let q = input[(j + 1) % input.len()];
            let (pi, qi) = (inside(p), inside(q));
            if pi {
                out.push(p);
            }
            if pi != qi {
                let d1 = (b - a).perp_dot(p - a);
                let d2 = (b - a).perp_dot(q - a);
                let t = d1 / (d1 - d2);
                out.push(p + (q - p) * t);
            }
        }
    }
    out
}

fn overlap_area(a: &Polygon, b: &[Vec3]) -> f32 {
    let (u, v) = plane_basis(a.normal);
    let origin = a.verts[0];
    let pa = project(&a.verts, origin, u, v);
    let pb = project(b, origin, u, v);
    signed_area(&clip(&pa, &pb)).abs()
}

fn touching(a: &Polygon, b: &Polygon) -> bool {
    if (1.0 + a.normal.dot(b.normal)).abs() > ANGLE_MAX {
        return false;
    }
    if (b.origin() - a.origin()).dot(a.normal).abs() > PLANAR_MAX {
        return false;
    }
    overlap_area(a, &b.verts) > OVERLAP_AREA_MIN
}

fn touching_surfaces(found: &Part, selected: &Part) -> Option<(Polygon, Polygon)> {
    let fa = world_faces(found);
    let fb = world_faces(selected);
    for a in &fa {
        for b in &fb {
            if touching(a, b) {
                return Some((a.clone(), b.clone()));
            }
        }
    }
    None
}

fn surface_coord(face: &Polygon, part: &Part) -> CFrame {
    let (u, v) = plane_basis(face.normal);
    let world = CFrame::new(rbx_math::Mat3::from_cols(u, v, face.normal), face.origin());
    part.cframe.inverse() * world
}

fn character_part(dm: &DataModel, id: InstanceId) -> bool {
    let mut p = dm.parent_of(id);
    while let Some(cur) = p {
        if dm.is_a(cur, "Model") && dm.find_first_child_of_class(cur, "Humanoid").is_some() {
            return true;
        }
        p = dm.parent_of(cur);
    }
    false
}

fn manual_pair(t0: u32, t1: u32) -> bool {
    if t0 == NO_JOIN || t1 == NO_JOIN {
        return false;
    }
    let snap = (t0 == STUDS && t1 == INLET)
        || (t0 == INLET && t1 == STUDS)
        || (t0 == UNIVERSAL && t1 == UNIVERSAL)
        || (t0 == UNIVERSAL && (t1 == INLET || t1 == STUDS))
        || ((t0 == INLET || t0 == STUDS) && t1 == UNIVERSAL);
    if snap {
        return false;
    }
    if t0 == GLUE || t1 == GLUE || t0 == WELD || t1 == WELD {
        return false;
    }
    let rotate = |t: u32| t == ROTATE || t == ROTATE_V || t == ROTATE_P;
    if rotate(t0) || rotate(t1) {
        return false;
    }
    let incompatible = (t0 == STUDS && t1 != INLET)
        || (t1 == STUDS && t0 != INLET)
        || (t0 == UNIVERSAL && t1 != STUDS && t1 != INLET)
        || (t1 == UNIVERSAL && t0 != STUDS && t0 != INLET);
    !incompatible && (t0 == SMOOTH || t0 == INLET) && (t1 == SMOOTH || t1 == INLET)
}

impl PhysicsWorld {
    pub fn manual_joints(
        &mut self,
        dm: &mut DataModel,
        selected: &[InstanceId],
        terrain: Option<InstanceId>,
    ) -> Vec<ManualJoint> {
        self.sync(dm);
        let mut out = Vec::new();
        for &sid in selected {
            let Some(&si) = self.index.get(&sid) else {
                continue;
            };
            let sel = self.parts[si].clone();
            let lo = crate::math::from_parry_vec(sel.aabb.mins) - Vec3::splat(SEARCH_MARGIN);
            let hi = crate::math::from_parry_vec(sel.aabb.maxs) + Vec3::splat(SEARCH_MARGIN);
            let mut found: Vec<usize> = (0..self.parts.len())
                .filter(|&j| {
                    let a = &self.parts[j].aabb;
                    let (amin, amax) = (
                        crate::math::from_parry_vec(a.mins),
                        crate::math::from_parry_vec(a.maxs),
                    );
                    j != si
                        && amin.x <= hi.x
                        && amax.x >= lo.x
                        && amin.y <= hi.y
                        && amax.y >= lo.y
                        && amin.z <= hi.z
                        && amax.z >= lo.z
                })
                .collect();
            found.sort_by_key(|&j| self.parts[j].id);
            for j in found {
                let other = self.parts[j].clone();
                if selected.contains(&other.id) {
                    continue;
                }
                let Some((f_found, f_sel)) = touching_surfaces(&other, &sel) else {
                    continue;
                };
                if character_part(dm, other.id) || character_part(dm, sel.id) {
                    continue;
                }
                let t0 = other.surfaces[f_found.surface];
                let t1 = sel.surfaces[f_sel.surface];
                if !manual_pair(t0, t1) {
                    continue;
                }
                let c0 = surface_coord(&f_found, &other);
                let world = other.cframe * c0;
                let c1 = sel.cframe.inverse() * world;
                out.push(ManualJoint {
                    part0: other.id,
                    part1: sel.id,
                    c0,
                    c1,
                    surface0: f_found.surface as u32,
                    surface1: f_sel.surface as u32,
                    parent: other.id,
                    terrain: false,
                });
            }
            if let Some(t) = terrain {
                out.extend(self.terrain_manual_joints(dm, t, &sel));
            }
        }
        out
    }

    fn terrain_manual_joints(
        &self,
        dm: &DataModel,
        terrain: InstanceId,
        part: &Part,
    ) -> Vec<ManualJoint> {
        let mut out = Vec::new();
        if matches!(part.shape.kind, ShapeKind::Ball) {
            return out;
        }
        let Some(data) = dm.terrain(terrain) else {
            return out;
        };
        let weld = |surface: usize| ManualJoint {
            part0: terrain,
            part1: part.id,
            c0: CFrame::IDENTITY,
            c1: part.cframe.inverse(),
            surface0: 0,
            surface1: surface as u32,
            parent: part.id,
            terrain: true,
        };
        if let Some(grid) = data
            .legacy
            .as_ref()
            .filter(|g| g.non_empty_cell_count() > 0)
        {
            let mut cells: Vec<(IVec3, usize)> = Vec::new();
            for face in world_faces(part) {
                let axis = (0..3).find(|&k| (face.normal[k].abs() - 1.0).abs() <= ANGLE_MAX);
                let Some(k) = axis else {
                    continue;
                };
                let plane = face.origin()[k];
                let lo = face
                    .verts
                    .iter()
                    .fold(Vec3::splat(f32::INFINITY), |m, p| m.min(*p));
                let hi = face
                    .verts
                    .iter()
                    .fold(Vec3::splat(f32::NEG_INFINITY), |m, p| m.max(*p));
                let mut probe = (lo + hi) * 0.5;
                probe[k] = plane + face.normal[k].signum() * TERRAIN_CELL * 0.5;
                let corner_lo = rbx_dom::rbx_terrain::legacy::world_to_cell_floor(lo);
                let corner_hi = rbx_dom::rbx_terrain::legacy::world_to_cell_floor(hi);
                let layer = rbx_dom::rbx_terrain::legacy::world_to_cell_floor(probe)[k];
                for x in corner_lo.x..=corner_hi.x {
                    for y in corner_lo.y..=corner_hi.y {
                        for z in corner_lo.z..=corner_hi.z {
                            let mut c = IVec3::new(x, y, z);
                            c[k] = layer;
                            if grid.get_cell(c) == rbx_dom::rbx_terrain::legacy::EMPTY_CELL {
                                continue;
                            }
                            let center = rbx_dom::rbx_terrain::legacy::cell_center(c);
                            let cell_plane =
                                center[k] - face.normal[k].signum() * TERRAIN_CELL * 0.5;
                            if (cell_plane - plane).abs() > PLANAR_MAX {
                                continue;
                            }
                            let half = Vec3::splat(TERRAIN_CELL * 0.5);
                            let mut square = Vec::new();
                            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
                            for (sa, sb) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                                let mut p = center;
                                p[k] = cell_plane;
                                p[a] += sa * half[a];
                                p[b] += sb * half[b];
                                square.push(p);
                            }
                            if overlap_area(&face, &square) > OVERLAP_AREA_MIN
                                && !cells.iter().any(|(cc, _)| *cc == c)
                            {
                                cells.push((c, face.surface));
                            }
                        }
                    }
                }
            }
            for (_, surface) in cells {
                if part.surfaces[surface] != NO_JOIN {
                    out.push(weld(surface));
                }
            }
        } else if let Some(grid) = data.smooth.as_ref().filter(|g| g.is_allocated()) {
            let lo = rbx_dom::rbx_terrain::legacy::world_to_cell_floor(
                crate::math::from_parry_vec(part.aabb.mins) - Vec3::splat(SEARCH_MARGIN),
            );
            let hi = rbx_dom::rbx_terrain::legacy::world_to_cell_floor(
                crate::math::from_parry_vec(part.aabb.maxs) + Vec3::splat(SEARCH_MARGIN),
            );
            let mut touching_terrain = false;
            'outer: for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        if grid.get_cell(x, y, z).occupancy() > 0 {
                            touching_terrain = true;
                            break 'outer;
                        }
                    }
                }
            }
            if touching_terrain {
                out.push(weld(4));
            }
        }
        out
    }
}
