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
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::VecDeque;

const PSCALE: f32 = 1.0 / 19.0;
const GYRO_FITNESS_SENSITIVITY: f32 = 2.0;

type CollisionSpec = ((InstanceId, InstanceId), usize, usize, CFrame, CFrame);
const MAX_STEPS_PER_CALL: u32 = 240;

pub(crate) fn legacy_friction(c0: f32, c1: f32) -> f32 {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SleepVerdict {
    Awake,
    Checking,
    Deep,
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
        let terrain_changes = self.terrain.sync(dm);
        self.wake_terrain_changes(&terrain_changes);
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
        self.release_touched_kinematic(&active);
        let terrain_active = self.terrain_narrowphase();
        if !self.recursive_wake.is_empty() {
            self.wake_step(true);
        }
        self.solve_step(&active, &terrain_active);
        let neighbors = self.wake_step(false);
        self.sleep_step(&neighbors);
        self.fallen_check();
        self.flush_spatial();
    }

    fn apply_dirty_angles(&mut self) {
        let mut dirty_assemblies = HashSet::default();
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
        let mut changed = HashSet::default();
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
        let mut assemblies = HashSet::default();
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
                self.assemblies[a].sleeping = false;
                self.assemblies[a].history.wake();
                self.external_wake.push(a);
            }
        }
    }

    pub(crate) fn part_velocity(&self, i: usize) -> (Vec3, Vec3) {
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
        self.buoyancy_forces();
        self.vehicle_seat_forces(dm);
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
            let (_, w_body) = self.part_velocity(pi);
            let asm = &self.assemblies[ai];
            let ibody = asm.ibody();
            let mut force = Vec3::ZERO;
            let mut torque = Vec3::ZERO;
            let mut solver_kind: Option<ConstraintKind> = None;
            let mut stability = self.movers[&id].stability;
            match kind {
                MoverKind::Position {
                    p,
                    d,
                    max_force,
                    position,
                } => {
                    let (p, d) = correct_pd_for_time_step(p, d);
                    let com = asm.position;
                    solver_kind = Some(ConstraintKind::LinearSpring {
                        pivot_a: body.point_to_object_space(com),
                        pivot_b: position + com - body.translation,
                        max_force,
                        p,
                        d,
                    });
                    if !self.movers[&id].reached
                        && (body.translation - position).length_squared() <= 0.01
                    {
                        reached.push(id);
                    }
                }
                MoverKind::Velocity {
                    max_force,
                    velocity,
                    ..
                } => {
                    solver_kind = Some(ConstraintKind::LinearVelocity {
                        max_force,
                        desired: velocity,
                    });
                }
                MoverKind::AngularVelocity {
                    max_torque,
                    angular_velocity,
                    ..
                } => {
                    solver_kind = Some(ConstraintKind::LegacyAngularVelocity {
                        target: angular_velocity,
                        max_torque,
                        min_torque: -max_torque,
                        use_integrated: true,
                    });
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
                    let (kp, kd) = correct_pd_for_time_step(p, d);
                    let wb = body.vector_to_object_space(w_body);
                    let mut tb = Vec3::ZERO;
                    if !(max_torque.x.abs() < 1e-5 && max_torque.z.abs() < 1e-5) {
                        let local_y = body.vector_to_object_space(cframe.up_vector());
                        let t = stability[0].fit_second(local_y.x);
                        let fitness_z = (-GYRO_FITNESS_SENSITIVITY * t).exp();
                        stability[0].add(local_y.x);
                        let t = stability[2].fit_second(local_y.z);
                        let fitness_x = (-GYRO_FITNESS_SENSITIVITY * t).exp();
                        stability[2].add(local_y.z);
                        tb.x = (kp * local_y.z - kd * wb.x) * fitness_x * ibody.x;
                        tb.z = (-kp * local_y.x - kd * wb.z) * fitness_z * ibody.z;
                    }
                    if max_torque.y.abs() >= 1e-5 {
                        let local_zneg = body.vector_to_object_space(cframe.look_vector());
                        let t = stability[1].fit_second(local_zneg.x);
                        let fitness = (-GYRO_FITNESS_SENSITIVITY * t).exp();
                        stability[1].add(local_zneg.x);
                        tb.y = -fitness * ibody.y * (kp * local_zneg.x + kd * wb.y);
                    }
                    let delta = tb / ibody.max(Vec3::splat(1e-12)) * WORLD_DT;
                    let limit = ibody * max_torque;
                    solver_kind = Some(ConstraintKind::BodyAngularVelocity {
                        target: delta + wb,
                        max_torque: limit,
                        min_torque: -limit,
                        use_integrated: false,
                    });
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
            if let Some(m) = self.movers.get_mut(&id) {
                m.stability = stability;
                match (solver_kind, m.constraint.as_mut()) {
                    (Some(k), Some(c))
                        if std::mem::discriminant(&k) == std::mem::discriminant(&c.kind) =>
                    {
                        c.kind = k;
                    }
                    (Some(k), _) => m.constraint = Some(Constraint::new(k)),
                    (None, _) => m.constraint = None,
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
                self.assemblies[ai].history.wake();
                self.external_wake.push(ai);
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
        let mut moving = Vec::new();
        for (i, p) in self.parts.iter_mut().enumerate() {
            if self.assemblies[p.assembly].is_moving() {
                p.aabb = p
                    .shape
                    .collider
                    .compute_aabb(&to_pose(&(p.cframe * p.shape.local)))
                    .loosened(0.01);
                self.spatial.update(p.id, p.aabb);
                moving.push(i);
            }
        }
        self.flush_spatial();
        let mut pairs = Vec::new();
        let mut found = Vec::new();
        for &i in &moving {
            found.clear();
            let aabb = self.parts[i].aabb;
            self.spatial.query(&aabb, &mut found);
            let ai = self.parts[i].assembly;
            for id in &found {
                let Some(&j) = self.index.get(id) else {
                    continue;
                };
                let aj = self.parts[j].assembly;
                if aj == ai {
                    continue;
                }
                if self.assemblies[aj].is_moving() && j < i {
                    continue;
                }
                if self.joined(self.parts[i].id, *id) {
                    continue;
                }
                pairs.push((i, j));
            }
        }
        pairs
    }

    fn release_touched_kinematic(&mut self, active: &[(InstanceId, InstanceId)]) {
        for &(a, b) in active {
            let (Some(&ia), Some(&ib)) = (self.index.get(&a), self.index.get(&b)) else {
                continue;
            };
            let (aa, ab) = (self.parts[ia].assembly, self.parts[ib].assembly);
            for (k, o) in [(aa, ab), (ab, aa)] {
                let other = &self.assemblies[o];
                if self.assemblies[k].kinematic && other.is_moving() && !other.kinematic {
                    self.assemblies[k].kinematic = false;
                }
            }
        }
    }

    fn narrowphase(&mut self, pairs: Vec<(usize, usize)>) -> Vec<(InstanceId, InstanceId)> {
        let long_step = (self.step_count / WORLD_STEPS_PER_LONG_STEP as u64) as i64;
        let dispatcher = DefaultQueryDispatcher;
        let step = self.step_count;
        let mut active = Vec::new();
        let mut touched = Vec::new();
        let mut wake = Vec::new();
        let mut recursive = Vec::new();
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
            let was_overlapping = pair.overlapping;
            pair.overlapping = overlapping;
            if was_overlapping && !overlapping {
                wake.push(pa.assembly);
                wake.push(pb.assembly);
            }
            if overlapping {
                if !was_overlapping {
                    let ai = pa.assembly;
                    let bi = pb.assembly;
                    let high = [ai, bi]
                        .iter()
                        .any(|&a| self.assemblies.get(a).is_some_and(|x| x.high_velocity()));
                    if high {
                        recursive.push(ai);
                        recursive.push(bi);
                    } else {
                        wake.push(ai);
                        wake.push(bi);
                    }
                }
                if pair.last_long_step < 0 {
                    touched.push(key);
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
                asm.wake_from_sleep();
            }
        }
        self.recursive_wake.extend(recursive);
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
                c.workspace = None;
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

    fn solve_step(
        &mut self,
        active: &[(InstanceId, InstanceId)],
        terrain_active: &[(InstanceId, rbx_math::IVec3)],
    ) {
        let mut bodies = vec![SolverBody::ground()];
        let mut map: HashMap<usize, usize> = HashMap::default();
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

        let mut mover_specs: Vec<(InstanceId, usize, CFrame)> = Vec::new();
        let mut mids: Vec<InstanceId> = self
            .movers
            .iter()
            .filter(|(_, m)| m.constraint.is_some())
            .map(|(k, _)| *k)
            .collect();
        mids.sort();
        for mid in mids {
            let Some(&pi) = self.movers[&mid].part.and_then(|p| self.index.get(&p)) else {
                continue;
            };
            let ai = self.parts[pi].assembly;
            let a = &self.assemblies[ai];
            if !a.is_moving() || a.kinematic {
                continue;
            }
            let ba = ensure(ai, &mut bodies, &self.assemblies);
            mover_specs.push((mid, ba, self.parts[pi].cframe));
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

        let mut terrain_specs: Vec<((InstanceId, rbx_math::IVec3), usize, CFrame)> = Vec::new();
        for &key in terrain_active {
            let Some(&i) = self.index.get(&key.0) else {
                continue;
            };
            let pa = &self.parts[i];
            let pose_a = pa.cframe * pa.shape.local;
            let ba = ensure(pa.assembly, &mut bodies, &self.assemblies);
            let smooth = self.terrain.smooth.is_some();
            let base = self.terrain_params(
                i,
                if smooth {
                    None
                } else {
                    Some(rbx_dom::rbx_terrain::conversion::PLASTIC)
                },
            );
            let Some(pair) = self.terrain.contacts.get_mut(&key) else {
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
                        points.push((n, p, c.dist, m.subshape2));
                    }
                }
            }
            if points.is_empty() {
                continue;
            }
            let materials: Vec<Option<u32>> = points
                .iter()
                .map(|(_, _, _, sub)| {
                    if smooth {
                        self.terrain.triangle_material(key.1, *sub)
                    } else {
                        None
                    }
                })
                .collect();
            let Some(pair) = self.terrain.contacts.get_mut(&key) else {
                continue;
            };
            pair.collisions.truncate(points.len());
            while pair.collisions.len() < points.len() {
                pair.collisions
                    .push(Constraint::new(ConstraintKind::collision()));
            }
            let mode = self.mode;
            let part_props = (
                pa.friction_weight,
                pa.friction,
                pa.elasticity_weight,
                pa.elasticity,
            );
            for ((c, (n, p, d, _)), mat) in pair.collisions.iter_mut().zip(points).zip(materials) {
                let (fr, re) = match (mode, mat) {
                    (PhysicalMode::New, Some(m)) => {
                        let t = rbx_math::materials::material_physics(m);
                        (
                            weighted(part_props.0, part_props.1, t.friction_weight, t.friction),
                            weighted(
                                part_props.2,
                                part_props.3,
                                t.elasticity_weight,
                                t.elasticity,
                            ),
                        )
                    }
                    _ => (base.friction, base.restitution),
                };
                if let ConstraintKind::Collision {
                    normal,
                    point_a,
                    depth,
                    friction,
                    restitution,
                    ..
                } = &mut c.kind
                {
                    *normal = n;
                    *point_a = p;
                    *depth = d;
                    *friction = fr;
                    *restitution = re;
                }
            }
            terrain_specs.push((key, ba, pa.cframe));
        }

        if map.is_empty() {
            return;
        }
        let mut refs: Vec<ConstraintRef<'_>> = Vec::new();
        let joints = &mut self.joints;
        let contacts = &mut self.contacts;
        let mut joint_lookup: HashMap<InstanceId, (usize, usize, CFrame, CFrame)> =
            HashMap::default();
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
        let mover_lookup: HashMap<InstanceId, (usize, CFrame)> = mover_specs
            .into_iter()
            .map(|(id, ba, fa)| (id, (ba, fa)))
            .collect();
        let mut mlist: Vec<(&InstanceId, &mut crate::world::Mover)> = self
            .movers
            .iter_mut()
            .filter(|(k, _)| mover_lookup.contains_key(*k))
            .collect();
        mlist.sort_by_key(|(k, _)| **k);
        for (mid, m) in mlist {
            let (ba, fa) = mover_lookup[mid];
            if let Some(c) = m.constraint.as_mut() {
                refs.push(ConstraintRef {
                    constraint: c,
                    body_a: ba,
                    body_b: 0,
                    frame_a: fa,
                    frame_b: CFrame::IDENTITY,
                });
            }
        }
        let mut col_lookup: HashMap<(InstanceId, InstanceId), (usize, usize, CFrame, CFrame)> =
            HashMap::default();
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
        let mut tlookup: HashMap<(InstanceId, rbx_math::IVec3), (usize, CFrame)> =
            HashMap::default();
        for (key, ba, fa) in terrain_specs {
            tlookup.insert(key, (ba, fa));
        }
        let mut tlist: Vec<(&(InstanceId, rbx_math::IVec3), &mut ContactPair)> = self
            .terrain
            .contacts
            .iter_mut()
            .filter(|(k, _)| tlookup.contains_key(*k))
            .collect();
        tlist.sort_by_key(|(k, _)| (k.0, k.1.x, k.1.y, k.1.z));
        for (key, pair) in tlist {
            let (ba, fa) = tlookup[key];
            for c in pair.collisions.iter_mut() {
                refs.push(ConstraintRef {
                    constraint: c,
                    body_a: ba,
                    body_b: 0,
                    frame_a: fa,
                    frame_b: CFrame::IDENTITY,
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
        for &ai in map.keys() {
            let asm = &mut self.assemblies[ai];
            asm.pre_solve_linear = asm.linear;
            asm.pre_solve_angular = asm.angular;
        }
        if self.spring_kernel_place {
            self.latch_humanoid_impacts();
        }
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

    fn sleep_neighbors(&self) -> Vec<Vec<(usize, bool)>> {
        let n = self.assemblies.len();
        let mut neighbors: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
        let mut link = |a: usize, b: usize, joint: bool| {
            if a == b
                || a >= n
                || b >= n
                || self.assemblies[a].anchored
                || self.assemblies[b].anchored
            {
                return;
            }
            for (x, y) in [(a, b), (b, a)] {
                match neighbors[x].iter_mut().find(|(o, _)| *o == y) {
                    Some(e) => e.1 |= joint,
                    None => neighbors[x].push((y, joint)),
                }
            }
        };
        for (key, pair) in &self.contacts {
            if !pair.overlapping {
                continue;
            }
            let (Some(&i), Some(&j)) = (self.index.get(&key.0), self.index.get(&key.1)) else {
                continue;
            };
            if self.parts[i].can_collide && self.parts[j].can_collide {
                link(self.parts[i].assembly, self.parts[j].assembly, false);
            }
        }
        for j in self.joints.values() {
            if let Some((a, b)) = self.active_joint(j) {
                link(self.parts[a].assembly, self.parts[b].assembly, true);
            }
        }
        neighbors
    }

    fn state_from_neighbors(&self, ai: usize, neighbors: &[Vec<(usize, bool)>]) -> SleepVerdict {
        if !self.allow_sleep {
            return SleepVerdict::Awake;
        }
        let mut all_sleeping = true;
        for &(n, _) in &neighbors[ai] {
            let o = &self.assemblies[n];
            if o.is_moving() {
                all_sleeping = false;
                if o.history_prevents_sleep() {
                    return SleepVerdict::Awake;
                }
            }
        }
        if all_sleeping {
            SleepVerdict::Deep
        } else {
            SleepVerdict::Checking
        }
    }

    fn wake_step(&mut self, contact_only: bool) -> Vec<Vec<(usize, bool)>> {
        let neighbors = self.sleep_neighbors();
        let mut external = Vec::new();
        let mut plain = Vec::new();
        if !contact_only {
            external = std::mem::take(&mut self.external_wake);
            for p in std::mem::take(&mut self.tickled_parts) {
                if let Some(&i) = self.index.get(&p) {
                    external.push(self.parts[i].assembly);
                }
            }
            plain = std::mem::take(&mut self.plain_wake);
        }
        let contact = std::mem::take(&mut self.recursive_wake);
        for (pending, through_awake, max_depth) in
            [(external, true, 8), (contact, false, 8), (plain, false, 0)]
        {
            if pending.is_empty() {
                continue;
            }
            let mut depth: HashMap<usize, usize> = HashMap::default();
            let mut queue: VecDeque<usize> = VecDeque::new();
            for a in pending {
                if a < self.assemblies.len()
                    && !self.assemblies[a].anchored
                    && !depth.contains_key(&a)
                {
                    depth.insert(a, 0);
                    queue.push_back(a);
                }
            }
            while let Some(a) = queue.pop_front() {
                let d = depth[&a];
                if !contact_only || self.assemblies[a].sleeping {
                    self.assemblies[a].wake_from_sleep();
                }
                for &(o, joint) in &neighbors[a] {
                    if depth.contains_key(&o) {
                        continue;
                    }
                    if d < max_depth {
                        if through_awake || self.assemblies[o].sleeping {
                            depth.insert(o, d + 1);
                            queue.push_back(o);
                        }
                    } else if joint {
                        depth.insert(o, 0);
                        queue.push_back(o);
                    }
                }
            }
        }
        let woke: Vec<usize> = (0..self.assemblies.len())
            .filter(|&a| self.assemblies[a].woke)
            .collect();
        for a in woke {
            self.assemblies[a].woke = false;
            for &(o, _) in &neighbors[a] {
                let other = &mut self.assemblies[o];
                if other.sleeping && !other.sleep_checking {
                    other.sleep_checking = true;
                }
            }
        }
        neighbors
    }

    fn sleep_step(&mut self, neighbors: &[Vec<(usize, bool)>]) {
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
        let mut changes = Vec::new();
        for ai in candidates {
            match self.state_from_neighbors(ai, neighbors) {
                SleepVerdict::Awake => {}
                SleepVerdict::Deep => changes.push((ai, false)),
                SleepVerdict::Checking => changes.push((ai, true)),
            }
        }
        for (ai, checking) in changes {
            self.assemblies[ai].fall_asleep(checking);
        }
        if self.step_count.is_multiple_of(4) {
            let checking: Vec<usize> = (0..self.assemblies.len())
                .filter(|&a| {
                    let x = &self.assemblies[a];
                    x.sleeping && x.sleep_checking && !x.anchored
                })
                .collect();
            let mut verdicts = Vec::new();
            for ai in checking {
                verdicts.push((ai, self.state_from_neighbors(ai, neighbors)));
            }
            for (ai, v) in verdicts {
                match v {
                    SleepVerdict::Awake => self.assemblies[ai].wake_from_sleep(),
                    SleepVerdict::Deep => self.assemblies[ai].sleep_checking = false,
                    SleepVerdict::Checking => {}
                }
            }
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

fn correct_pd_for_time_step(p: f32, d: f32) -> (f32, f32) {
    let discriminant = 4.0 * p - d * d;
    if discriminant >= 0.0 {
        let w = 0.5 * discriminant.sqrt();
        let threshold = std::f32::consts::FRAC_PI_6;
        if w * WORLD_DT > threshold {
            let s = threshold / (w * WORLD_DT);
            return (p * s * s, d * s);
        }
    } else {
        let w = 0.5 * d;
        if w > 0.8 / WORLD_DT {
            let s = 0.8 / (w * WORLD_DT);
            return (p * s * s, d * s);
        }
    }
    (p, d)
}
