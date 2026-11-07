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

fn outer(a: Vec3, b: Vec3) -> Mat3 {
    Mat3::from_cols(a * b.x, a * b.y, a * b.z)
}

impl PartShape {
    pub fn new(kind: ShapeKind, size: Vec3) -> PartShape {
        let size = size.max(Vec3::splat(0.01));
        let h = size * 0.5;
        match kind {
            ShapeKind::Block => {
                PartShape::hollow_box(kind, size, SharedShape::cuboid(h.x, h.y, h.z))
            }
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
                let (points, com) = if kind == ShapeKind::Wedge {
                    (
                        wedge_points(size),
                        Vec3::new(0.0, -size.y / 6.0, size.z / 6.0),
                    )
                } else {
                    (
                        corner_wedge_points(size),
                        Vec3::new(size.x / 8.0, -size.y / 4.0, -size.z / 8.0),
                    )
                };
                let pts: Vec<_> = points.iter().map(|p| to_parry_vec(*p)).collect();
                let collider = SharedShape::convex_hull(&pts)
                    .unwrap_or_else(|| SharedShape::cuboid(h.x, h.y, h.z));
                PartShape {
                    com,
                    ..PartShape::hollow_box(kind, size, collider)
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
