use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum NormalId {
    Right = 0,
    Top = 1,
    Back = 2,
    Left = 3,
    Bottom = 4,
    Front = 5,
}

impl NormalId {
    pub const ALL: [NormalId; 6] = [
        NormalId::Right,
        NormalId::Top,
        NormalId::Back,
        NormalId::Left,
        NormalId::Bottom,
        NormalId::Front,
    ];

    pub fn from_index(index: u8) -> Option<NormalId> {
        NormalId::ALL.get(index as usize).copied()
    }

    pub fn opposite(self) -> NormalId {
        NormalId::ALL[(self as usize + 3) % 6]
    }
}

pub fn normal_id_to_vector(id: NormalId) -> Vec3 {
    match id {
        NormalId::Right => Vec3::X,
        NormalId::Top => Vec3::Y,
        NormalId::Back => Vec3::Z,
        NormalId::Left => Vec3::NEG_X,
        NormalId::Bottom => Vec3::NEG_Y,
        NormalId::Front => Vec3::NEG_Z,
    }
}

pub fn vector_to_normal_id(v: Vec3) -> NormalId {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z {
        if v.x >= 0.0 {
            NormalId::Right
        } else {
            NormalId::Left
        }
    } else if a.y >= a.z {
        if v.y >= 0.0 {
            NormalId::Top
        } else {
            NormalId::Bottom
        }
    } else if v.z >= 0.0 {
        NormalId::Back
    } else {
        NormalId::Front
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CFrame {
    pub rotation: Mat3,
    pub translation: Vec3,
}

impl Default for CFrame {
    fn default() -> Self {
        CFrame::IDENTITY
    }
}

impl CFrame {
    pub const IDENTITY: CFrame = CFrame {
        rotation: Mat3::IDENTITY,
        translation: Vec3::ZERO,
    };

    pub fn new(rotation: Mat3, translation: Vec3) -> CFrame {
        CFrame {
            rotation,
            translation,
        }
    }

    pub fn from_translation(translation: Vec3) -> CFrame {
        CFrame {
            rotation: Mat3::IDENTITY,
            translation,
        }
    }

    pub fn from_rows(rows: [f32; 9], translation: Vec3) -> CFrame {
        CFrame {
            rotation: Mat3::from_cols(
                Vec3::new(rows[0], rows[3], rows[6]),
                Vec3::new(rows[1], rows[4], rows[7]),
                Vec3::new(rows[2], rows[5], rows[8]),
            ),
            translation,
        }
    }

    pub fn rows(&self) -> [f32; 9] {
        let m = &self.rotation;
        [
            m.x_axis.x, m.y_axis.x, m.z_axis.x, m.x_axis.y, m.y_axis.y, m.z_axis.y, m.x_axis.z,
            m.y_axis.z, m.z_axis.z,
        ]
    }

    pub fn get(&self, row: usize, col: usize) -> f32 {
        self.rotation.col(col)[row]
    }

    pub fn from_quaternion(q: Quat, translation: Vec3) -> CFrame {
        CFrame {
            rotation: Mat3::from_quat(q.normalize()),
            translation,
        }
    }

    pub fn to_quaternion(&self) -> Quat {
        quat_from_matrix(&self.rotation)
    }

    pub fn from_axis_angle(axis: Vec3, angle: f32) -> CFrame {
        CFrame {
            rotation: Mat3::from_axis_angle(axis.normalize_or_zero(), angle),
            translation: Vec3::ZERO,
        }
    }

    pub fn from_euler_xyz(rx: f32, ry: f32, rz: f32) -> CFrame {
        let m = Mat3::from_rotation_x(rx) * Mat3::from_rotation_y(ry) * Mat3::from_rotation_z(rz);
        CFrame {
            rotation: m,
            translation: Vec3::ZERO,
        }
    }

    pub fn to_euler_xyz(&self) -> (f32, f32, f32) {
        let r = self.rows();
        let (r00, r01, r02, r11, r12, r21, r22) = (r[0], r[1], r[2], r[4], r[5], r[7], r[8]);
        let y = r02.clamp(-1.0, 1.0).asin();
        if r02.abs() < 0.999_999 {
            ((-r12).atan2(r22), y, (-r01).atan2(r00))
        } else {
            (r21.atan2(r11), y, 0.0)
        }
    }

    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> CFrame {
        let look = (target - eye).normalize_or(Vec3::NEG_Z);
        let mut up_v = up;
        if look.cross(up_v).length_squared() < 1e-8 {
            up_v = if look.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        }
        let z = -look;
        let x = up_v.cross(z).normalize();
        let y = z.cross(x);
        CFrame {
            rotation: Mat3::from_cols(x, y, z),
            translation: eye,
        }
    }

    pub fn inverse(&self) -> CFrame {
        let rt = self.rotation.transpose();
        CFrame {
            rotation: rt,
            translation: -(rt * self.translation),
        }
    }

    pub fn point_to_world_space(&self, p: Vec3) -> Vec3 {
        self.rotation * p + self.translation
    }

    pub fn point_to_object_space(&self, p: Vec3) -> Vec3 {
        self.rotation.transpose() * (p - self.translation)
    }

    pub fn vector_to_world_space(&self, v: Vec3) -> Vec3 {
        self.rotation * v
    }

    pub fn vector_to_object_space(&self, v: Vec3) -> Vec3 {
        self.rotation.transpose() * v
    }

    pub fn to_world_space(&self, other: &CFrame) -> CFrame {
        *self * *other
    }

    pub fn to_object_space(&self, other: &CFrame) -> CFrame {
        self.inverse() * *other
    }

    pub fn right_vector(&self) -> Vec3 {
        self.rotation.x_axis
    }

    pub fn up_vector(&self) -> Vec3 {
        self.rotation.y_axis
    }

    pub fn look_vector(&self) -> Vec3 {
        -self.rotation.z_axis
    }

    pub fn lerp(&self, goal: &CFrame, alpha: f32) -> CFrame {
        let a = self.to_quaternion();
        let b = goal.to_quaternion();
        CFrame {
            rotation: Mat3::from_quat(a.slerp(b, alpha).normalize()),
            translation: self.translation.lerp(goal.translation, alpha),
        }
    }

    pub fn is_axis_aligned(&self) -> bool {
        is_axis_aligned(&self.rotation)
    }

    pub fn orient_id(&self) -> Option<u8> {
        orient_id(&self.rotation)
    }

    pub fn from_orient_id(id: u8) -> Option<Mat3> {
        matrix_from_orient_id(id)
    }

    pub fn orthonormalized(&self) -> CFrame {
        CFrame {
            rotation: orthonormalize(&self.rotation),
            translation: self.translation,
        }
    }

    pub fn is_finite(&self) -> bool {
        self.rotation.is_finite() && self.translation.is_finite()
    }
}

impl Mul for CFrame {
    type Output = CFrame;

    fn mul(self, rhs: CFrame) -> CFrame {
        CFrame {
            rotation: self.rotation * rhs.rotation,
            translation: self.rotation * rhs.translation + self.translation,
        }
    }
}

impl Mul<Vec3> for CFrame {
    type Output = Vec3;

    fn mul(self, rhs: Vec3) -> Vec3 {
        self.point_to_world_space(rhs)
    }
}

pub fn is_axis_aligned(m: &Mat3) -> bool {
    let mut rows = [0; 3];
    let mut cols = [0; 3];
    for (col, col_count) in cols.iter_mut().enumerate() {
        let c = m.col(col);
        for (row, row_count) in rows.iter_mut().enumerate() {
            let t = c[row];
            if t == 1.0 || t == -1.0 {
                *row_count += 1;
                *col_count += 1;
            } else if t != 0.0 {
                return false;
            }
        }
    }
    rows == [1, 1, 1] && cols == [1, 1, 1]
}

pub fn orient_id(m: &Mat3) -> Option<u8> {
    if !is_axis_aligned(m) {
        return None;
    }
    let x = vector_to_normal_id(m.x_axis) as u8;
    let y = vector_to_normal_id(m.y_axis) as u8;
    Some(6 * x + y)
}

pub fn legal_orient_id(id: u8) -> bool {
    let x = (id / 6) % 3;
    let y = id % 3;
    id < 36 && x != y
}

pub fn matrix_from_orient_id(id: u8) -> Option<Mat3> {
    if !legal_orient_id(id) {
        return None;
    }
    let x = normal_id_to_vector(NormalId::from_index(id / 6)?);
    let y = normal_id_to_vector(NormalId::from_index(id % 6)?);
    Some(Mat3::from_cols(x, y, x.cross(y)))
}

pub fn orthonormalize(m: &Mat3) -> Mat3 {
    let x = m.x_axis.normalize_or(Vec3::X);
    let mut y = m.y_axis - x * x.dot(m.y_axis);
    y = y.normalize_or(x.any_orthonormal_vector());
    let mut z = m.z_axis - x * x.dot(m.z_axis) - y * y.dot(m.z_axis);
    z = z.normalize_or(x.cross(y));
    Mat3::from_cols(x, y, z)
}

pub fn quat_from_matrix(m: &Mat3) -> Quat {
    let r = |row: usize, col: usize| m.col(col)[row];
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        Quat::from_xyzw(
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            0.25 * s,
        )
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        Quat::from_xyzw(
            0.25 * s,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        )
    } else if r(1, 1) > r(2, 2) {
        let s = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        Quat::from_xyzw(
            (r(0, 1) + r(1, 0)) / s,
            0.25 * s,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        )
    } else {
        let s = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        Quat::from_xyzw(
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            0.25 * s,
            (r(1, 0) - r(0, 1)) / s,
        )
    };
    q.normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orient_ids_round_trip() {
        for id in 0..36u8 {
            if let Some(m) = matrix_from_orient_id(id) {
                assert!(is_axis_aligned(&m));
                assert_eq!(orient_id(&m), Some(id));
            }
        }
        assert_eq!(orient_id(&Mat3::IDENTITY), Some(1));
    }

    #[test]
    fn inverse_and_spaces() {
        let cf = CFrame::new(
            Mat3::from_axis_angle(Vec3::new(1.0, 2.0, 3.0).normalize(), 0.7),
            Vec3::new(4.0, -2.0, 9.0),
        );
        let p = Vec3::new(1.0, 2.0, 3.0);
        let back = cf.point_to_object_space(cf.point_to_world_space(p));
        assert!((back - p).length() < 1e-5);
        let id = cf * cf.inverse();
        assert!((id.translation).length() < 1e-5);
        let q = cf.to_quaternion();
        let m = Mat3::from_quat(q);
        assert!((m.x_axis - cf.rotation.x_axis).length() < 1e-5);
    }

    #[test]
    fn rows_round_trip() {
        let cf = CFrame::from_rows([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], Vec3::ONE);
        assert_eq!(cf.get(0, 1), -1.0);
        assert_eq!(cf.rows()[1], -1.0);
        assert_eq!(CFrame::from_rows(cf.rows(), cf.translation), cf);
    }
}
