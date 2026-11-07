use crate::math::to_parry_vec;
use parry3d::shape::SharedShape;
use rbx_math::{CFrame, Mat3, Vec3};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    Compound,
}

#[derive(Clone)]
pub struct PartShape {
    pub kind: ShapeKind,
    pub size: Vec3,
    pub collider: SharedShape,
    pub local: CFrame,
    pub volume: f32,
    pub com: Vec3,
    pub unit_inertia: Mat3,
}

fn wedge_points(size: Vec3) -> Vec<Vec3> {
    let h = size * 0.5;
    vec![
        Vec3::new(h.x, h.y, h.z),
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(-h.x, h.y, h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(-h.x, -h.y, -h.z),
    ]
}

fn corner_wedge_points(size: Vec3) -> Vec<Vec3> {
    let h = size * 0.5;
    vec![
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(h.x, h.y, -h.z),
    ]
}

fn tetra_inertia(points: &[Vec3], faces: &[[usize; 3]]) -> (f32, Vec3, Mat3) {
    let mut volume = 0.0f32;
    let mut com = Vec3::ZERO;
    let mut cov = Mat3::ZERO;
    let canon = Mat3::from_cols(
        Vec3::new(1.0 / 60.0, 1.0 / 120.0, 1.0 / 120.0),
        Vec3::new(1.0 / 120.0, 1.0 / 60.0, 1.0 / 120.0),
        Vec3::new(1.0 / 120.0, 1.0 / 120.0, 1.0 / 60.0),
    );
    for f in faces {
        let a = points[f[0]];
        let b = points[f[1]];
        let c = points[f[2]];
        let m = Mat3::from_cols(a, b, c);
        let det = m.determinant();
        volume += det / 6.0;
        com += (a + b + c) * (det / 24.0);
        cov += m * canon * m.transpose() * det;
    }
    if volume.abs() < 1e-12 {
        return (0.0, Vec3::ZERO, Mat3::ZERO);
    }
    let com = com / volume;
    let trace = cov.x_axis.x + cov.y_axis.y + cov.z_axis.z;
    let inertia_origin = Mat3::from_diagonal(Vec3::splat(trace)) - cov;
    let shift = Mat3::from_diagonal(Vec3::splat(com.length_squared())) - outer(com, com);
    let inertia_com = inertia_origin - shift * volume;
    (volume, com, inertia_com * (1.0 / volume))
}

fn outer(a: Vec3, b: Vec3) -> Mat3 {
    Mat3::from_cols(a * b.x, a * b.y, a * b.z)
}

const WEDGE_FACES: [[usize; 3]; 8] = [
    [0, 3, 4],
    [0, 4, 1],
    [3, 0, 2],
    [3, 2, 5],
    [0, 1, 2],
    [3, 5, 4],
    [5, 2, 1],
    [5, 1, 4],
];

const CORNER_FACES: [[usize; 3]; 6] = [
    [0, 1, 4],
    [0, 4, 3],
    [2, 3, 4],
    [1, 0, 3],
    [1, 3, 2],
    [1, 2, 4],
];

impl PartShape {
    pub fn new(kind: ShapeKind, size: Vec3) -> PartShape {
        let size = size.max(Vec3::splat(0.01));
        let h = size * 0.5;
        match kind {
            ShapeKind::Block => PartShape {
                kind,
                size,
                collider: SharedShape::cuboid(h.x, h.y, h.z),
                local: CFrame::default(),
                volume: size.x * size.y * size.z,
                com: Vec3::ZERO,
                unit_inertia: Mat3::from_diagonal(Vec3::new(
                    (size.y * size.y + size.z * size.z) / 12.0,
                    (size.x * size.x + size.z * size.z) / 12.0,
                    (size.x * size.x + size.y * size.y) / 12.0,
                )),
            },
            ShapeKind::Ball => {
                let r = h.x;
                PartShape {
                    kind,
                    size,
                    collider: SharedShape::ball(r),
                    local: CFrame::default(),
                    volume: 4.0 / 3.0 * PI * r * r * r,
                    com: Vec3::ZERO,
                    unit_inertia: Mat3::from_diagonal(Vec3::splat(0.4 * r * r)),
                }
            }
            ShapeKind::Cylinder => {
                let r = h.y.min(h.z);
                let len = size.x;
                PartShape {
                    kind,
                    size,
                    collider: SharedShape::cylinder(h.x, r),
                    local: CFrame::new(
                        Mat3::from_rotation_z(-std::f32::consts::FRAC_PI_2),
                        Vec3::ZERO,
                    ),
                    volume: PI * r * r * len,
                    com: Vec3::ZERO,
                    unit_inertia: Mat3::from_diagonal(Vec3::new(
                        0.5 * r * r,
                        (3.0 * r * r + len * len) / 12.0,
                        (3.0 * r * r + len * len) / 12.0,
                    )),
                }
            }
            ShapeKind::Compound => {
                PartShape::hollow_box(kind, size, SharedShape::cuboid(h.x, h.y, h.z))
            }
            ShapeKind::Wedge | ShapeKind::CornerWedge => {
                let (points, faces): (Vec<Vec3>, &[[usize; 3]]) = if kind == ShapeKind::Wedge {
                    (wedge_points(size), &WEDGE_FACES)
                } else {
                    (corner_wedge_points(size), &CORNER_FACES)
                };
                let (volume, com, unit_inertia) = tetra_inertia(&points, faces);
                let pts: Vec<_> = points.iter().map(|p| to_parry_vec(*p)).collect();
                let collider = SharedShape::convex_hull(&pts)
                    .unwrap_or_else(|| SharedShape::cuboid(h.x, h.y, h.z));
                PartShape {
                    kind,
                    size,
                    collider,
                    local: CFrame::default(),
                    volume: volume.abs(),
                    com,
                    unit_inertia,
                }
            }
        }
    }

    pub fn hollow_box(kind: ShapeKind, size: Vec3, collider: SharedShape) -> PartShape {
        let size = size.max(Vec3::splat(0.01));
        let area = 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x);
        let mut moment = [0.0f32; 3];
        for (i, m) in moment.iter_mut().enumerate() {
            let x = size[i];
            let y = size[(i + 1) % 3];
            let z = size[(i + 2) % 3];
            *m = (1.0 / (2.0 * area))
                * (y * y * y * z / 3.0
                    + y * z * z * z / 3.0
                    + x * y * z * z
                    + x * y * y * y / 3.0
                    + x * y * y * z
                    + x * z * z * z / 3.0);
        }
        PartShape {
            kind,
            size,
            collider,
            local: CFrame::default(),
            volume: size.x * size.y * size.z,
            com: Vec3::ZERO,
            unit_inertia: Mat3::from_diagonal(Vec3::from_array(moment)),
        }
    }

    pub fn extents_radius(&self) -> f32 {
        (self.size * 0.5).length()
    }
}

pub fn shift_inertia(inertia: Mat3, mass: f32, offset: Vec3) -> Mat3 {
    inertia
        + (Mat3::from_diagonal(Vec3::splat(offset.length_squared())) - outer(offset, offset)) * mass
}

pub fn rotate_inertia(inertia: Mat3, rotation: Mat3) -> Mat3 {
    rotation * inertia * rotation.transpose()
}
