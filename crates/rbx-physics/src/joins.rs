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
        let out: Vec<InstanceId> = self
            .part_joints
            .get(&id)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|j| self.joints.get(j).is_some_and(|jj| jj.in_world))
                    .collect()
            })
            .unwrap_or_default();
        out
    }

    pub fn destroy_joints(&mut self, dm: &mut DataModel, id: InstanceId) {
        self.sync(dm);
        for j in self.joints_of(id) {
            if dm.contains(j) {
                let _ = dm.destroy(j);
            }
            self.remove_joint_record(j);
            self.topology_dirty = true;
        }
    }

    fn joined(&self, a: InstanceId, b: InstanceId) -> bool {
        self.part_joints.get(&a).is_some_and(|v| {
            v.iter().any(|jid| {
                self.joints.get(jid).is_some_and(|j| {
                    j.in_world
                        && ((j.part0 == Some(a) && j.part1 == Some(b))
                            || (j.part0 == Some(b) && j.part1 == Some(a)))
                })
            })
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
        let query = parry3d::bounding_volume::Aabb::new(
            crate::math::to_parry_vec(lo),
            crate::math::to_parry_vec(hi),
        );
        let mut candidates: Vec<usize> = self
            .candidates(&query)
            .into_iter()
            .filter(|&j| j != pi)
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

    fn group_intersecting(&self, group: &[usize], delta: Vec3, tolerance: f32) -> bool {
        let members: std::collections::HashSet<usize> = group.iter().copied().collect();
        for &pi in group {
            let p = &self.parts[pi];
            let mut cf = p.cframe;
            cf.translation += delta;
            let pose = to_pose(&(cf * p.shape.local));
            let aabb = p.shape.collider.compute_aabb(&pose);
            if aabb.mins.y < MAX_DRAG_DEPTH {
                return true;
            }
            for j in self.candidates(&aabb) {
                if members.contains(&j) {
                    continue;
                }
                let o = &self.parts[j];
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
                ) && c.dist < -tolerance
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn intersects_others(&self, id: InstanceId, tolerance: f32) -> bool {
        match self.index.get(&id) {
            Some(&pi) => self.group_intersecting(&[pi], Vec3::ZERO, tolerance),
            None => false,
        }
    }

    fn search_up(&self, group: &[usize], start: Vec3) -> Option<Vec3> {
        if !self.group_intersecting(group, start, MAX_OVERLAP_OR_GAP) {
            return Some(start);
        }
        let mut moved = 0.0f32;
        let mut free = false;
        for _ in 0..10000 {
            if moved > 16000.0 {
                break;
            }
            moved += 1.2;
            if !self.group_intersecting(
                group,
                start + Vec3::new(0.0, moved, 0.0),
                MAX_OVERLAP_OR_GAP,
            ) {
                free = true;
                break;
            }
        }
        if !free {
            return None;
        }
        let mut lo = moved - 1.2;
        let mut hi = moved;
        while hi - lo > MAIN_GRID + 1e-4 {
            let mid = to_grid(Vec3::splat(lo + (hi - lo) * 0.51), MAIN_GRID).x;
            if (mid - lo).abs() < 1e-4 || (mid - hi).abs() < 1e-4 {
                break;
            }
            if self.group_intersecting(group, start + Vec3::new(0.0, mid, 0.0), MAX_OVERLAP_OR_GAP)
            {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(start + Vec3::new(0.0, hi, 0.0))
    }

    fn apply_group_delta(&mut self, dm: &mut DataModel, group: &[usize], delta: Vec3) {
        if delta == Vec3::ZERO {
            return;
        }
        for &pi in group {
            let mut cf = self.parts[pi].cframe;
            cf.translation += delta;
            self.parts[pi].cframe = cf;
            self.spatial_dirty.push(self.parts[pi].id);
            let id = self.parts[pi].id;
            dm.set_unchecked(id, "CFrame", Variant::CFrame(cf));
            dm.notify_changed(id, "Position");
        }
        self.topology_dirty = true;
        self.flush_spatial();
    }

    pub fn safe_move(&mut self, dm: &mut DataModel, id: InstanceId) {
        self.sync(dm);
        let Some(&pi) = self.index.get(&id) else {
            return;
        };
        if let Some(delta) = self.search_up(&[pi], Vec3::ZERO) {
            self.apply_group_delta(dm, &[pi], delta);
        }
    }

    pub fn move_group(&mut self, dm: &mut DataModel, ids: &[InstanceId], delta: Vec3, join: bool) {
        self.sync(dm);
        let group_ids: std::collections::HashSet<InstanceId> = ids.iter().copied().collect();
        for &id in ids {
            for j in self.joints_of(id) {
                let other = self.joints.get(&j).and_then(|jj| {
                    if jj.part0 == Some(id) {
                        jj.part1
                    } else {
                        jj.part0
                    }
                });
                if other.is_some_and(|o| !group_ids.contains(&o)) {
                    if dm.contains(j) {
                        let _ = dm.destroy(j);
                    }
                    self.remove_joint_record(j);
                    self.topology_dirty = true;
                }
            }
        }
        let group: Vec<usize> = ids
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        if group.is_empty() {
            return;
        }
        let final_delta = self.search_up(&group, delta).unwrap_or(delta);
        self.apply_group_delta(dm, &group, final_delta);
        if join {
            for &id in ids {
                self.join_filtered(dm, id, |o| !group_ids.contains(&o));
            }
        }
    }

    pub fn resize_part(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        face: NormalId,
        amount: i32,
        clamp: impl Fn(&DataModel, Vec3) -> Vec3,
    ) -> bool {
        self.destroy_joints(dm, id);
        let result = self.resize_impl(dm, id, face, amount, clamp);
        self.join(dm, id);
        result
    }

    fn resize_impl(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        face: NormalId,
        amount: i32,
        clamp: impl Fn(&DataModel, Vec3) -> Vec3,
    ) -> bool {
        if amount == 0 {
            return false;
        }
        let original_size = crate::world::part_size(dm, id);
        let original_cf = dm
            .get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
        let three_d = !(dm.class_of(id) == Some("Part")
            && dm.get_raw(id, "shape").and_then(|v| v.as_enum()) == Some(0));
        let mut delta = Vec3::ZERO;
        if three_d {
            delta[face as usize % 3] = amount as f32;
        } else {
            delta = Vec3::splat(amount as f32);
        }
        let pos_neg = if (face as usize) < 3 { 1.0 } else { -1.0 };
        let new_size = clamp(dm, original_size + delta);
        let delta_xml = new_size - original_size;
        let delta_world = original_cf.rotation * delta_xml;
        let new_cf = CFrame::new(
            original_cf.rotation,
            original_cf.translation + delta_world * 0.5 * pos_neg,
        );
        dm.set_unchecked(id, "CFrame", Variant::CFrame(new_cf));
        dm.set_unchecked(id, "size", Variant::Vector3(new_size));
        self.upsert_part(dm, id);
        if self.intersects_others(id, 3.0 * MAX_OVERLAP_OR_GAP) {
            dm.set_unchecked(id, "CFrame", Variant::CFrame(original_cf));
            dm.set_unchecked(id, "size", Variant::Vector3(original_size));
            self.upsert_part(dm, id);
            return false;
        }
        true
    }
}
