use parry3d::math::{Pose3, Rot3, Vec3 as PVec3};
use rbx_math::{CFrame, Mat3, Quat, Vec3, quat_from_matrix};

pub fn orthonormal_basis(w: Vec3) -> (Vec3, Vec3) {
    let w = w.normalize_or(Vec3::X);
    let u = if w.x.abs() >= w.y.abs() && w.x.abs() >= w.z.abs() {
        Vec3::new(-w.y, w.x, 0.0)
    } else {
        Vec3::new(0.0, w.z, -w.y)
    };
    let u = u.normalize_or(Vec3::Y);
    (u, w.cross(u))
}

pub fn fill_rotation(v: Vec3) -> Mat3 {
    let angle = v.length();
    if angle < 1e-12 {
        return Mat3::IDENTITY;
    }
    Mat3::from_axis_angle(v / angle, angle)
}

pub fn rotate_about_z(m: Mat3, angle: f32) -> Mat3 {
    m * Mat3::from_rotation_z(angle)
}

pub fn to_parry_vec(v: Vec3) -> PVec3 {
    PVec3::new(v.x, v.y, v.z)
}

pub fn from_parry_vec(v: PVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn to_parry_quat(q: Quat) -> Rot3 {
    Rot3::from_xyzw(q.x, q.y, q.z, q.w).normalize()
}

pub fn to_pose(cf: &CFrame) -> Pose3 {
    let q = quat_from_matrix(&cf.rotation);
    Pose3::from_parts(to_parry_vec(cf.translation), to_parry_quat(q))
}

pub fn cframe_mul_pose(cf: &CFrame, local: &CFrame) -> Pose3 {
    to_pose(&(*cf * *local))
}

pub fn sym_inverse_inertia_world(rotation: &Mat3, inv_local: &Mat3) -> Mat3 {
    *rotation * *inv_local * rotation.transpose()
}

pub fn safe_inverse(m: &Mat3) -> Mat3 {
    let det = m.determinant();
    if det.abs() < 1e-20 || !det.is_finite() {
        Mat3::ZERO
    } else {
        m.inverse()
    }
}
