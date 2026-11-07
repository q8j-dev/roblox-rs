use crate::broadphase::sweep_pairs;
use crate::math::to_pose;
use crate::solver::{BodyState, Constraint, ConstraintKind, ConstraintRef, SolverBody, WORLD_DT};
use crate::world::{
    ContactPair, GRAVITY, JointKind, MoverKind, PhysicalMode, PhysicsWorld, StepEvents,
    WORLD_STEPS_PER_LONG_STEP, WORLD_STEPS_PER_UI_STEP,
};
use parry3d::bounding_volume::{Aabb, BoundingVolume};
use parry3d::query::{DefaultQueryDispatcher, PersistentQueryDispatcher};
use rbx_dom::{DataModel, InstanceId};
use rbx_math::{CFrame, Mat3, NormalId, Vec3, normal_id_to_vector};
use std::collections::{HashMap, HashSet};

const PSCALE: f32 = 1.0 / 19.0;

type CollisionSpec = ((InstanceId, InstanceId), usize, usize, CFrame, CFrame);
const MAX_STEPS_PER_CALL: u32 = 240;

fn legacy_friction(c0: f32, c1: f32) -> f32 {
    let c0 = c0.clamp(0.0, 2.0);
    let c1 = c1.clamp(0.0, 2.0);
    if (c0 <= 1.0 && c1 <= 1.0) || (c0 >= 1.0 && c1 >= 1.0) {
        c0.min(c1)
    } else {
        c0 + c1 - 1.0
    }
}

fn weighted(wx: f32, x: f32, wy: f32, y: f32) -> f32 {
    let den = wx + wy;
    if den <= 0.0 {
        return (x + y) * 0.5;
    }
    (wx * x + wy * y) / den
}

fn clamp_vec(v: Vec3, m: Vec3) -> Vec3 {
    v.clamp(-m.abs(), m.abs())
}

fn different_5_percent(a: Vec3, b: Vec3) -> bool {
    let d = (a - b).abs();
    let tol = a.abs().max(b.abs()) * 0.05;
    d.x > tol.x.max(1e-6) || d.y > tol.y.max(1e-6) || d.z > tol.z.max(1e-6)
}

pub fn joint_k_multiplier(size: Vec3, ball: bool) -> f32 {
    let mut s = [size.x, size.y, size.z];
    s.sort_by(|a, b| a.total_cmp(b));
    let clipped = [s[0].max(1.0), s[1].max(1.0), s[2].max(1.0)];
    let (x, y, z) = (clipped[0] as i32, clipped[1] as i32, clipped[2] as i32);
    let zf = z as f32;
    let mut m = if ball {
        match x {
            1 => 0.23,
            2 => 1.49,
            3 => 4.43,
            4 => 11.50,
            _ => (x * x * x) as f32 * 0.175,
        }
    } else {
        match x {
            1 => match y {
                1 => match z {
                    1 => 0.91,
                    2 => 1.61,
                    3 => 2.0,
                    4 => 2.13,
                    _ => zf * 0.4,
                },
                2 => match z {
                    2 => 3.5,
                    3 => 4.16,
                    4 => 4.79,
                    _ => {
                        if zf < 15.0 {
                            zf * 0.9
                        } else {
                            zf * 0.75
                        }
                    }
                },
                3 => {
                    if zf < 7.0 {
                        zf * 1.66
                    } else {
                        zf * 1.18
                    }
                }
                4 => {
                    if zf < 7.0 {
                        zf * 2.26
                    } else {
                        zf * 1.53
                    }
                }
                _ => zf * (y as f32 * 0.3 + 0.66),
            },
            2 => match y {
                2 => match z {
                    2 => 7.34,
                    3 => 9.90,
                    4 => 11.22,
                    _ => {
                        if zf < 15.0 {
                            zf * 1.9
                        } else {
                            zf * 1.5
                        }
                    }
                },
                3 => match z {
                    3 => 15.0,
                    4 => 19.0,
                    _ => {
                        if zf < 15.0 {
                            zf * 2.0
                        } else {
                            zf * 1.5
                        }
                    }
                },
                _ => zf * (y as f32 * 0.66),
            },
            _ => (x * y * z) as f32 * 0.25,
        }
    };
    if s[0] < 1.0 {
        m *= s[0];
    }
    m
}

pub fn joint_k(size: Vec3, ball: bool) -> f32 {
    joint_k_multiplier(size, ball) * 960000.0
}

impl PhysicsWorld {
    pub fn step(&mut self, dm: &mut DataModel, dt: f64) -> StepEvents {
        self.sync(dm);
        self.rebuild_assemblies();
        self.accumulator += dt;
        let step = 1.0 / 240.0;
        let mut n = 0;
        while self.accumulator >= step - 1e-9 && n < MAX_STEPS_PER_CALL {
            self.accumulator -= step;
            self.world_step(dm);
            n += 1;
        }
        if n == MAX_STEPS_PER_CALL {
            self.accumulator = 0.0;
        }
        self.write_back(dm);
        self.take_events()
    }

    pub fn world_step(&mut self, dm: &DataModel) {
        if self.topology_dirty {
            self.rebuild_assemblies();
        }
        self.step_count += 1;
        self.time += WORLD_DT as f64;
        if self
            .step_count
            .is_multiple_of(WORLD_STEPS_PER_UI_STEP as u64)
        {
            self.ui_step();
            if !self.humanoids.is_empty() {
                self.refresh_humanoid_cache(dm);
                let long = self
                    .step_count
                    .is_multiple_of(2 * WORLD_STEPS_PER_UI_STEP as u64);
                self.humanoid_ui_step(dm, long, WORLD_STEPS_PER_UI_STEP as f32 * WORLD_DT);
            }
        }
        self.apply_dirty_angles();
        self.compute_forces(dm);
        self.step_joints();
        let pairs = self.find_pairs();
        let active = self.narrowphase(pairs);
        self.solve_step(&active);
        self.sleep_step(&active);
        self.fallen_check();
    }

    fn apply_dirty_angles(&mut self) {
        let mut dirty_assemblies = HashSet::new();
        for j in self.joints.values_mut() {
            if j.angle_dirty {
                j.angle_dirty = false;
                if let Some(p) = j.part0.and_then(|p| self.index.get(&p)) {
                    dirty_assemblies.insert(self.parts[*p].assembly);
                }
            }
        }
        for a in dirty_assemblies {
            if a < self.assemblies.len() {
                self.update_assembly_offsets(a);
                self.assemblies[a].sleeping = false;
            }
        }
    }

    fn ui_step(&mut self) {
        let t = self.time;
        let mut changed = HashSet::new();
        let ids: Vec<InstanceId> = self.joints.keys().copied().collect();
        for id in ids {
            let (part0, part1, in_world) = {
                let j = &self.joints[&id];
                (j.part0, j.part1, j.in_world)
            };
            if !in_world {
                continue;
            }
            let axle_input = part0.and_then(|p| self.index.get(&p)).map(|&i| {
                let j = &self.joints[&id];
                let face = PhysicsWorld::surface_normal_id(&j.c0) as usize;
                self.parts[i].surface_inputs[face]
            });
            let Some(j) = self.joints.get_mut(&id) else {
                continue;
            };
            match j.kind {
                JointKind::Motor | JointKind::VelocityMotor => {
                    let max_vel = j.max_velocity.abs();
                    let delta = j.desired_angle - j.current_angle;
                    let scripted = if delta.abs() < max_vel {
                        j.desired_angle
                    } else if delta > 0.0 {
                        j.current_angle + max_vel
                    } else {
                        j.current_angle - max_vel
                    };
                    if j.pose_freshness > 0 {
                        j.pose_freshness -= 1;
                    } else if j.pose_mask_weight < 1.0 || j.pose_angle > 0.0 {
                        j.pose_mask_weight = (j.pose_mask_weight + 1.0 / 32.0).min(1.0);
                        j.pose_angle = (j.pose_angle * (1.0 - 1.0 / 32.0) - 0.01).max(0.0);
                    }
                    let new = scripted * j.pose_mask_weight + j.pose_angle;
                    if new != j.current_angle {
                        j.current_angle = new;
                        changed.insert((part0, part1));
                    }
                }
                JointKind::Motor6D => {
                    let max_vel = j.max_velocity.abs();
                    let current_z = j.current_axis_angle.z;
                    let delta = j.desired_angle - current_z;
                    let scripted = if delta.abs() < max_vel {
                        j.desired_angle
                    } else if delta > 0.0 {
                        current_z + max_vel
                    } else {
                        current_z - max_vel
                    };
                    if j.pose_freshness > 0 {
                        j.pose_freshness -= 1;
                    } else if j.pose_mask_weight < 1.0
                        || j.pose_axis_angle != Vec3::ZERO
                        || j.pose_offset != Vec3::ZERO
                    {
                        j.pose_mask_weight = (j.pose_mask_weight + 1.0 / 32.0).min(1.0);
                        let sq = j.pose_axis_angle.length_squared();
                        if sq != 0.0 {
                            let nsq = (sq * (1.0 - 1.0 / 32.0) - 0.01).max(0.0);
                            j.pose_axis_angle *= nsq / sq;
                        }
                        let sm = j.pose_offset.length_squared();
                        if sm != 0.0 {
                            let nsm = (sm * (1.0 - 1.0 / 32.0) - 0.01).max(0.0);
                            j.pose_offset *= nsm / sm;
                        }
                    }
                    let aa = Vec3::new(0.0, 0.0, scripted * j.pose_mask_weight) + j.pose_axis_angle;
                    let off = j.pose_offset;
                    if aa != j.current_axis_angle || off != j.current_offset {
                        j.current_axis_angle = aa;
                        j.current_offset = off;
                        j.current_angle = aa.z;
                        changed.insert((part0, part1));
                    }
                }
                JointKind::RotateP | JointKind::RotateV => {
                    let (input, pa, pb) = axle_input.unwrap_or((0, 0.0, 0.0));
                    j.ui_value = match input {
                        12 => pb,
                        13 => pa * (t as f32 * pb).sin(),
                        _ => 0.0,
                    };
                    if j.ui_value != 0.0 {
                        changed.insert((part0, part1));
                    }
                }
                _ => {}
            }
        }
        let mut assemblies = HashSet::new();
        for (p0, p1) in changed {
            for p in [p0, p1].into_iter().flatten() {
                if let Some(&i) = self.index.get(&p) {
                    assemblies.insert(self.parts[i].assembly);
                }
            }
        }
        for a in assemblies {
            if a >= self.assemblies.len() {
                continue;
            }
            let has_motor = self.assemblies[a]
                .tree
                .iter()
                .any(|e| self.joints.get(&e.joint).is_some_and(|j| j.kind.is_motor()));
            if has_motor {
                self.update_assembly_offsets(a);
            }
            if !self.assemblies[a].anchored {
                if self.assemblies[a].sleeping {
                    self.grid_dirty = true;
                }
                self.assemblies[a].sleeping = false;
                self.assemblies[a].history.wake();
            }
        }
    }

    fn part_velocity(&self, i: usize) -> (Vec3, Vec3) {
        let p = &self.parts[i];
        let a = &self.assemblies[p.assembly];
        if a.anchored {
            (p.velocity, p.rot_velocity)
        } else {
            (a.velocity_at(p.cframe.translation), a.angular)
        }
    }

    fn compute_forces(&mut self, dm: &DataModel) {
        for a in self.assemblies.iter_mut() {
            a.force = Vec3::new(0.0, -GRAVITY * a.mass, 0.0);
            a.torque = Vec3::ZERO;
        }
        self.humanoid_forces();
        let mut ids: Vec<InstanceId> = self.movers.keys().copied().collect();
        ids.sort();
        let mut wake = Vec::new();
        let mut reached = Vec::new();
        for id in ids {
            let (part, kind) = {
                let m = &self.movers[&id];
                (m.part, m.kind)
            };
            let Some(&pi) = part.and_then(|p| self.index.get(&p)) else {
                continue;
            };
            let ai = self.parts[pi].assembly;
            if ai >= self.assemblies.len() || self.assemblies[ai].anchored {
                continue;
            }
            let body = self.parts[pi].cframe;
            let (v_body, w_body) = self.part_velocity(pi);
            let asm = &self.assemblies[ai];
            let mass = asm.mass;
            let ibody = asm.ibody();
            let mut force = Vec3::ZERO;
            let mut torque = Vec3::ZERO;
            match kind {
                MoverKind::Position {
                    p,
                    d,
                    max_force,
                    position,
                } => {
                    let accel =
                        (position - body.translation) * (p * PSCALE) - v_body * (d * PSCALE);
                    force = clamp_vec(accel * mass, max_force);
                    if !self.movers[&id].reached
                        && (body.translation - position).length_squared() <= 0.01
                    {
                        reached.push(id);
                    }
                }
                MoverKind::Velocity {
                    p,
                    max_force,
                    velocity,
                } => {
                    let accel = (velocity - asm.linear) * (p * PSCALE);
                    force = clamp_vec(accel * mass, max_force);
                }
                MoverKind::AngularVelocity {
                    p,
                    max_torque,
                    angular_velocity,
                } => {
                    let accel = (angular_velocity - w_body) * (p * PSCALE);
                    torque = clamp_vec(accel * mass, max_torque);
                }
                MoverKind::Force { force: f } => force = f,
                MoverKind::Thrust { force: f, location } => {
                    let fw = body.rotation * f;
                    force = fw;
                    torque = (body * location - asm.position).cross(fw);
                }
                MoverKind::Gyro {
                    p,
                    d,
                    max_torque,
                    cframe,
                } => {
                    let kp = p * PSCALE;
                    let kd = d * PSCALE;
                    let mut added = Vec3::ZERO;
                    let old = asm.torque;
                    if !(max_torque.x.abs() < 1e-5 && max_torque.z.abs() < 1e-5) {
                        let local_y = body.vector_to_object_space(cframe.up_vector());
                        let dx = kp * ibody.x * local_y.z;
                        let dz = -kp * ibody.z * local_y.x;
                        let mut tb = body.vector_to_object_space(old + added);
                        if (dx - tb.x).abs() < max_torque.x * ibody.x {
                            tb.x = dx;
                        } else {
                            tb.x += dx;
                        }
                        if (dz - tb.z).abs() < max_torque.z * ibody.z {
                            tb.z = dz;
                        } else {
                            tb.z += dz;
                        }
                        let wb = body.vector_to_object_space(w_body);
                        tb.x -= kd * ibody.x * wb.x;
                        tb.z -= kd * ibody.z * wb.z;
                        added += body.vector_to_world_space(tb) - (old + added);
                    }
                    if max_torque.y.abs() >= 1e-5 {
                        let local_zneg = body.vector_to_object_space(cframe.look_vector());
                        let dy = -kp * ibody.y * local_zneg.x;
                        let cur = old + added;
                        let mut tb = body.vector_to_object_space(cur);
                        if (dy - tb.y).abs() < max_torque.y * ibody.y {
                            tb.y = dy;
                        } else {
                            tb.y += dy;
                        }
                        let wb = body.vector_to_object_space(w_body);
                        tb.y -= kd * ibody.y * wb.y;
                        added += body.vector_to_world_space(tb) - cur;
                    }
                    torque = added;
                }
                MoverKind::Rocket(r) => {
                    if r.active {
                        let target_pos = match r.target.and_then(|t| self.index.get(&t)) {
                            Some(&ti) => self.parts[ti].cframe * r.target_offset,
                            None => match r.target {
                                Some(t) => dm
                                    .get_raw(t, "CFrame")
                                    .and_then(|v| v.as_cframe())
                                    .map(|c| c * r.target_offset)
                                    .unwrap_or(r.target_offset),
                                None => r.target_offset,
                            },
                        };
                        let delta = target_pos - body.translation;
                        let dir = delta.normalize_or_zero();
                        let mut f = -asm.force;
                        let s = dir.dot(f);
                        let mut t = r.max_thrust * r.max_thrust - f.dot(f) + s * s;
                        t = if t > 0.0 { t.sqrt() - s } else { 0.0 };
                        f += dir * t;
                        let max_speed = r.max_speed.clamp(1.0, 10000.0);
                        let drag = r.max_thrust / (max_speed * max_speed);
                        let vel = asm.linear;
                        f -= vel * (drag * vel.length());
                        f = f.clamp(Vec3::splat(-r.max_thrust), Vec3::splat(r.max_thrust));
                        force = f;
                        let fdir = if f.length_squared() > 1e-6 {
                            f.normalize()
                        } else {
                            Vec3::ZERO
                        };
                        let aim = (dir * r.cartoon_factor + fdir * (1.0 - r.cartoon_factor))
                            .normalize_or_zero();
                        let local_z = body.vector_to_object_space(aim);
                        let old = asm.torque;
                        let kp = r.turn_p * PSCALE;
                        let kd = r.turn_d * PSCALE;
                        let dx = kp * ibody.x * local_z.y;
                        let dy = -kp * ibody.y * local_z.x;
                        let mut tb = body.vector_to_object_space(old);
                        if (dx - tb.x).abs() < r.max_torque.x * ibody.x {
                            tb.x = dx;
                        } else {
                            tb.x += dx;
                        }
                        if (dy - tb.y).abs() < r.max_torque.y * ibody.y {
                            tb.y = dy;
                        } else {
                            tb.y += dy;
                        }
                        let wb = body.vector_to_object_space(w_body);
                        tb.x -= kd * ibody.x * wb.x;
                        tb.y -= kd * ibody.y * wb.y;
                        torque = body.vector_to_world_space(tb) - old;
                        if !self.movers[&id].reached && delta.length() < r.target_radius {
                            reached.push(id);
                        }
                    }
                }
            }
            let asm = &mut self.assemblies[ai];
            asm.force += force;
            asm.torque += torque;
            wake.push((ai, id, force, torque));
        }
        for (ai, id, force, torque) in wake {
            let key = id;
            let entry = self
                .mover_wake
                .entry(key)
                .or_insert((Vec3::ZERO, Vec3::ZERO));
            if different_5_percent(force, entry.0) || different_5_percent(torque, entry.1) {
                *entry = (force, torque);
                let asm = &mut self.assemblies[ai];
                if asm.sleeping {
                    asm.sleeping = false;
                    asm.history.wake();
                    self.grid_dirty = true;
                }
            }
        }
        for id in reached {
            if let Some(m) = self.movers.get_mut(&id) {
                m.reached = true;
            }
            self.events_mut().reached_target.push(id);
        }
    }

    fn step_joints(&mut self) {
        let ids: Vec<InstanceId> = self
            .joints
            .values()
            .filter(|j| {
                matches!(
                    j.kind,
                    JointKind::RotateP
                        | JointKind::RotateV
                        | JointKind::Rotate
                        | JointKind::Glue
                        | JointKind::ManualGlue
                )
            })
            .map(|j| j.id)
            .collect();
        for id in ids {
            self.ensure_joint_constraints(id);
            let Some(j) = self.joints.get(&id) else {
                continue;
            };
            let Some((a, b)) = self.active_joint(j) else {
                continue;
            };
            let kind = j.kind;
            match kind {
                JointKind::RotateP => {
                    let target = j.ui_value + j.base_angle;
                    let delta = delta_rotation_close(target, j.stepping_angle);
                    let inc = delta.clamp(-0.01, 0.01);
                    let new_angle = j.stepping_angle + inc;
                    let c1 = j.c1;
                    let axis = Mat3::from_axis_angle(c1.look_vector(), new_angle) * c1.up_vector();
                    if let Some(j) = self.joints.get_mut(&id) {
                        j.stepping_angle = new_angle;
                        if let Some(c) = j.constraints.get_mut(2) {
                            c.set_axis_b(axis);
                        }
                    }
                }
                JointKind::RotateV => {
                    let face0 = PhysicsWorld::surface_normal_id(&j.c0);
                    let face1 = PhysicsWorld::surface_normal_id(&j.c1);
                    let pa = &self.parts[a];
                    let pb = &self.parts[b];
                    let arm = |size: Vec3, face: NormalId| {
                        let n = face as usize % 3;
                        size[(n + 1) % 3].max(size[(n + 2) % 3])
                    };
                    let l = arm(pa.shape.size, face0).min(arm(pb.shape.size, face1)) * 0.10;
                    let k = joint_k(pa.shape.size, false).min(joint_k(pb.shape.size, false));
                    let ui = j.ui_value;
                    if let Some(j) = self.joints.get_mut(&id)
                        && let Some(Constraint {
                            kind:
                                ConstraintKind::AngularVelocity {
                                    max_force, desired, ..
                                },
                            ..
                        }) = j.constraints.get_mut(2)
                    {
                        *max_force = (1.0 + 10.0 * ui.abs()) * k * l * l;
                        *desired = ui * -31.0;
                    }
                }
                _ => {}
            }
        }
    }

    fn find_pairs(&mut self) -> Vec<(usize, usize)> {
        let n = self.parts.len();
        let mut aabbs = Vec::with_capacity(n);
        let mut moving = Vec::new();
        for (i, p) in self.parts.iter_mut().enumerate() {
            let a = &self.assemblies[p.assembly];
            if a.is_moving() || self.grid_dirty {
                p.aabb = p
                    .shape
                    .collider
                    .compute_aabb(&to_pose(&(p.cframe * p.shape.local)))
                    .loosened(0.01);
            }
            aabbs.push(p.aabb);
            if a.is_moving() {
                moving.push((i, p.aabb));
            }
        }
        if self.grid_dirty {
            self.grid.clear();
            for (i, p) in self.parts.iter().enumerate() {
                if !self.assemblies[p.assembly].is_moving() {
                    self.grid.insert(i, &aabbs[i]);
                }
            }
            self.grid_dirty = false;
        }
        let mut pairs = Vec::new();
        sweep_pairs(&moving, &mut pairs);
        let mut found = Vec::new();
        for &(i, aabb) in &moving {
            found.clear();
            self.grid.query(&aabb, &aabbs, &mut found);
            for &j in &found {
                if !self.assemblies[self.parts[j].assembly].is_moving() {
                    pairs.push((i, j));
                }
            }
        }
        pairs.retain(|&(i, j)| self.parts[i].assembly != self.parts[j].assembly);
        pairs
    }

    fn narrowphase(&mut self, pairs: Vec<(usize, usize)>) -> Vec<(InstanceId, InstanceId)> {
        let long_step = (self.step_count / WORLD_STEPS_PER_LONG_STEP as u64) as i64;
        let dispatcher = DefaultQueryDispatcher;
        let step = self.step_count;
        let mut active = Vec::new();
        let mut touched = Vec::new();
        let mut wake = Vec::new();
        for (i, j) in pairs {
            let (i, j) = if self.parts[i].id < self.parts[j].id {
                (i, j)
            } else {
                (j, i)
            };
            let key = (self.parts[i].id, self.parts[j].id);
            let pa = &self.parts[i];
            let pb = &self.parts[j];
            let pose_a = to_pose(&(pa.cframe * pa.shape.local));
            let pose_b = to_pose(&(pb.cframe * pb.shape.local));
            let pos12 = pose_a.inv_mul(&pose_b);
            let pair = self.contacts.entry(key).or_insert_with(|| ContactPair {
                a: key.0,
                b: key.1,
                manifolds: Vec::new(),
                workspace: None,
                collisions: Vec::new(),
                last_long_step: -1,
                overlapping: false,
                seen: 0,
            });
            pair.seen = step;
            let ok = dispatcher
                .contact_manifolds(
                    &pos12,
                    pa.shape.collider.as_ref(),
                    pb.shape.collider.as_ref(),
                    0.0,
                    &mut pair.manifolds,
                    &mut pair.workspace,
                )
                .is_ok();
            let overlapping = ok
                && pair
                    .manifolds
                    .iter()
                    .any(|m| m.points.iter().any(|c| c.dist <= 0.0));
            pair.overlapping = overlapping;
            if overlapping {
                if pair.last_long_step < 0 {
                    touched.push(key);
                    let ai = pa.assembly;
                    let bi = pb.assembly;
                    wake.push(ai);
                    wake.push(bi);
                }
                pair.last_long_step = long_step;
                if pa.can_collide && pb.can_collide {
                    active.push(key);
                }
            } else if pair.last_long_step >= 0 && pair.last_long_step < long_step {
                pair.last_long_step = -1;
                self.pending_touch_ended(key);
            }
        }
        for key in touched {
            self.events_mut().touched.push(key);
        }
        for a in wake {
            if let Some(asm) = self.assemblies.get_mut(a)
                && asm.sleeping
            {
                asm.sleeping = false;
                asm.history.wake();
                self.grid_dirty = true;
            }
        }
        let stale: Vec<(InstanceId, InstanceId)> = self
            .contacts
            .iter()
            .filter(|(_, c)| {
                c.seen != step && c.last_long_step >= 0 && c.last_long_step < long_step
            })
            .map(|(k, _)| *k)
            .collect();
        for key in stale {
            let moving = self.pair_has_moving(key);
            if moving && let Some(c) = self.contacts.get_mut(&key) {
                c.last_long_step = -1;
                c.overlapping = false;
                c.manifolds.clear();
                self.pending_touch_ended(key);
            }
        }
        self.contacts
            .retain(|_, c| c.seen == step || c.last_long_step >= 0 || step - c.seen < 240);
        active
    }

    fn pair_has_moving(&self, key: (InstanceId, InstanceId)) -> bool {
        let f = |id: &InstanceId| {
            self.index
                .get(id)
                .map(|&i| self.assemblies[self.parts[i].assembly].is_moving())
        };
        f(&key.0).unwrap_or(true) || f(&key.1).unwrap_or(true)
    }

    fn pending_touch_ended(&mut self, key: (InstanceId, InstanceId)) {
        self.events_mut().touch_ended.push(key);
    }

    fn solve_step(&mut self, active: &[(InstanceId, InstanceId)]) {
        let mut bodies = vec![SolverBody::ground()];
        let mut map: HashMap<usize, usize> = HashMap::new();
        for (ai, a) in self.assemblies.iter().enumerate() {
            if a.is_moving() && !a.kinematic {
                map.insert(ai, bodies.len());
                bodies.push(SolverBody {
                    uid: a.uid,
                    simulated: true,
                    state: BodyState {
                        position: a.position,
                        orientation: a.orientation,
                        linear_velocity: a.linear,
                        angular_velocity: a.angular,
                    },
                    mass_inv: if a.mass > 0.0 { 1.0 / a.mass } else { 0.0 },
                    inv_inertia_world: a.inv_inertia_world(),
                    force: a.force,
                    torque: a.torque,
                    impulse: Vec3::ZERO,
                    angular_impulse: Vec3::ZERO,
                });
            }
        }
        let mut ensure = |ai: usize,
                          bodies: &mut Vec<SolverBody>,
                          assemblies: &[crate::world::Assembly]|
         -> usize {
            if let Some(&b) = map.get(&ai) {
                return b;
            }
            let a = &assemblies[ai];
            let idx = bodies.len();
            bodies.push(SolverBody {
                uid: if a.anchored || a.kinematic { 0 } else { a.uid },
                simulated: false,
                state: BodyState {
                    position: a.position,
                    orientation: a.orientation,
                    linear_velocity: if a.anchored || a.kinematic {
                        a.linear
                    } else {
                        Vec3::ZERO
                    },
                    angular_velocity: if a.anchored || a.kinematic {
                        a.angular
                    } else {
                        Vec3::ZERO
                    },
                },
                mass_inv: 0.0,
                inv_inertia_world: Mat3::ZERO,
                force: Vec3::ZERO,
                torque: Vec3::ZERO,
                impulse: Vec3::ZERO,
                angular_impulse: Vec3::ZERO,
            });
            map.insert(ai, idx);
            idx
        };

        let mode = self.mode;
        let mut joint_specs: Vec<(InstanceId, usize, usize, CFrame, CFrame)> = Vec::new();
        let mut jids: Vec<InstanceId> = self
            .joints
            .values()
            .filter(|j| !j.constraints.is_empty())
            .map(|j| j.id)
            .collect();
        jids.sort();
        for jid in jids {
            let j = &self.joints[&jid];
            let Some((a, b)) = self.active_joint(j) else {
                continue;
            };
            let aa = self.parts[a].assembly;
            let ab = self.parts[b].assembly;
            if aa == ab || (!self.assemblies[aa].is_moving() && !self.assemblies[ab].is_moving()) {
                continue;
            }
            let ba = ensure(aa, &mut bodies, &self.assemblies);
            let bb = ensure(ab, &mut bodies, &self.assemblies);
            joint_specs.push((jid, ba, bb, self.parts[a].cframe, self.parts[b].cframe));
        }

        let mut collision_specs: Vec<CollisionSpec> = Vec::new();
        let mut sorted_active = active.to_vec();
        sorted_active.sort();
        for key in sorted_active {
            let (Some(&i), Some(&j)) = (self.index.get(&key.0), self.index.get(&key.1)) else {
                continue;
            };
            let pa = &self.parts[i];
            let pb = &self.parts[j];
            let (friction, restitution) = match mode {
                PhysicalMode::Legacy => (
                    legacy_friction(pa.legacy_friction, pb.legacy_friction),
                    pa.legacy_elasticity.min(pb.legacy_elasticity),
                ),
                PhysicalMode::New => (
                    weighted(
                        pa.friction_weight,
                        pa.friction,
                        pb.friction_weight,
                        pb.friction,
                    ),
                    weighted(
                        pa.elasticity_weight,
                        pa.elasticity,
                        pb.elasticity_weight,
                        pb.elasticity,
                    ),
                ),
            };
            let pose_a = pa.cframe * pa.shape.local;
            let ba = ensure(pa.assembly, &mut bodies, &self.assemblies);
            let bb = ensure(pb.assembly, &mut bodies, &self.assemblies);
            let Some(pair) = self.contacts.get_mut(&key) else {
                continue;
            };
            let mut points = Vec::new();
            for m in &pair.manifolds {
                let n = pose_a.rotation * crate::math::from_parry_vec(m.local_n1);
                if n.length_squared() < 0.8 {
                    continue;
                }
                let n = n.normalize();
                for c in &m.points {
                    if c.dist <= 0.0 {
                        let p = pose_a * crate::math::from_parry_vec(c.local_p1);
                        points.push((n, p, c.dist));
                    }
                }
            }
            if points.is_empty() {
                continue;
            }
            if pair.collisions.len() > points.len() {
                pair.collisions.truncate(points.len());
            }
            while pair.collisions.len() < points.len() {
                pair.collisions
                    .push(Constraint::new(ConstraintKind::collision()));
            }
            for (c, (n, p, d)) in pair.collisions.iter_mut().zip(points) {
                if let ConstraintKind::Collision {
                    normal,
                    point_a,
                    depth,
                    friction: f,
                    restitution: r,
                    ..
                } = &mut c.kind
                {
                    *normal = n;
                    *point_a = p;
                    *depth = d;
                    *f = friction;
                    *r = restitution;
                }
            }
            collision_specs.push((key, ba, bb, pa.cframe, pb.cframe));
        }

        if map.is_empty() {
            return;
        }
        let mut refs: Vec<ConstraintRef<'_>> = Vec::new();
        let joints = &mut self.joints;
        let contacts = &mut self.contacts;
        let mut joint_lookup: HashMap<InstanceId, (usize, usize, CFrame, CFrame)> = HashMap::new();
        for (jid, ba, bb, fa, fb) in joint_specs {
            joint_lookup.insert(jid, (ba, bb, fa, fb));
        }
        let mut jlist: Vec<(&InstanceId, &mut crate::world::Joint)> = joints
            .iter_mut()
            .filter(|(k, _)| joint_lookup.contains_key(*k))
            .collect();
        jlist.sort_by_key(|(k, _)| **k);
        for (jid, j) in jlist {
            let (ba, bb, fa, fb) = joint_lookup[jid];
            for c in j.constraints.iter_mut() {
                if c.broken {
                    continue;
                }
                refs.push(ConstraintRef {
                    constraint: c,
                    body_a: ba,
                    body_b: bb,
                    frame_a: fa,
                    frame_b: fb,
                });
            }
        }
        let mut col_lookup: HashMap<(InstanceId, InstanceId), (usize, usize, CFrame, CFrame)> =
            HashMap::new();
        for (key, ba, bb, fa, fb) in collision_specs {
            col_lookup.insert(key, (ba, bb, fa, fb));
        }
        let mut clist: Vec<(&(InstanceId, InstanceId), &mut ContactPair)> = contacts
            .iter_mut()
            .filter(|(k, _)| col_lookup.contains_key(*k))
            .collect();
        clist.sort_by_key(|(k, _)| **k);
        for (key, pair) in clist {
            let (ba, bb, fa, fb) = col_lookup[key];
            for c in pair.collisions.iter_mut() {
                refs.push(ConstraintRef {
                    constraint: c,
                    body_a: ba,
                    body_b: bb,
                    frame_a: fa,
                    frame_b: fb,
                });
            }
        }
        self.solver.solve(&mut bodies, &mut refs, WORLD_DT);
        drop(refs);
        let broken: Vec<InstanceId> = self
            .joints
            .values()
            .filter(|j| !j.constraints.is_empty() && j.constraints.iter().all(|c| c.broken))
            .map(|j| j.id)
            .collect();
        self.events_mut().broken_joints.extend(broken);
        for (ai, bi) in map {
            let b = &bodies[bi];
            if !b.simulated {
                continue;
            }
            let asm = &mut self.assemblies[ai];
            asm.position = b.state.position;
            asm.orientation = b.state.orientation;
            asm.linear = b.state.linear_velocity;
            asm.angular = b.state.angular_velocity;
            asm.moved = true;
            self.sync_part_frames(ai);
        }
    }

    fn sleep_step(&mut self, active: &[(InstanceId, InstanceId)]) {
        let mut neighbors: HashMap<usize, Vec<usize>> = HashMap::new();
        for key in active {
            let (Some(&i), Some(&j)) = (self.index.get(&key.0), self.index.get(&key.1)) else {
                continue;
            };
            let a = self.parts[i].assembly;
            let b = self.parts[j].assembly;
            neighbors.entry(a).or_default().push(b);
            neighbors.entry(b).or_default().push(a);
        }
        let mut candidates = Vec::new();
        for (ai, a) in self.assemblies.iter_mut().enumerate() {
            if !a.is_moving() || a.kinematic {
                continue;
            }
            let q = rbx_math::quat_from_matrix(&a.orientation);
            let coord = (
                a.position,
                rbx_math::Vec4::new(q.x, q.y, q.z, q.w) * a.radius,
            );
            if a.history.sample(coord) {
                candidates.push(ai);
            }
        }
        let mut to_sleep = Vec::new();
        for ai in candidates {
            let blocked = neighbors.get(&ai).is_some_and(|ns| {
                ns.iter().any(|&n| {
                    let o = &self.assemblies[n];
                    o.is_moving() && o.history_prevents_sleep()
                })
            });
            if !blocked {
                to_sleep.push(ai);
            }
        }
        if !to_sleep.is_empty() {
            self.grid_dirty = true;
        }
        for ai in to_sleep {
            self.assemblies[ai].sleeping = true;
        }
    }

    fn fallen_check(&mut self) {
        let h = self.fallen_parts_destroy_height;
        let mut fallen = Vec::new();
        for a in &self.assemblies {
            if a.is_moving() && a.position.y < h {
                for &p in &a.parts {
                    fallen.push(self.parts[p].id);
                }
            }
        }
        if fallen.is_empty() {
            return;
        }
        for id in &fallen {
            self.remove_part(*id);
        }
        self.events_mut().fallen.extend(fallen);
        self.rebuild_assemblies();
    }
}

fn delta_rotation_close(target: f32, current: f32) -> f32 {
    let two_pi = std::f32::consts::TAU;
    let mut d = (target - current) % two_pi;
    if d > std::f32::consts::PI {
        d -= two_pi;
    } else if d < -std::f32::consts::PI {
        d += two_pi;
    }
    d
}

pub fn face_vector(id: NormalId) -> Vec3 {
    normal_id_to_vector(id)
}

pub fn aabb_of(min: Vec3, max: Vec3) -> Aabb {
    Aabb::new(
        crate::math::to_parry_vec(min),
        crate::math::to_parry_vec(max),
    )
}
