use crate::math::{fill_rotation, orthonormal_basis};
use rbx_math::{CFrame, Mat3, Vec3, orthonormalize};
use std::collections::HashMap;

pub const WORLD_DT: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Debug)]
pub struct Modulation {
    pub threshold_max: f32,
    pub threshold_min: f32,
    pub aggressive: f32,
    pub conservative: f32,
    pub easing_up: f32,
    pub easing_down: f32,
}

#[derive(Clone, Debug)]
pub struct SolverConfig {
    pub pgs_iterations: u32,
    pub collision_restitution_threshold: f32,
    pub collision_penetration_margin: f32,
    pub collision_penetration_resolution_damping: f32,
    pub collision_friction_static_to_dynamic_threshold: f32,
    pub collision_friction_static_scale: f32,
    pub collision_friction_dynamic_scale: f32,
    pub align2_axes_friction_constant: f32,
    pub align2_axes_position_stage_friction_constant: f32,
    pub align2_axes_max_corrective_angle: f32,
    pub align2_axes_correction_damping: f32,
    pub ball_in_socket_max_correction: f32,
    pub ball_in_socket_correction_damping: f32,
    pub sor_constraints: Modulation,
    pub sor_collisions: Modulation,
    pub cache_vel: Modulation,
    pub cache_pos: Modulation,
    pub stabilization_mass_reduction_power: f32,
    pub stabilization_inertia_scale: f32,
    pub vel_cache_damping: f32,
    pub pos_cache_damping: f32,
    pub constraint_caching: bool,
    pub angular_damping: f32,
    pub integrate_only_positions: bool,
    pub block_pgs: bool,
    pub velocity_stage_sor: bool,
    pub position_stage_sor: bool,
    pub virtual_masses: bool,
}

impl Default for SolverConfig {
    fn default() -> Self {
        SolverConfig {
            pgs_iterations: 20,
            collision_restitution_threshold: 1.0,
            collision_penetration_margin: 0.05,
            collision_penetration_resolution_damping: 0.7,
            collision_friction_static_to_dynamic_threshold: 1.0,
            collision_friction_static_scale: 1.0,
            collision_friction_dynamic_scale: 1.0,
            align2_axes_friction_constant: 0.02,
            align2_axes_position_stage_friction_constant: 0.01,
            align2_axes_max_corrective_angle: 3.0,
            align2_axes_correction_damping: 1.0,
            ball_in_socket_max_correction: 0.3,
            ball_in_socket_correction_damping: 1.0,
            sor_constraints: Modulation {
                threshold_max: 1.0,
                threshold_min: 0.01,
                aggressive: 1.5,
                conservative: 0.75,
                easing_up: 0.0001,
                easing_down: 0.2,
            },
            sor_collisions: Modulation {
                threshold_max: 0.05,
                threshold_min: 0.01,
                aggressive: 1.9,
                conservative: 1.0,
                easing_up: 0.0001,
                easing_down: 0.2,
            },
            cache_vel: Modulation {
                threshold_min: 0.5,
                threshold_max: 2.0,
                conservative: 0.93,
                aggressive: 0.99,
                easing_down: 0.01,
                easing_up: 0.0002,
            },
            cache_pos: Modulation {
                threshold_min: 0.5,
                threshold_max: 1.5,
                conservative: 0.70,
                aggressive: 0.99,
                easing_down: 0.01,
                easing_up: 0.0002,
            },
            stabilization_mass_reduction_power: 0.3,
            stabilization_inertia_scale: 0.05,
            vel_cache_damping: 1.0,
            pos_cache_damping: 1.0,
            constraint_caching: true,
            angular_damping: 0.911328,
            integrate_only_positions: false,
            block_pgs: true,
            velocity_stage_sor: true,
            position_stage_sor: true,
            virtual_masses: true,
        }
    }
}

impl SolverConfig {
    pub fn positional_correction() -> SolverConfig {
        SolverConfig {
            pos_cache_damping: 0.0,
            vel_cache_damping: 0.0,
            constraint_caching: false,
            block_pgs: false,
            velocity_stage_sor: false,
            position_stage_sor: false,
            collision_friction_static_scale: 0.0,
            collision_friction_dynamic_scale: 0.0,
            integrate_only_positions: true,
            ..SolverConfig::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Jac {
    lin_a: Vec3,
    ang_a: Vec3,
    lin_b: Vec3,
    ang_b: Vec3,
}

impl Jac {
    fn dot(&self, o: &Jac) -> f32 {
        self.lin_a.dot(o.lin_a)
            + self.ang_a.dot(o.ang_a)
            + self.lin_b.dot(o.lin_b)
            + self.ang_b.dot(o.ang_b)
    }

    fn scaled(&self, s: f32) -> Jac {
        Jac {
            lin_a: self.lin_a * s,
            ang_a: self.ang_a * s,
            lin_b: self.lin_b * s,
            ang_b: self.ang_b * s,
        }
    }

    fn add(&self, o: &Jac) -> Jac {
        Jac {
            lin_a: self.lin_a + o.lin_a,
            ang_a: self.ang_a + o.ang_a,
            lin_b: self.lin_b + o.lin_b,
            ang_b: self.ang_b + o.ang_b,
        }
    }

    fn project(&self, va: &VirD, vb: &VirD) -> f32 {
        self.lin_a.dot(va.lin)
            + self.ang_a.dot(va.ang)
            + self.lin_b.dot(vb.lin)
            + self.ang_b.dot(vb.ang)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vars {
    pub min: f32,
    pub max: f32,
    pub reaction: f32,
    pub impulse: f32,
}

impl Default for Vars {
    fn default() -> Self {
        Vars {
            min: f32::NEG_INFINITY,
            max: f32::INFINITY,
            reaction: 0.0,
            impulse: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct VirD {
    lin: Vec3,
    ang: Vec3,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MovingRegression {
    confidence: f32,
    last_point: f32,
    last_tangent: f32,
    last_curvature: f32,
}

impl MovingRegression {
    fn fit(&self, y: f32, predicted: f32) -> f32 {
        self.confidence * (y - predicted).abs() / (y.abs().max(predicted.abs()) + 0.00001)
    }

    fn fit_zero(&self, y: f32) -> f32 {
        self.fit(y, self.last_point)
    }

    pub(crate) fn fit_second(&self, y: f32) -> f32 {
        self.fit(y, self.last_point + self.last_tangent + self.last_curvature)
    }

    pub(crate) fn add(&mut self, y: f32) {
        let t = y - self.last_point;
        self.last_curvature = t - self.last_tangent;
        self.last_tangent = t;
        self.last_point = y;
        self.confidence += 0.1 * (1.0 - self.confidence);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ConstraintCache {
    vel_impulse: f32,
    vel_reaction: f32,
    vel_sor: f32,
    vel_damping: f32,
    pos_impulse: f32,
    pos_reaction: f32,
    pos_sor: f32,
    pos_damping: f32,
    vel_regression: MovingRegression,
    pos_regression: MovingRegression,
}

impl Default for ConstraintCache {
    fn default() -> Self {
        ConstraintCache {
            vel_impulse: 0.0,
            vel_reaction: 0.0,
            vel_sor: 1.9,
            vel_damping: 1.0,
            pos_impulse: 0.0,
            pos_reaction: 0.0,
            pos_sor: 1.9,
            pos_damping: 1.0,
            vel_regression: MovingRegression::default(),
            pos_regression: MovingRegression::default(),
        }
    }
}

fn recompute_sor(sor: f32, rel_error: f32, m: &Modulation) -> f32 {
    let mut new_sor = m.aggressive;
    if rel_error > m.threshold_min {
        let t = rel_error.clamp(m.threshold_min, m.threshold_max);
        let t = (t - m.threshold_min) / (m.threshold_max - m.threshold_min);
        new_sor = t * m.conservative + (1.0 - t) * m.aggressive;
    }
    if new_sor > sor {
        m.easing_up * new_sor + (1.0 - m.easing_up) * sor
    } else {
        m.easing_down * new_sor + (1.0 - m.easing_down) * sor
    }
}

fn cache_damping(fit: f32, old: f32, m: &Modulation) -> f32 {
    let t = (fit.clamp(m.threshold_min, m.threshold_max) - m.threshold_min)
        / (m.threshold_max - m.threshold_min);
    let factor = t * m.conservative + (1.0 - t) * m.aggressive;
    let easing = if factor < old {
        m.easing_down
    } else {
        m.easing_up
    };
    easing * factor + (1.0 - easing) * old
}

impl ConstraintCache {
    fn store(
        &mut self,
        vel: &Vars,
        pos: &Vars,
        sor_vel: f32,
        sor_pos: f32,
        collision: bool,
        cfg: &SolverConfig,
    ) {
        let vel_fit = self.vel_regression.fit_second(vel.impulse);
        let pos_fit = self.pos_regression.fit_second(pos.impulse);
        self.vel_damping = cache_damping(vel_fit, self.vel_damping, &cfg.cache_vel);
        self.pos_damping = cache_damping(pos_fit, self.pos_damping, &cfg.cache_pos);
        if !collision {
            self.vel_sor = recompute_sor(sor_vel, vel_fit, &cfg.sor_constraints);
            self.pos_sor = recompute_sor(sor_pos, pos_fit, &cfg.sor_constraints);
        } else {
            let vf = self.vel_regression.fit_zero(vel.impulse);
            let pf = self.pos_regression.fit_zero(pos.impulse);
            self.vel_sor = recompute_sor(sor_vel, vf, &cfg.sor_collisions);
            self.pos_sor = recompute_sor(sor_pos, pf, &cfg.sor_collisions);
        }
        self.vel_impulse = self.vel_damping * vel.impulse;
        self.vel_reaction = vel.reaction;
        self.pos_impulse = self.pos_damping * pos.impulse;
        self.pos_reaction = pos.reaction;
        let vi = self.vel_impulse;
        let pi = self.pos_impulse;
        self.vel_regression.add(vi);
        self.pos_regression.add(pi);
    }
}

#[derive(Clone, Debug)]
pub enum ConstraintKind {
    Collision {
        normal: Vec3,
        point_a: Vec3,
        depth: f32,
        friction: f32,
        restitution: f32,
        cached_tangent1: Vec3,
    },
    BallInSocket {
        pivot_a: Vec3,
        pivot_b: Vec3,
    },
    LegacyBreakableBallInSocket {
        pivot_a: Vec3,
        pivot_b: Vec3,
        normal_a: Vec3,
        tangent_a1: Vec3,
        tangent_a2: Vec3,
        max_normal_force: f32,
    },
    Align2Axes {
        axis_a: Vec3,
        axis_b: Vec3,
        orth_b1: Vec3,
        orth_b2: Vec3,
        ws_orth_b1: Vec3,
        ws_orth_b2: Vec3,
    },
    AngularVelocity {
        axis_b: Vec3,
        max_force: f32,
        desired: f32,
    },
    LinearVelocity {
        max_force: Vec3,
        desired: Vec3,
    },
    LinearSpring {
        pivot_a: Vec3,
        pivot_b: Vec3,
        max_force: Vec3,
        p: f32,
        d: f32,
    },
    AchievePosition {
        pivot_a: Vec3,
        pivot_b: Vec3,
        max_force: Vec3,
        target_velocity: Vec3,
    },
    BodyAngularVelocity {
        target: Vec3,
        max_torque: Vec3,
        min_torque: Vec3,
        use_integrated: bool,
    },
    LegacyAngularVelocity {
        target: Vec3,
        max_torque: Vec3,
        min_torque: Vec3,
        use_integrated: bool,
    },
}

impl ConstraintKind {
    pub fn collision() -> ConstraintKind {
        ConstraintKind::Collision {
            normal: Vec3::Y,
            point_a: Vec3::ZERO,
            depth: 0.0,
            friction: 0.0,
            restitution: 0.0,
            cached_tangent1: Vec3::X,
        }
    }

    pub fn align2_axes(axis_a: Vec3, axis_b: Vec3) -> ConstraintKind {
        let (orth_b1, orth_b2) = orthonormal_basis(axis_b);
        ConstraintKind::Align2Axes {
            axis_a,
            axis_b,
            orth_b1,
            orth_b2,
            ws_orth_b1: Vec3::ZERO,
            ws_orth_b2: Vec3::ZERO,
        }
    }

    pub fn breakable_ball_in_socket(
        pivot_a: Vec3,
        pivot_b: Vec3,
        normal_a: Vec3,
        max_normal_force: f32,
    ) -> ConstraintKind {
        let (t1, t2) = orthonormal_basis(normal_a);
        ConstraintKind::LegacyBreakableBallInSocket {
            pivot_a,
            pivot_b,
            normal_a: normal_a.normalize_or(Vec3::X),
            tangent_a1: t1,
            tangent_a2: t2,
            max_normal_force,
        }
    }

    pub fn dimension(&self) -> usize {
        match self {
            ConstraintKind::AngularVelocity { .. } => 1,
            _ => 3,
        }
    }

    pub fn is_collision(&self) -> bool {
        matches!(self, ConstraintKind::Collision { .. })
    }
}

#[derive(Clone, Debug)]
pub struct Constraint {
    pub kind: ConstraintKind,
    pub broken: bool,
    cache: [ConstraintCache; 3],
}

impl Constraint {
    pub fn new(kind: ConstraintKind) -> Constraint {
        let mut cache = [ConstraintCache::default(); 3];
        if kind.is_collision() {
            for c in &mut cache {
                c.vel_sor = 1.0;
                c.pos_sor = 1.0;
            }
        }
        Constraint {
            kind,
            broken: false,
            cache,
        }
    }

    pub fn set_axis_b(&mut self, axis: Vec3) {
        if let ConstraintKind::Align2Axes {
            axis_b,
            orth_b1,
            orth_b2,
            ..
        } = &mut self.kind
        {
            *axis_b = axis;
            let (a, b) = orthonormal_basis(axis);
            *orth_b1 = a;
            *orth_b2 = b;
        }
    }

    pub fn velocity_impulse(&self, dim: usize) -> f32 {
        self.cache[dim].vel_impulse
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BodyState {
    pub position: Vec3,
    pub orientation: Mat3,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
}

#[derive(Clone, Debug)]
pub struct SolverBody {
    pub uid: u64,
    pub simulated: bool,
    pub state: BodyState,
    pub mass_inv: f32,
    pub inv_inertia_world: Mat3,
    pub force: Vec3,
    pub torque: Vec3,
    pub impulse: Vec3,
    pub angular_impulse: Vec3,
}

impl SolverBody {
    pub fn ground() -> SolverBody {
        SolverBody {
            uid: 0,
            simulated: false,
            state: BodyState {
                position: Vec3::ZERO,
                orientation: Mat3::IDENTITY,
                linear_velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
            },
            mass_inv: 0.0,
            inv_inertia_world: Mat3::ZERO,
            force: Vec3::ZERO,
            torque: Vec3::ZERO,
            impulse: Vec3::ZERO,
            angular_impulse: Vec3::ZERO,
        }
    }
}

pub struct ConstraintRef<'a> {
    pub constraint: &'a mut Constraint,
    pub body_a: usize,
    pub body_b: usize,
    pub frame_a: CFrame,
    pub frame_b: CFrame,
}

#[derive(Clone, Copy, Debug, Default)]
struct DynProps {
    integrated_lin: Vec3,
    integrated_ang: Vec3,
    orientation: Mat3,
    position: Vec3,
    lin: Vec3,
    ang: Vec3,
}

#[derive(Clone, Copy, Debug)]
struct MassInertia {
    inertia: Mat3,
    mass_inv: f32,
    pos_to_vel: f32,
}

impl Default for MassInertia {
    fn default() -> Self {
        MassInertia {
            inertia: Mat3::ZERO,
            mass_inv: 0.0,
            pos_to_vel: 0.0,
        }
    }
}

impl MassInertia {
    fn inv_mass(&self, pos: bool) -> f32 {
        if pos {
            self.mass_inv * self.pos_to_vel
        } else {
            self.mass_inv
        }
    }

    fn inv_inertia(&self, pos: bool, scale: f32) -> Mat3 {
        if pos {
            self.inertia * (scale * self.pos_to_vel)
        } else {
            self.inertia
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BodyCache {
    lin: Vec3,
    ang: Vec3,
    integrated_lin: Vec3,
    integrated_ang: Vec3,
    vird_vel: VirD,
    vird_pos: VirD,
    mass_inv: f32,
    inv_inertia: Mat3,
}

#[derive(Default)]
pub struct Solver {
    pub config: SolverConfig,
    body_cache: HashMap<u64, BodyCache>,
}

fn cross_rows(r: Vec3) -> [Vec3; 3] {
    [
        Vec3::new(0.0, -r.z, r.y),
        Vec3::new(r.z, 0.0, -r.x),
        Vec3::new(-r.y, r.x, 0.0),
    ]
}

fn point_jacobians(jac: &mut [Jac], rel_a: Vec3, rel_b: Vec3) {
    let ca = cross_rows(rel_a);
    let cb = cross_rows(rel_b);
    for i in 0..3 {
        let mut e = Vec3::ZERO;
        e[i] = 1.0;
        jac[i] = Jac {
            lin_a: e,
            lin_b: -e,
            ang_a: -ca[i],
            ang_b: cb[i],
        };
    }
}

fn set3(vars: &mut [Vars], f: impl Fn(&mut Vars, usize)) {
    for (i, v) in vars.iter_mut().enumerate().take(3) {
        f(v, i);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_equation(
    c: &mut ConstraintKind,
    jac: &mut [Jac],
    use_block: &mut [bool],
    vel: &mut [Vars],
    pos: &mut [Vars],
    ba: &DynProps,
    bb: &DynProps,
    ca: &CFrame,
    cb: &CFrame,
    cfg: &SolverConfig,
    dt: f32,
) {
    match c {
        ConstraintKind::BallInSocket { pivot_a, pivot_b } => {
            let wa = *ca * *pivot_a;
            let wb = *cb * *pivot_b;
            let ra = wa - ba.position;
            let rb = wb - bb.position;
            point_jacobians(jac, ra, rb);
            let va = ba.integrated_lin + ba.integrated_ang.cross(ra);
            let vb = bb.integrated_lin + bb.integrated_ang.cross(rb);
            let dv = vb - va;
            let dp = wb - wa;
            let err = dp.length();
            let solve = (cfg.ball_in_socket_correction_damping * err)
                .min(cfg.ball_in_socket_max_correction);
            let stab = if err > 0.0 {
                dp * (solve / err)
            } else {
                Vec3::ZERO
            };
            set3(vel, |v, i| v.reaction = dv[i]);
            set3(pos, |v, i| v.reaction = stab[i]);
        }
        ConstraintKind::LegacyBreakableBallInSocket {
            pivot_a,
            pivot_b,
            normal_a,
            tangent_a1,
            tangent_a2,
            max_normal_force,
        } => {
            use_block[0] = false;
            let wa = *ca * *pivot_a;
            let wb = *cb * *pivot_b;
            let ra = wa - ba.position;
            let rb = wb - bb.position;
            let axes = [
                ca.rotation * *normal_a,
                ca.rotation * *tangent_a1,
                ca.rotation * *tangent_a2,
            ];
            for i in 0..3 {
                jac[i] = Jac {
                    lin_a: axes[i],
                    lin_b: -axes[i],
                    ang_a: -axes[i].cross(ra),
                    ang_b: axes[i].cross(rb),
                };
            }
            let va = ba.integrated_lin + ba.integrated_ang.cross(ra);
            let vb = bb.integrated_lin + bb.integrated_ang.cross(rb);
            let dv = vb - va;
            let dp = wb - wa;
            let err = dp.length();
            let solve = (cfg.ball_in_socket_correction_damping * err)
                .min(cfg.ball_in_socket_max_correction);
            let stab = if err > 0.0 {
                dp * (solve / err)
            } else {
                Vec3::ZERO
            };
            set3(vel, |v, i| v.reaction = dv.dot(axes[i]));
            set3(pos, |v, i| v.reaction = stab.dot(axes[i]));
            vel[0].min = -*max_normal_force * dt;
            pos[0].impulse *= 0.95;
        }
        ConstraintKind::Align2Axes {
            axis_a,
            axis_b,
            orth_b1,
            orth_b2,
            ws_orth_b1,
            ws_orth_b2,
        } => {
            let old1 = *ws_orth_b1;
            let old2 = *ws_orth_b2;
            *ws_orth_b1 = cb.rotation * *orth_b1;
            *ws_orth_b2 = cb.rotation * *orth_b2;
            let w_axis_b = cb.rotation * *axis_b;
            let w_axis_a = ca.rotation * *axis_a;
            jac[0] = Jac {
                ang_a: *ws_orth_b1,
                ang_b: -*ws_orth_b1,
                ..Jac::default()
            };
            jac[1] = Jac {
                ang_a: *ws_orth_b2,
                ang_b: -*ws_orth_b2,
                ..Jac::default()
            };
            jac[2] = Jac {
                ang_a: w_axis_b,
                ang_b: -w_axis_b,
                ..Jac::default()
            };
            use_block[2] = false;
            let rel = bb.integrated_ang - ba.integrated_ang;
            let wvi = old1 * vel[0].impulse + old2 * vel[1].impulse;
            vel[0].impulse = ws_orth_b1.dot(wvi);
            vel[0].reaction = ws_orth_b1.dot(rel);
            vel[1].impulse = ws_orth_b2.dot(wvi);
            vel[1].reaction = ws_orth_b2.dot(rel);
            let max_imp = cfg.align2_axes_friction_constant
                * (vel[0].impulse * vel[0].impulse + vel[1].impulse * vel[1].impulse).sqrt();
            vel[2].max = max_imp;
            vel[2].min = -max_imp;
            vel[2].reaction = w_axis_b.dot(rel);
            let mut err = w_axis_a.cross(w_axis_b);
            let cos = w_axis_a.dot(w_axis_b);
            let max_angle = cfg.align2_axes_max_corrective_angle.to_radians();
            if cos < max_angle.cos() {
                let sin = err.length();
                let scale = if sin > 0.00001 {
                    max_angle.sin() / sin
                } else {
                    0.0
                };
                err *= scale;
            }
            err *= cfg.align2_axes_correction_damping;
            let wpi = old1 * pos[0].impulse + old2 * pos[1].impulse;
            pos[0].impulse = ws_orth_b1.dot(wpi);
            pos[0].reaction = ws_orth_b1.dot(err);
            pos[1].impulse = ws_orth_b2.dot(wpi);
            pos[1].reaction = ws_orth_b2.dot(err);
            let max_pos = cfg.align2_axes_position_stage_friction_constant
                * (pos[0].impulse * pos[0].impulse + pos[1].impulse * pos[1].impulse).sqrt();
            pos[2].max = max_pos;
            pos[2].min = -max_pos;
            pos[2].reaction = 0.0;
        }
        ConstraintKind::AngularVelocity {
            axis_b,
            max_force,
            desired,
        } => {
            let w = cb.rotation * *axis_b;
            jac[0] = Jac {
                ang_a: w,
                ang_b: -w,
                ..Jac::default()
            };
            let rel = bb.integrated_ang - ba.integrated_ang;
            let max_imp = *max_force * dt;
            vel[0].max = max_imp;
            vel[0].min = -max_imp;
            vel[0].reaction = w.dot(rel) - *desired;
            pos[0].max = 0.0;
            pos[0].min = 0.0;
            pos[0].reaction = 0.0;
        }
        ConstraintKind::LinearVelocity { max_force, desired } => {
            use_block[..3].fill(false);
            for i in 0..3 {
                let mut e = Vec3::ZERO;
                e[i] = 1.0;
                jac[i] = Jac {
                    lin_a: e,
                    lin_b: -e,
                    ..Jac::default()
                };
            }
            let rel = bb.integrated_lin - ba.integrated_lin;
            let r = *desired + rel;
            let mf = *max_force * dt;
            set3(vel, |v, i| {
                v.reaction = r[i];
                v.min = -mf[i];
                v.max = mf[i];
            });
            set3(pos, |v, _| {
                v.reaction = 0.0;
                v.min = 0.0;
                v.max = 0.0;
            });
        }
        ConstraintKind::LinearSpring {
            pivot_a,
            pivot_b,
            max_force,
            p,
            d,
        } => {
            use_block[..3].fill(false);
            let wa = *ca * *pivot_a;
            let wb = *cb * *pivot_b;
            let ra = wa - ba.position;
            let rb = wb - bb.position;
            point_jacobians(jac, ra, rb);
            let va = ba.integrated_lin + ba.integrated_ang.cross(ra);
            let vb = bb.integrated_lin + bb.integrated_ang.cross(rb);
            let dv = va - vb;
            let dp = wa - wb;
            let r = -(dp * *p + dv * *d) * WORLD_DT;
            let mf = *max_force * dt;
            set3(vel, |v, i| {
                v.reaction = r[i];
                v.min = -mf[i];
                v.max = mf[i];
            });
            set3(pos, |v, _| {
                v.reaction = 0.0;
                v.min = 0.0;
                v.max = 0.0;
            });
        }
        ConstraintKind::AchievePosition {
            pivot_a,
            pivot_b,
            max_force,
            target_velocity,
        } => {
            use_block[..3].fill(false);
            let wa = *ca * *pivot_a;
            let wb = *cb * *pivot_b;
            let ra = wa - ba.position;
            let rb = wb - bb.position;
            point_jacobians(jac, ra, rb);
            let va = ba.integrated_lin + ba.integrated_ang.cross(ra);
            let vb = bb.integrated_lin + bb.integrated_ang.cross(rb);
            let mut dv = vb - va;
            let dp = wb - wa;
            let tmin = target_velocity.abs() * (0.1 * dt);
            let tmax = target_velocity.abs() * dt;
            let adp = dp.abs();
            let mut t = Vec3::ZERO;
            for i in 0..3 {
                t[i] = if adp[i] - tmax[i] >= 0.0 {
                    1.0
                } else if tmin[i] - adp[i] >= 0.0 {
                    0.0
                } else {
                    (adp[i] - tmin[i]) / (tmax[i] - tmin[i])
                };
            }
            dv += t * *target_velocity;
            let stab = (Vec3::ONE - t) * dp;
            let mv = *max_force * dt;
            let mp = (Vec3::ONE - t) * mv * dt;
            set3(vel, |v, i| {
                v.reaction = dv[i];
                v.min = -mv[i];
                v.max = mv[i];
            });
            set3(pos, |v, i| {
                v.reaction = stab[i];
                v.min = -mp[i];
                v.max = mp[i];
            });
        }
        ConstraintKind::Collision {
            normal,
            point_a,
            depth,
            friction,
            restitution,
            cached_tangent1,
        } => {
            let n = *normal;
            let ra = *point_a - ba.position;
            let rb = *point_a + n * *depth - bb.position;
            jac[0] = Jac {
                lin_a: -n,
                lin_b: n,
                ang_a: -ra.cross(n),
                ang_b: rb.cross(n),
            };
            use_block[..3].fill(false);
            let idv = (ba.integrated_lin + ba.integrated_ang.cross(ra))
                - (bb.integrated_lin + bb.integrated_ang.cross(rb));
            let inv = idv.dot(n);
            let pdv = (ba.lin + ba.ang.cross(ra)) - (bb.lin + bb.ang.cross(rb));
            let pnv = pdv.dot(n);
            vel[0].reaction = inv
                + *restitution
                    * if pnv < cfg.collision_restitution_threshold {
                        0.0
                    } else {
                        pnv
                    };
            vel[0].min = 0.0;
            vel[0].max = f32::INFINITY;
            let itv = idv - n * inv;
            let ptv = pdv - n * pnv;
            let ptv2 = ptv.length_squared();
            let cached_t2 = cached_tangent1.cross(n);
            let thr = cfg.collision_friction_static_to_dynamic_threshold;
            if ptv2 < thr * thr {
                let (t1, t2) = orthonormal_basis(n);
                jac[1] = Jac {
                    lin_a: -t1,
                    lin_b: t1,
                    ang_a: -ra.cross(t1),
                    ang_b: rb.cross(t1),
                };
                jac[2] = Jac {
                    lin_a: -t2,
                    lin_b: t2,
                    ang_a: -ra.cross(t2),
                    ang_b: rb.cross(t2),
                };
                let bound = cfg.collision_friction_static_scale * *friction;
                let tvi = *cached_tangent1 * vel[1].impulse + cached_t2 * vel[2].impulse;
                vel[1].reaction = itv.dot(t1);
                vel[1].impulse = tvi.dot(t1);
                vel[1].min = -bound;
                vel[1].max = bound;
                vel[2].reaction = itv.dot(t2);
                vel[2].impulse = tvi.dot(t2);
                vel[2].min = -bound;
                vel[2].max = bound;
                let tpi = *cached_tangent1 * pos[1].impulse + cached_t2 * pos[2].impulse;
                pos[1].reaction = 0.0;
                pos[1].impulse = tpi.dot(t1);
                pos[1].min = -bound;
                pos[1].max = bound;
                pos[2].reaction = 0.0;
                pos[2].impulse = tpi.dot(t2);
                pos[2].min = -bound;
                pos[2].max = bound;
                *cached_tangent1 = t1;
            } else {
                let t1 = ptv.normalize_or(Vec3::X);
                let t2 = t1.cross(n);
                jac[1] = Jac {
                    lin_a: -t1,
                    lin_b: t1,
                    ang_a: -ra.cross(t1),
                    ang_b: rb.cross(t1),
                };
                jac[2] = Jac {
                    lin_a: -t2,
                    lin_b: t2,
                    ang_a: -ra.cross(t2),
                    ang_b: rb.cross(t2),
                };
                let tvi = *cached_tangent1 * vel[1].impulse + cached_t2 * vel[2].impulse;
                vel[1].reaction = itv.dot(t1);
                vel[1].impulse = tvi.dot(t1);
                vel[1].min = 0.0;
                vel[1].max = cfg.collision_friction_dynamic_scale * *friction;
                vel[2] = Vars {
                    min: 0.0,
                    max: 0.0,
                    reaction: 0.0,
                    impulse: 0.0,
                };
                for p in pos.iter_mut().take(3).skip(1) {
                    *p = Vars {
                        min: 0.0,
                        max: 0.0,
                        reaction: 0.0,
                        impulse: 0.0,
                    };
                }
                *cached_tangent1 = t1;
            }
            pos[0].reaction = -cfg.collision_penetration_resolution_damping
                * (*depth + cfg.collision_penetration_margin);
            pos[0].min = 0.0;
            pos[0].max = f32::INFINITY;
        }
        ConstraintKind::BodyAngularVelocity {
            target,
            max_torque,
            min_torque,
            use_integrated,
        } => {
            use_block[..3].fill(false);
            for i in 0..3 {
                let mut e = Vec3::ZERO;
                e[i] = 1.0;
                let w = ca.rotation * e;
                jac[i] = Jac {
                    ang_a: w,
                    ang_b: -w,
                    ..Jac::default()
                };
            }
            let rel = if *use_integrated {
                bb.integrated_ang - ba.integrated_ang
            } else {
                bb.ang - ba.ang
            };
            let rel = ca.vector_to_object_space(rel);
            let r = *target + rel;
            set3(vel, |v, i| {
                v.reaction = r[i];
                v.min = min_torque[i] * dt;
                v.max = max_torque[i] * dt;
            });
            set3(pos, |v, _| {
                v.reaction = 0.0;
                v.min = 0.0;
                v.max = 0.0;
            });
        }
        ConstraintKind::LegacyAngularVelocity {
            target,
            max_torque,
            min_torque,
            use_integrated,
        } => {
            use_block[..3].fill(false);
            for i in 0..3 {
                let mut e = Vec3::ZERO;
                e[i] = 1.0;
                jac[i] = Jac {
                    ang_a: e,
                    ang_b: -e,
                    ..Jac::default()
                };
            }
            let rel = if *use_integrated {
                bb.integrated_ang - ba.integrated_ang
            } else {
                bb.ang - ba.ang
            };
            let r = *target + rel;
            set3(vel, |v, i| {
                v.reaction = r[i];
                v.min = min_torque[i] * dt;
                v.max = max_torque[i] * dt;
            });
            set3(pos, |v, _| {
                v.reaction = 0.0;
                v.min = 0.0;
                v.max = 0.0;
            });
        }
    }
}

fn compute_broken(c: &ConstraintKind, vel: &[Vars]) -> bool {
    match c {
        ConstraintKind::LegacyBreakableBallInSocket {
            max_normal_force, ..
        } => vel[0].impulse < -0.9 * *max_normal_force * WORLD_DT,
        _ => false,
    }
}

fn invert_block(m: [[f32; 3]; 3], d: usize) -> [[f32; 3]; 3] {
    let mut r = [[0.0f32; 3]; 3];
    match d {
        1 => {
            if m[0][0] != 0.0 && m[0][0].is_finite() {
                r[0][0] = 1.0 / m[0][0];
            }
        }
        2 => {
            let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
            if det != 0.0 && det.is_finite() {
                r[0][0] = m[1][1] / det;
                r[1][1] = m[0][0] / det;
                r[0][1] = -m[0][1] / det;
                r[1][0] = -m[1][0] / det;
            }
        }
        _ => {
            let mm = Mat3::from_cols_array_2d(&m).transpose();
            let det = mm.determinant();
            if det != 0.0 && det.is_finite() {
                let inv = mm.inverse().transpose();
                r = inv.to_cols_array_2d();
            }
        }
    }
    r
}

struct Row {
    a: usize,
    b: usize,
    offset: usize,
    dim: usize,
    collision: bool,
}

fn integrate_velocities(
    d: &mut DynProps,
    m: &MassInertia,
    ext: Vec3,
    ext_ang: Vec3,
    dt: f32,
    cfg: &SolverConfig,
) {
    d.integrated_lin = d.lin + ext * m.mass_inv;
    d.integrated_ang = d.ang * (-cfg.angular_damping * dt).exp() + m.inertia * ext_ang;
}

impl Solver {
    pub fn clear_body_cache(&mut self, uid: u64) {
        self.body_cache.remove(&uid);
    }

    fn fudge_mass(&self, mass_inv: f32) -> f32 {
        mass_inv.powf(self.config.stabilization_mass_reduction_power)
    }

    #[allow(clippy::needless_range_loop)]
    pub fn solve(
        &mut self,
        bodies: &mut [SolverBody],
        constraints: &mut [ConstraintRef<'_>],
        dt: f32,
    ) {
        let cfg = self.config.clone();
        let n = bodies.len();
        let mut dynp = vec![DynProps::default(); n];
        let mut mass = vec![MassInertia::default(); n];
        let mut mult = vec![0.0f32; n];
        let mut vird_vel = vec![VirD::default(); n];
        let mut vird_pos = vec![VirD::default(); n];
        for (i, b) in bodies.iter().enumerate() {
            let mut d = DynProps {
                lin: b.state.linear_velocity,
                ang: b.state.angular_velocity,
                orientation: b.state.orientation,
                position: b.state.position,
                ..DynProps::default()
            };
            if b.simulated {
                let mi = b.mass_inv;
                let mp = self.fudge_mass(mi);
                let m = MassInertia {
                    inertia: b.inv_inertia_world,
                    mass_inv: mi,
                    pos_to_vel: if mi > 0.0 { mp / mi } else { 0.0 },
                };
                let ext = b.force * dt + b.impulse;
                let ext_ang = b.torque * dt + b.angular_impulse;
                integrate_velocities(&mut d, &m, ext, ext_ang, dt, &cfg);
                if cfg.integrate_only_positions {
                    d.lin = Vec3::ZERO;
                    d.ang = Vec3::ZERO;
                    d.integrated_lin = Vec3::ZERO;
                    d.integrated_ang = Vec3::ZERO;
                }
                mass[i] = m;
                mult[i] = 1.0;
            } else if let Some(cache) = self
                .body_cache
                .get(&b.uid)
                .filter(|_| b.uid != 0 && cfg.virtual_masses)
            {
                d.lin = cache.lin;
                d.ang = cache.ang;
                d.integrated_lin = cache.integrated_lin;
                d.integrated_ang = cache.integrated_ang;
                vird_vel[i] = cache.vird_vel;
                vird_pos[i] = cache.vird_pos;
                let mi = cache.mass_inv;
                let mp = self.fudge_mass(mi);
                mass[i] = MassInertia {
                    inertia: cache.inv_inertia,
                    mass_inv: mi,
                    pos_to_vel: if mi > 0.0 { mp / mi } else { 0.0 },
                };
            } else {
                d.integrated_lin = d.lin;
                d.integrated_ang = d.ang;
            }
            dynp[i] = d;
        }

        let mut rows = Vec::with_capacity(constraints.len());
        let mut total = 0usize;
        let mut order: Vec<usize> = (0..constraints.len()).collect();
        order.sort_by_key(|&i| constraints[i].constraint.kind.is_collision());
        for &ci in &order {
            let c = &constraints[ci];
            let dim = c.constraint.kind.dimension();
            rows.push(Row {
                a: c.body_a,
                b: c.body_b,
                offset: total,
                dim,
                collision: c.constraint.kind.is_collision(),
            });
            total += dim;
        }

        let mut jac = vec![Jac::default(); total];
        let mut vel = vec![Vars::default(); total];
        let mut pos = vec![Vars::default(); total];
        let mut sor_vel = vec![1.0f32; total];
        let mut sor_pos = vec![1.0f32; total];
        let mut use_block = vec![cfg.block_pgs; total];

        for (ri, &ci) in order.iter().enumerate() {
            let r = &rows[ri];
            let c = &mut constraints[ci];
            for k in 0..r.dim {
                let o = r.offset + k;
                let cache = &c.constraint.cache[k];
                vel[o] = Vars {
                    impulse: cache.vel_impulse * cfg.vel_cache_damping,
                    reaction: cache.vel_reaction,
                    ..Vars::default()
                };
                pos[o] = Vars {
                    impulse: cache.pos_impulse * cfg.pos_cache_damping,
                    reaction: cache.pos_reaction,
                    ..Vars::default()
                };
                sor_vel[o] = cache.vel_sor;
                sor_pos[o] = cache.pos_sor;
            }
            let ba = dynp[r.a];
            let bb = dynp[r.b];
            let range = r.offset..r.offset + r.dim;
            build_equation(
                &mut c.constraint.kind,
                &mut jac[range.clone()],
                &mut use_block[range.clone()],
                &mut vel[range.clone()],
                &mut pos[range.clone()],
                &ba,
                &bb,
                &c.frame_a,
                &c.frame_b,
                &cfg,
                dt,
            );
            if c.constraint.broken {
                for o in range {
                    vel[o] = Vars {
                        min: 0.0,
                        max: 0.0,
                        reaction: 0.0,
                        impulse: 0.0,
                    };
                    pos[o] = vel[o];
                }
            }
        }
        if !cfg.velocity_stage_sor {
            sor_vel.fill(1.0);
        }
        if !cfg.position_stage_sor {
            sor_pos.fill(1.0);
        }

        let mut em_vel = vec![Jac::default(); total];
        let mut em_pos = vec![Jac::default(); total];
        for r in &rows {
            let ma = mass[r.a];
            let mb = mass[r.b];
            for k in 0..r.dim {
                let o = r.offset + k;
                let j = &jac[o];
                em_vel[o] = Jac {
                    lin_a: j.lin_a * ma.inv_mass(false),
                    ang_a: ma.inv_inertia(false, 0.0) * j.ang_a,
                    lin_b: j.lin_b * mb.inv_mass(false),
                    ang_b: mb.inv_inertia(false, 0.0) * j.ang_b,
                };
                em_pos[o] = Jac {
                    lin_a: j.lin_a * ma.inv_mass(true),
                    ang_a: ma.inv_inertia(true, cfg.stabilization_inertia_scale) * j.ang_a,
                    lin_b: j.lin_b * mb.inv_mass(true),
                    ang_b: mb.inv_inertia(true, cfg.stabilization_inertia_scale) * j.ang_b,
                };
            }
        }

        let mut pj_vel = vec![Jac::default(); total];
        let mut pj_pos = vec![Jac::default(); total];
        for r in &rows {
            for (stage_em, stage_pj, stage_vars, stage_sor) in [
                (&em_vel, &mut pj_vel, &mut vel, &sor_vel),
                (&em_pos, &mut pj_pos, &mut pos, &sor_pos),
            ] {
                let d = r.dim;
                let mut m = [[0.0f32; 3]; 3];
                for i in 0..d {
                    for k in 0..d {
                        let o_i = r.offset + i;
                        let o_k = r.offset + k;
                        if i == k {
                            m[i][k] = jac[o_i].dot(&stage_em[o_i]);
                        } else if use_block[o_i] && use_block[o_k] {
                            let (lo, hi) = if i < k { (o_i, o_k) } else { (o_k, o_i) };
                            m[i][k] = jac[lo].dot(&stage_em[hi]);
                        }
                    }
                }
                let inv = invert_block(m, d);
                let mut reactions = [0.0f32; 3];
                for i in 0..d {
                    let mut pj = Jac::default();
                    let mut react = 0.0;
                    for k in 0..d {
                        pj = pj.add(&jac[r.offset + k].scaled(inv[i][k]));
                        react += inv[i][k] * stage_vars[r.offset + k].reaction;
                    }
                    let s = stage_sor[r.offset + i];
                    stage_pj[r.offset + i] = pj.scaled(s);
                    reactions[i] = s * react;
                }
                for i in 0..d {
                    stage_vars[r.offset + i].reaction = reactions[i];
                }
            }
        }

        for r in &rows {
            for k in 0..r.dim {
                let o = r.offset + k;
                for em in [&mut em_vel[o], &mut em_pos[o]] {
                    em.lin_a *= mult[r.a];
                    em.ang_a *= mult[r.a];
                    em.lin_b *= mult[r.b];
                    em.ang_b *= mult[r.b];
                }
            }
        }

        for r in &rows {
            for k in 0..r.dim {
                let o = r.offset + k;
                apply(
                    &mut vird_vel[r.a],
                    vel[o].impulse,
                    em_vel[o].lin_a,
                    em_vel[o].ang_a,
                );
                apply(
                    &mut vird_vel[r.b],
                    vel[o].impulse,
                    em_vel[o].lin_b,
                    em_vel[o].ang_b,
                );
                apply(
                    &mut vird_pos[r.a],
                    pos[o].impulse,
                    em_pos[o].lin_a,
                    em_pos[o].ang_a,
                );
                apply(
                    &mut vird_pos[r.b],
                    pos[o].impulse,
                    em_pos[o].lin_b,
                    em_pos[o].ang_b,
                );
            }
        }

        for _ in 0..cfg.pgs_iterations {
            for r in &rows {
                update_stage(r, &mut vel, &mut vird_vel, &pj_vel, &em_vel);
                update_stage(r, &mut pos, &mut vird_pos, &pj_pos, &em_pos);
            }
        }

        if cfg.constraint_caching {
            for (ri, &ci) in order.iter().enumerate() {
                let r = &rows[ri];
                let c = &mut constraints[ci];
                let range = r.offset..r.offset + r.dim;
                if !c.constraint.broken {
                    c.constraint.broken = compute_broken(&c.constraint.kind, &vel[range.clone()]);
                }
                let collision = r.collision;
                for k in 0..r.dim {
                    let o = r.offset + k;
                    c.constraint.cache[k]
                        .store(&vel[o], &pos[o], sor_vel[o], sor_pos[o], collision, &cfg);
                }
            }
        }

        for (i, b) in bodies.iter_mut().enumerate() {
            if !b.simulated {
                continue;
            }
            let d = &mut dynp[i];
            if !cfg.integrate_only_positions {
                d.lin = d.integrated_lin + vird_vel[i].lin;
                d.ang = d.integrated_ang + vird_vel[i].ang;
            }
            d.position += vird_pos[i].lin;
            d.orientation = fill_rotation(vird_pos[i].ang) * d.orientation;
            if !cfg.integrate_only_positions {
                d.position += d.lin * dt;
                d.orientation = fill_rotation(d.ang * dt) * d.orientation;
            }
            d.orientation = orthonormalize(&d.orientation);
            if b.uid != 0 {
                self.body_cache.insert(
                    b.uid,
                    BodyCache {
                        lin: b.state.linear_velocity,
                        ang: b.state.angular_velocity,
                        integrated_lin: d.integrated_lin,
                        integrated_ang: d.integrated_ang,
                        vird_vel: vird_vel[i],
                        vird_pos: vird_pos[i],
                        mass_inv: b.mass_inv,
                        inv_inertia: b.inv_inertia_world,
                    },
                );
            }
            b.state.position = d.position;
            b.state.orientation = d.orientation;
            if cfg.integrate_only_positions {
                b.state.linear_velocity = Vec3::ZERO;
                b.state.angular_velocity = Vec3::ZERO;
            } else {
                b.state.linear_velocity = d.lin;
                b.state.angular_velocity = d.ang;
            }
        }
    }
}

fn apply(v: &mut VirD, impulse: f32, lin: Vec3, ang: Vec3) {
    v.lin += lin * impulse;
    v.ang += ang * impulse;
}

fn update_stage(r: &Row, vars: &mut [Vars], vird: &mut [VirD], pj: &[Jac], em: &[Jac]) {
    let d = r.dim;
    let mut projected = [0.0f32; 3];
    for (i, p) in projected.iter_mut().enumerate().take(d) {
        *p = pj[r.offset + i].project(&vird[r.a], &vird[r.b]);
    }
    let old: [f32; 3] = std::array::from_fn(|i| {
        if i < d {
            vars[r.offset + i].impulse
        } else {
            0.0
        }
    });
    let normal = old[0];
    let mut delta = [0.0f32; 3];
    for i in 0..d {
        let v = &mut vars[r.offset + i];
        let mut lo = v.min;
        let mut hi = v.max;
        if r.collision && i > 0 {
            lo *= normal;
            hi *= normal;
        }
        let new = (old[i] + (v.reaction - projected[i])).max(lo).min(hi);
        delta[i] = new - old[i];
        v.impulse = new;
    }
    for i in 0..d {
        let e = &em[r.offset + i];
        if delta[i] != 0.0 {
            if r.a == r.b {
                apply(
                    &mut vird[r.a],
                    delta[i],
                    e.lin_a + e.lin_b,
                    e.ang_a + e.ang_b,
                );
            } else {
                apply(&mut vird[r.a], delta[i], e.lin_a, e.ang_a);
                apply(&mut vird[r.b], delta[i], e.lin_b, e.ang_b);
            }
        }
    }
}
