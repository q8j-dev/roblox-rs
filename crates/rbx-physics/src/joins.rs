use crate::math::to_pose;
use crate::world::{Part, PhysicsWorld};
use parry3d::query::contact;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, NormalId, Vec3, normal_id_to_vector, vector_to_normal_id};

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

const JOINT_MAX_UNALIGNED: f32 = 0.05;
const JOINT_OVERLAP_MIN: f32 = 0.35;
const JOINT_OVERLAP_MIN2: f32 = 0.1;
const JOINT_ANGLE_MAX: f32 = 0.01;
const JOINT_PLANAR_MAX: f32 = 0.01;
const ROTATE_ANGLE_MAX: f32 = JOINT_MAX_UNALIGNED * 0.5;
const ROTATE_PLANAR_MAX: f32 = JOINT_MAX_UNALIGNED;
const GLUE_ANGLE_MAX: f32 = JOINT_MAX_UNALIGNED;
const GLUE_PLANAR_MAX: f32 = JOINT_MAX_UNALIGNED;
const MAIN_GRID: f32 = 0.1;
const MAX_OVERLAP_OR_GAP: f32 = 0.01;
const MAX_DRAG_DEPTH: f32 = -400.0;

#[derive(Clone, Copy, Debug)]
struct Face {
    center: Vec3,
    normal: Vec3,
    u: Vec3,
    v: Vec3,
    hu: f32,
    hv: f32,
}

impl Face {
    fn corners(&self) -> [Vec3; 4] {
        [
            self.center + self.u * self.hu + self.v * self.hv,
            self.center + self.u * self.hu - self.v * self.hv,
            self.center - self.u * self.hu - self.v * self.hv,
            self.center - self.u * self.hu + self.v * self.hv,
        ]
    }

    fn interval(&self, other: &Face, axis: Vec3) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for c in other.corners() {
            let d = (c - self.center).dot(axis);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        (lo, hi)
    }

    fn overlap_rect(&self, other: &Face) -> Option<(f32, f32, f32, f32)> {
        let (ul, uh) = self.interval(other, self.u);
        let (vl, vh) = self.interval(other, self.v);
        let u0 = ul.max(-self.hu);
        let u1 = uh.min(self.hu);
        let v0 = vl.max(-self.hv);
        let v1 = vh.min(self.hv);
        if u1 > u0 && v1 > v0 {
            Some((u0, u1, v0, v1))
        } else {
            None
        }
    }

    fn has_overlap(&self, other: &Face, min: f32) -> bool {
        match self.overlap_rect(other) {
            Some((u0, u1, v0, v1)) => (u1 - u0) > min && (v1 - v0) > min,
            None => false,
        }
    }

    fn within_planes(&self, other: &Face, planar: f32) -> bool {
        (other.center - self.center).dot(self.normal).abs() <= planar
            && (self.center - other.center).dot(other.normal).abs() <= planar
    }

    fn fuzzy_contains_in_extrusion(&self, p: Vec3, tol: f32) -> bool {
        let d = p - self.center;
        d.dot(self.u).abs() <= self.hu + tol && d.dot(self.v).abs() <= self.hv + tol
    }
}

pub fn normal_id_to_matrix(id: NormalId) -> Mat3 {
    let uvw = |v: Vec3| -> Vec3 {
        match id {
            NormalId::Right => Vec3::new(v.z, v.y, -v.x),
            NormalId::Top => Vec3::new(-v.x, v.z, v.y),
            NormalId::Back => v,
            NormalId::Left => Vec3::new(-v.z, v.y, v.x),
            NormalId::Bottom => Vec3::new(v.x, -v.z, v.y),
            NormalId::Front => Vec3::new(-v.x, v.y, -v.z),
        }
    };
    Mat3::from_cols(uvw(Vec3::X), uvw(Vec3::Y), uvw(Vec3::Z))
}

fn face_in_world(part: &Part, id: NormalId) -> Face {
    let k = id as usize % 3;
    let size = part.shape.size;
    let n_local = normal_id_to_vector(id);
    let mut u_local = Vec3::ZERO;
    let mut v_local = Vec3::ZERO;
    u_local[(k + 1) % 3] = 1.0;
    v_local[(k + 2) % 3] = 1.0;
    let cf = part.cframe;
    Face {
        center: cf * (n_local * size[k] * 0.5),
        normal: cf.rotation * n_local,
        u: cf.rotation * u_local,
        v: cf.rotation * v_local,
        hu: size[(k + 1) % 3] * 0.5,
        hv: size[(k + 2) % 3] * 0.5,
    }
}

fn face_coord_in_object(part: &Part, id: NormalId) -> CFrame {
    let k = id as usize % 3;
    CFrame::new(
        normal_id_to_matrix(id),
        normal_id_to_vector(id) * part.shape.size[k] * 0.5,
    )
}

fn fuzzy_axis_aligned(a: &Mat3, b: &Mat3, angle: f32) -> bool {
    let c = angle.cos();
    [a.x_axis, a.y_axis, a.z_axis].iter().all(|ax| {
        [b.x_axis, b.y_axis, b.z_axis]
            .iter()
            .any(|bx| ax.dot(*bx).abs() >= c)
    })
}

fn closest_object_normal(dir: Vec3, rotation: &Mat3) -> NormalId {
    vector_to_normal_id(rotation.transpose() * dir)
}

fn is_rotate(t: u32) -> bool {
    matches!(t, ROTATE | ROTATE_V | ROTATE_P)
}

fn can_build(p0: &Part, p1: &Part, n0: NormalId, n1: NormalId, angle: f32, planar: f32) -> bool {
    if !fuzzy_axis_aligned(&p0.cframe.rotation, &p1.cframe.rotation, angle) {
        return false;
    }
    let f0 = face_in_world(p0, n0);
    let f1 = face_in_world(p1, n1);
    f0.has_overlap(&f1, JOINT_OVERLAP_MIN2) && f0.within_planes(&f1, planar)
}

fn to_grid(v: Vec3, grid: f32) -> Vec3 {
    (v / grid).round() * grid
}

pub struct JointSpec {
    pub class: &'static str,
    pub part0: InstanceId,
    pub part1: InstanceId,
    pub c0: CFrame,
    pub c1: CFrame,
    pub base_angle: f32,
    pub glue: Option<[Vec3; 4]>,
}

fn rigid_coords(p0: &Part, p1: &Part, n0: NormalId) -> (CFrame, CFrame) {
    let c0 = face_coord_in_object(p0, n0);
    let world = p0.cframe * c0;
    let c1 = p1.cframe.inverse() * world;
    (c0, c1)
}

fn try_rotate(p0: &Part, p1: &Part, n0: NormalId, n1: NormalId) -> Option<JointSpec> {
    let mut t0 = p0.surfaces[n0 as usize];
    let mut t1 = p1.surfaces[n1 as usize];
    if !(is_rotate(t0) || is_rotate(t1)) {
        return None;
    }
    let w0 = p0.cframe.rotation * normal_id_to_vector(n0);
    let w1 = p1.cframe.rotation * normal_id_to_vector(n1);
    if w0.angle_between(-w1) > ROTATE_ANGLE_MAX {
        return None;
    }
    let f0 = face_in_world(p0, n0);
    let f1 = face_in_world(p1, n1);
    if !f0.has_overlap(&f1, JOINT_OVERLAP_MIN) || !f0.within_planes(&f1, ROTATE_PLANAR_MAX) {
        return None;
    }
    let touch0 = f1.fuzzy_contains_in_extrusion(f0.center, JOINT_MAX_UNALIGNED);
    let touch1 = f0.fuzzy_contains_in_extrusion(f1.center, JOINT_MAX_UNALIGNED);
    let k0 = is_rotate(t0) && touch0;
    let k1 = is_rotate(t1) && touch1;
    if !(k0 || k1) {
        return None;
    }
    let (mut a, mut b, mut na, mut nb) = (p0, p1, n0, n1);
    let (mut wa, mut wb) = (w0, w1);
    if !k0 || (k1 && p1.shape.size.element_sum() > p0.shape.size.element_sum()) {
        std::mem::swap(&mut t0, &mut t1);
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut na, &mut nb);
        std::mem::swap(&mut wa, &mut wb);
    }
    let axle_in_a = face_coord_in_object(a, na);
    let axle_world = a.cframe * axle_in_a.translation;
    let axle_in_b = b.cframe.point_to_object_space(axle_world);
    let into_b = nb.opposite();
    let aligned = |hole: &CFrame| {
        let hole_world = b.cframe * hole.translation;
        if (axle_world - hole_world).length_squared() > JOINT_MAX_UNALIGNED * JOINT_MAX_UNALIGNED {
            return false;
        }
        for pol in [1.0f32, -1.0] {
            let r0 = axle_world - wa * pol;
            let r1 = hole_world + wb * pol;
            if (r0 - r1).length_squared() > JOINT_MAX_UNALIGNED * JOINT_MAX_UNALIGNED {
                return false;
            }
        }
        true
    };
    let mut hole = CFrame::new(normal_id_to_matrix(into_b), to_grid(axle_in_b, MAIN_GRID));
    if !aligned(&hole) {
        hole = CFrame::new(normal_id_to_matrix(into_b), axle_in_b);
        if !aligned(&hole) {
            return None;
        }
    }
    let class = match t0 {
        ROTATE => "Rotate",
        ROTATE_P => "RotateP",
        _ => "RotateV",
    };
    let base_angle = if class == "Rotate" {
        0.0
    } else {
        let ca = a.cframe * axle_in_a;
        let cb = b.cframe * hole;
        let z = ca.rotation.z_axis;
        let xa = ca.rotation.x_axis;
        let xb = cb.rotation.x_axis;
        z.dot(xb.cross(xa)).atan2(xb.dot(xa))
    };
    Some(JointSpec {
        class,
        part0: a.id,
        part1: b.id,
        c0: axle_in_a,
        c1: hole,
        base_angle,
        glue: None,
    })
}

fn try_weld(p0: &Part, p1: &Part, n0: NormalId, n1: NormalId) -> Option<JointSpec> {
    let t0 = p0.surfaces[n0 as usize];
    let t1 = p1.surfaces[n1 as usize];
    if !(t0 == WELD || t1 == WELD) || !can_build(p0, p1, n0, n1, JOINT_ANGLE_MAX, JOINT_PLANAR_MAX)
    {
        return None;
    }
    let (c0, c1) = rigid_coords(p0, p1, n0);
    Some(JointSpec {
        class: "Weld",
        part0: p0.id,
        part1: p1.id,
        c0,
        c1,
        base_angle: 0.0,
        glue: None,
    })
}

fn try_snap(p0: &Part, p1: &Part, n0: NormalId, n1: NormalId) -> Option<JointSpec> {
    let t0 = p0.surfaces[n0 as usize];
    let t1 = p1.surfaces[n1 as usize];
    let ok = (t0 == STUDS && t1 == INLET)
        || (t0 == INLET && t1 == STUDS)
        || (t0 == UNIVERSAL && t1 == UNIVERSAL)
        || (t0 == UNIVERSAL && (t1 == INLET || t1 == STUDS))
        || ((t0 == INLET || t0 == STUDS) && t1 == UNIVERSAL);
    if !ok || !can_build(p0, p1, n0, n1, JOINT_ANGLE_MAX, JOINT_PLANAR_MAX) {
        return None;
    }
    let (c0, c1) = rigid_coords(p0, p1, n0);
    Some(JointSpec {
        class: "Snap",
        part0: p0.id,
        part1: p1.id,
        c0,
        c1,
        base_angle: 0.0,
        glue: None,
    })
}

fn try_glue(p0: &Part, p1: &Part, n0: NormalId, n1: NormalId) -> Option<JointSpec> {
    let t0 = p0.surfaces[n0 as usize];
    let t1 = p1.surfaces[n1 as usize];
    if !(t0 == GLUE || t1 == GLUE) || !can_build(p0, p1, n0, n1, GLUE_ANGLE_MAX, GLUE_PLANAR_MAX) {
        return None;
    }
    let f0 = face_in_world(p0, n0);
    let f1 = face_in_world(p1, n1);
    let (u0, u1, v0, v1) = f0.overlap_rect(&f1)?;
    let c0 = face_coord_in_object(p0, n0);
    let jc0_world = p0.cframe * c0;
    let c1 = p1.cframe.inverse() * jc0_world;
    let c1 = CFrame::new(c1.rotation, to_grid(c1.translation, MAIN_GRID));
    let corners_world = [
        f0.center + f0.u * u1 + f0.v * v1,
        f0.center + f0.u * u1 + f0.v * v0,
        f0.center + f0.u * u0 + f0.v * v0,
        f0.center + f0.u * u0 + f0.v * v1,
    ];
    let glue = corners_world.map(|c| jc0_world.point_to_object_space(c));
    Some(JointSpec {
        class: "Glue",
        part0: p0.id,
        part1: p1.id,
        c0,
        c1,
        base_angle: 0.0,
        glue: Some(glue),
    })
}

pub fn can_join(p0: &Part, p1: &Part) -> Option<JointSpec> {
    let r1 = p1.cframe.rotation;
    for i in 0..6u8 {
        let n0 = NormalId::from_index(i)?;
        let w0 = p0.cframe.rotation * normal_id_to_vector(n0);
        let n1 = closest_object_normal(-w0, &r1);
        if p0.surfaces[n0 as usize] == NO_JOIN || p1.surfaces[n1 as usize] == NO_JOIN {
            continue;
        }
        if p0.surfaces[n0 as usize] == SMOOTH && p1.surfaces[n1 as usize] == SMOOTH {
            continue;
        }
        if let Some(j) = try_rotate(p0, p1, n0, n1) {
            return Some(j);
        }
        if let Some(j) = try_weld(p0, p1, n0, n1) {
            return Some(j);
        }
        if let Some(j) = try_snap(p0, p1, n0, n1) {
            return Some(j);
        }
        if let Some(j) = try_glue(p0, p1, n0, n1) {
            return Some(j);
        }
    }
    None
}

impl PhysicsWorld {
    pub fn joints_of(&self, id: InstanceId) -> Vec<InstanceId> {
        let mut out: Vec<InstanceId> = self
            .joints
            .values()
            .filter(|j| j.in_world && (j.part0 == Some(id) || j.part1 == Some(id)))
            .map(|j| j.id)
            .collect();
        out.sort();
        out
    }

    pub fn destroy_joints(&mut self, dm: &mut DataModel, id: InstanceId) {
        self.sync(dm);
        for j in self.joints_of(id) {
            if dm.contains(j) {
                let _ = dm.destroy(j);
            }
            self.joints.remove(&j);
            self.topology_dirty = true;
        }
    }

    fn joined(&self, a: InstanceId, b: InstanceId) -> bool {
        self.joints.values().any(|j| {
            j.in_world
                && ((j.part0 == Some(a) && j.part1 == Some(b))
                    || (j.part0 == Some(b) && j.part1 == Some(a)))
        })
    }

    pub fn join(&mut self, dm: &mut DataModel, id: InstanceId) -> Vec<InstanceId> {
        self.join_filtered(dm, id, |_| true)
    }

    pub fn join_filtered(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        allow: impl Fn(InstanceId) -> bool,
    ) -> Vec<InstanceId> {
        self.sync(dm);
        let Some(&pi) = self.index.get(&id) else {
            return Vec::new();
        };
        let p = self.parts[pi].clone();
        let pose = to_pose(&(p.cframe * p.shape.local));
        let aabb = p.shape.collider.compute_aabb(&pose);
        let lo = crate::math::from_parry_vec(aabb.mins) - Vec3::splat(JOINT_MAX_UNALIGNED);
        let hi = crate::math::from_parry_vec(aabb.maxs) + Vec3::splat(JOINT_MAX_UNALIGNED);
        let mut candidates: Vec<usize> = (0..self.parts.len())
            .filter(|&j| {
                if j == pi {
                    return false;
                }
                let o = &self.parts[j];
                let oa = o
                    .shape
                    .collider
                    .compute_aabb(&to_pose(&(o.cframe * o.shape.local)));
                let omin = crate::math::from_parry_vec(oa.mins);
                let omax = crate::math::from_parry_vec(oa.maxs);
                omin.x <= hi.x
                    && omax.x >= lo.x
                    && omin.y <= hi.y
                    && omax.y >= lo.y
                    && omin.z <= hi.z
                    && omax.z >= lo.z
            })
            .collect();
        candidates.sort_by_key(|&j| self.parts[j].id);
        let Some(js) = self
            .joints_service
            .or_else(|| dm.get_service("JointsService").ok())
        else {
            return Vec::new();
        };
        self.joints_service = Some(js);
        let mut created = Vec::new();
        for j in candidates {
            let other = self.parts[j].clone();
            if !allow(other.id) || self.joined(id, other.id) {
                continue;
            }
            let Some(spec) = can_join(&p, &other) else {
                continue;
            };
            let Ok(inst) = dm.create(spec.class) else {
                continue;
            };
            dm.set_unchecked(inst, "Part0", Variant::Ref(Some(spec.part0)));
            dm.set_unchecked(inst, "Part1", Variant::Ref(Some(spec.part1)));
            dm.set_unchecked(inst, "C0", Variant::CFrame(spec.c0));
            dm.set_unchecked(inst, "C1", Variant::CFrame(spec.c1));
            if spec.class == "RotateP" || spec.class == "RotateV" {
                dm.set_unchecked(inst, "BaseAngle", Variant::Float(spec.base_angle));
            }
            if let Some(g) = spec.glue {
                for (k, name) in ["F0", "F1", "F2", "F3"].iter().enumerate() {
                    dm.set_unchecked(inst, name, Variant::Vector3(g[k]));
                }
            }
            if dm.set_parent(inst, Some(js)).is_ok() {
                if let Some(kind) = crate::world::JointKind::from_class(spec.class) {
                    self.upsert_joint(dm, inst, kind);
                }
                created.push(inst);
            }
        }
        created
    }

    fn intersecting(&self, pi: usize, cf: &CFrame) -> bool {
        let p = &self.parts[pi];
        let world_cf = *cf * p.shape.local;
        let pose = to_pose(&world_cf);
        let aabb = p.shape.collider.compute_aabb(&pose);
        if aabb.mins.y < MAX_DRAG_DEPTH {
            return true;
        }
        for (j, o) in self.parts.iter().enumerate() {
            if j == pi {
                continue;
            }
            let opose = to_pose(&(o.cframe * o.shape.local));
            let oa = o.shape.collider.compute_aabb(&opose);
            if oa.mins.x > aabb.maxs.x
                || oa.maxs.x < aabb.mins.x
                || oa.mins.y > aabb.maxs.y
                || oa.maxs.y < aabb.mins.y
                || oa.mins.z > aabb.maxs.z
                || oa.maxs.z < aabb.mins.z
            {
                continue;
            }
            if let Ok(Some(c)) = contact(
                &pose,
                p.shape.collider.as_ref(),
                &opose,
                o.shape.collider.as_ref(),
                0.0,
            ) && c.dist < -MAX_OVERLAP_OR_GAP
            {
                return true;
            }
        }
        false
    }

    pub fn safe_move(&mut self, dm: &mut DataModel, id: InstanceId) {
        self.sync(dm);
        let Some(&pi) = self.index.get(&id) else {
            return;
        };
        let start = self.parts[pi].cframe;
        if !self.intersecting(pi, &start) {
            return;
        }
        let mut cf = start;
        let mut moved = 0.0f32;
        let mut free = false;
        for _ in 0..10000 {
            if moved > 16000.0 {
                break;
            }
            cf.translation.y += 1.2;
            moved += 1.2;
            if !self.intersecting(pi, &cf) {
                free = true;
                break;
            }
        }
        if !free {
            return;
        }
        let mut lo = moved - 1.2;
        let mut hi = moved;
        while hi - lo > MAIN_GRID + 1e-4 {
            let mid = to_grid(Vec3::splat(lo + (hi - lo) * 0.51), MAIN_GRID).x;
            if (mid - lo).abs() < 1e-4 || (mid - hi).abs() < 1e-4 {
                break;
            }
            let mut test = start;
            test.translation.y += mid;
            if self.intersecting(pi, &test) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let mut final_cf = start;
        final_cf.translation.y += hi;
        self.parts[pi].cframe = final_cf;
        dm.set_unchecked(id, "CFrame", Variant::CFrame(final_cf));
        dm.notify_changed(id, "Position");
    }
}
