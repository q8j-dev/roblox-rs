use crate::shape::ShapeKind;
use crate::world::Part;
use rbx_math::{CFrame, Mat3, NormalId, Vec3};

const OVERLAP_SCALE: f32 = 0.99;
const EDGE_DISTANCE: f32 = 1e-5;

struct PolyMesh {
    verts: Vec<Vec3>,
    faces: Vec<Vec<usize>>,
}

impl PolyMesh {
    fn of(kind: ShapeKind, size: Vec3) -> PolyMesh {
        let (x, y, z) = (size.x * 0.5, size.y * 0.5, size.z * 0.5);
        let v = |a: f32, b: f32, c: f32| Vec3::new(a, b, c);
        match kind {
            ShapeKind::Wedge => PolyMesh {
                verts: vec![
                    v(x, y, z),
                    v(x, -y, z),
                    v(x, -y, -z),
                    v(-x, y, z),
                    v(-x, -y, z),
                    v(-x, -y, -z),
                ],
                faces: vec![
                    vec![0, 3, 4, 1],
                    vec![3, 0, 2, 5],
                    vec![0, 1, 2],
                    vec![3, 5, 4],
                    vec![5, 2, 1, 4],
                ],
            },
            ShapeKind::CornerWedge => PolyMesh {
                verts: vec![
                    v(x, -y, z),
                    v(x, -y, -z),
                    v(-x, -y, -z),
                    v(-x, -y, z),
                    v(x, y, -z),
                ],
                faces: vec![
                    vec![0, 1, 4],
                    vec![0, 4, 3],
                    vec![2, 3, 4],
                    vec![1, 0, 3, 2],
                    vec![1, 2, 4],
                ],
            },
            _ => PolyMesh {
                verts: vec![
                    v(x, y, z),
                    v(x, y, -z),
                    v(x, -y, z),
                    v(x, -y, -z),
                    v(-x, y, z),
                    v(-x, y, -z),
                    v(-x, -y, z),
                    v(-x, -y, -z),
                ],
                faces: vec![
                    vec![1, 0, 2, 3],
                    vec![1, 5, 4, 0],
                    vec![0, 4, 6, 2],
                    vec![4, 5, 7, 6],
                    vec![7, 3, 2, 6],
                    vec![5, 1, 3, 7],
                ],
            },
        }
    }

    fn vert(&self, face: usize, i: usize) -> Vec3 {
        self.verts[self.faces[face][i]]
    }

    fn plane(&self, face: usize) -> (Vec3, f32) {
        let a = self.vert(face, 0);
        let b = self.vert(face, 1);
        let c = self.vert(face, 2);
        let n = (b - a).cross(c - a).normalize_or_zero();
        (n, n.dot(a))
    }

    fn neighbor(&self, face: usize, edge: usize) -> Option<usize> {
        let f = &self.faces[face];
        let a = f[edge];
        let b = f[(edge + 1) % f.len()];
        (0..self.faces.len()).find(|&o| {
            o != face && {
                let g = &self.faces[o];
                (0..g.len()).any(|k| g[k] == b && g[(k + 1) % g.len()] == a)
            }
        })
    }

    fn point_in_extrusion(&self, point: Vec3, face: usize) -> bool {
        let (n, d) = self.plane(face);
        let on_plane = point - n * (n.dot(point) - d);
        (0..self.faces[face].len()).all(|e| match self.neighbor(face, e) {
            Some(o) => {
                let (on, od) = self.plane(o);
                on.dot(on_plane) <= od
            }
            None => true,
        })
    }

    fn surface_coord(&self, face: usize) -> CFrame {
        let (n, _) = self.plane(face);
        CFrame::new(well_formed_rotation(n), self.vert(face, 2))
    }
}

fn well_formed_rotation(z: Vec3) -> Mat3 {
    let project = |axis: Vec3| axis - z * axis.dot(z);
    let py = project(Vec3::Y);
    let y = if py.length() > 1e-6 {
        py.normalize()
    } else {
        project(Vec3::Z).normalize_or_zero()
    };
    Mat3::from_cols(y.cross(z), y, z)
}

pub(crate) fn is_non_orthogonal(kind: ShapeKind) -> bool {
    matches!(kind, ShapeKind::Wedge | ShapeKind::CornerWedge)
}

fn legacy_face(kind: ShapeKind, n: NormalId) -> Option<usize> {
    match kind {
        ShapeKind::Wedge => Some(match n {
            NormalId::Right => 2,
            NormalId::Left => 3,
            NormalId::Top => 1,
            NormalId::Bottom => 4,
            NormalId::Back => 0,
            NormalId::Front => 1,
        }),
        ShapeKind::CornerWedge => match n {
            NormalId::Right => Some(0),
            NormalId::Left => Some(2),
            NormalId::Top => None,
            NormalId::Bottom => Some(3),
            NormalId::Back => Some(1),
            NormalId::Front => Some(4),
        },
        _ => Some(n as usize),
    }
}

fn fuzzy_axis_aligned(a: &Mat3, b: &Mat3, angle: f32) -> bool {
    let c = angle.cos();
    [a.x_axis, a.y_axis, a.z_axis].iter().all(|ax| {
        [b.x_axis, b.y_axis, b.z_axis]
            .iter()
            .any(|bx| ax.dot(*bx).abs() >= c)
    })
}

fn segments_cross(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> Option<f32> {
    let tolerance = OVERLAP_SCALE - 1.0;
    let u1 = (a1 - a0).normalize_or_zero();
    let u2 = (b1 - b0).normalize_or_zero();
    let b = u1.dot(u2);
    if (b.abs() - 1.0).abs() < tolerance {
        return None;
    }
    let d = a0 - b0;
    let s1 = d.dot(u1);
    let s2 = d.dot(u2);
    let denom = 1.0 - b * b;
    if denom == 0.0 {
        return None;
    }
    let t1 = (b * s2 - s1 * b * b) / denom - s1;
    let t2 = (s2 - s1 * b) / denom;
    if t1 < -tolerance || t2 < -tolerance {
        return None;
    }
    if t1 > (a1 - a0).length() + tolerance || t2 > (b1 - b0).length() + tolerance {
        return None;
    }
    Some(((a0 + u1 * t1) - (b0 + u2 * t2)).length())
}

struct Side<'a> {
    part: &'a Part,
    mesh: PolyMesh,
    face: usize,
}

impl Side<'_> {
    fn world(&self, p: Vec3) -> Vec3 {
        self.part.cframe * p
    }

    fn local(&self, p: Vec3) -> Vec3 {
        self.part.cframe.point_to_object_space(p)
    }

    fn count(&self) -> usize {
        self.mesh.faces[self.face].len()
    }
}

fn vertices_overlap(a: &Side, b: &Side) -> bool {
    (0..a.count()).any(|i| {
        let p = b.local(a.world(a.mesh.vert(a.face, i) * OVERLAP_SCALE));
        b.mesh.point_in_extrusion(p, b.face)
    })
}

fn edges_overlap(a: &Side, b: &Side) -> bool {
    let na = a.count();
    let nb = b.count();
    for i in 0..na {
        let a0 = b.local(a.world(a.mesh.vert(a.face, i)));
        let a1 = b.local(a.world(a.mesh.vert(a.face, (i + 1) % na)));
        for j in 0..nb {
            let b0 = b.mesh.vert(b.face, j);
            let b1 = b.mesh.vert(b.face, (j + 1) % nb);
            if segments_cross(a0, a1, b0, b1).is_some_and(|d| d < EDGE_DISTANCE) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn can_build_poly(
    p0: &Part,
    p1: &Part,
    n0: NormalId,
    n1: NormalId,
    angle: f32,
    planar: f32,
) -> bool {
    let (Some(f0), Some(f1)) = (
        legacy_face(p0.shape.kind, n0),
        legacy_face(p1.shape.kind, n1),
    ) else {
        return false;
    };
    let s0 = Side {
        part: p0,
        mesh: PolyMesh::of(p0.shape.kind, p0.shape.size),
        face: f0,
    };
    let s1 = Side {
        part: p1,
        mesh: PolyMesh::of(p1.shape.kind, p1.shape.size),
        face: f1,
    };
    let c0 = p0.cframe * s0.mesh.surface_coord(f0);
    let c1 = p1.cframe * s1.mesh.surface_coord(f1);
    if !fuzzy_axis_aligned(&c0.rotation, &c1.rotation, angle) {
        return false;
    }
    let overlapped =
        vertices_overlap(&s0, &s1) || vertices_overlap(&s1, &s0) || edges_overlap(&s0, &s1);
    if !overlapped {
        return false;
    }
    c0.point_to_object_space(c1.translation).z.abs() <= 2.0 * planar
}
