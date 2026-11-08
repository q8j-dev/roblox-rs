use crate::shape::ShapeKind;
use crate::step::joint_k;
use crate::world::PhysicsWorld;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HState {
    FallingDown = 0,
    Ragdoll = 1,
    GettingUp = 2,
    Jumping = 3,
    Swimming = 4,
    Freefall = 5,
    Flying = 6,
    Landed = 7,
    Running = 8,
    RunningSlave = 9,
    RunningNoPhysics = 10,
    StrafingNoPhysics = 11,
    Climbing = 12,
    Seated = 13,
    PlatformStanding = 14,
    Dead = 15,
    Physics = 16,
}

pub const NUM_STATES: usize = 17;

impl HState {
    pub const ALL: [HState; NUM_STATES] = [
        HState::FallingDown,
        HState::Ragdoll,
        HState::GettingUp,
        HState::Jumping,
        HState::Swimming,
        HState::Freefall,
        HState::Flying,
        HState::Landed,
        HState::Running,
        HState::RunningSlave,
        HState::RunningNoPhysics,
        HState::StrafingNoPhysics,
        HState::Climbing,
        HState::Seated,
        HState::PlatformStanding,
        HState::Dead,
        HState::Physics,
    ];

    pub fn from_u32(v: u32) -> Option<HState> {
        HState::ALL.get(v as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            HState::FallingDown => "FallingDown",
            HState::Ragdoll => "Ragdoll",
            HState::GettingUp => "GettingUp",
            HState::Jumping => "Jumping",
            HState::Swimming => "Swimming",
            HState::Freefall => "Freefall",
            HState::Flying => "Flying",
            HState::Landed => "Landed",
            HState::Running => "Running",
            HState::RunningSlave => "RunningSlave",
            HState::RunningNoPhysics => "RunningNoPhysics",
            HState::StrafingNoPhysics => "StrafingNoPhysics",
            HState::Climbing => "Climbing",
            HState::Seated => "Seated",
            HState::PlatformStanding => "PlatformStanding",
            HState::Dead => "Dead",
            HState::Physics => "Physics",
        }
    }

    fn arms_collide(self) -> bool {
        !matches!(
            self,
            HState::Freefall
                | HState::Jumping
                | HState::GettingUp
                | HState::Running
                | HState::RunningSlave
                | HState::Landed
                | HState::Climbing
                | HState::RunningNoPhysics
                | HState::StrafingNoPhysics
                | HState::Seated
                | HState::PlatformStanding
        )
    }

    fn legs_collide(self) -> bool {
        self.arms_collide()
    }

    fn torso_collide(self) -> bool {
        self != HState::Jumping
    }

    fn enable_auto_jump(self) -> bool {
        !matches!(
            self,
            HState::Flying
                | HState::Jumping
                | HState::Freefall
                | HState::Dead
                | HState::FallingDown
                | HState::Physics
                | HState::Climbing
                | HState::GettingUp
                | HState::Ragdoll
                | HState::Swimming
                | HState::Seated
                | HState::PlatformStanding
        )
    }

    fn is_running_base(self) -> bool {
        matches!(
            self,
            HState::Running | HState::RunningSlave | HState::Landed | HState::Climbing
        )
    }

    fn is_no_physics(self) -> bool {
        matches!(self, HState::RunningNoPhysics | HState::StrafingNoPhysics)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum HEvent {
    NoHealth,
    NoNeck,
    JumpCmd,
    StrafeCmd,
    NoStrafeCmd,
    SitCmd,
    NoSitCmd,
    PlatformStandCmd,
    NoPlatformStandCmd,
    Tipped,
    Upright,
    FaceLadder,
    AwayLadder,
    OffFloor,
    OffFloorGrace,
    OnFloor,
    Touched,
    NearlyTouched,
    TouchedHard,
    ActivatePhysics,
    Finished,
    TimerUp,
    NoTouchOneSecond,
    HasGyro,
    HasBuoyancy,
    NoBuoyancy,
}

const EVENTS: [HEvent; 26] = [
    HEvent::NoHealth,
    HEvent::NoNeck,
    HEvent::JumpCmd,
    HEvent::StrafeCmd,
    HEvent::NoStrafeCmd,
    HEvent::SitCmd,
    HEvent::NoSitCmd,
    HEvent::PlatformStandCmd,
    HEvent::NoPlatformStandCmd,
    HEvent::Tipped,
    HEvent::Upright,
    HEvent::FaceLadder,
    HEvent::AwayLadder,
    HEvent::OffFloor,
    HEvent::OffFloorGrace,
    HEvent::OnFloor,
    HEvent::Touched,
    HEvent::NearlyTouched,
    HEvent::TouchedHard,
    HEvent::ActivatePhysics,
    HEvent::Finished,
    HEvent::TimerUp,
    HEvent::NoTouchOneSecond,
    HEvent::HasGyro,
    HEvent::HasBuoyancy,
    HEvent::NoBuoyancy,
];

use HState as S;
const X: Option<HState> = None;
const fn o(s: HState) -> Option<HState> {
    Some(s)
}

const TABLE: [[Option<HState>; 26]; NUM_STATES] = [
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::GettingUp),
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::GettingUp),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::GettingUp),
        X,
        X,
        X,
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        o(S::Running),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        X,
        X,
        o(S::Freefall),
        o(S::Freefall),
        X,
        X,
        X,
        X,
        X,
        o(S::Freefall),
        o(S::Freefall),
        X,
        X,
        X,
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Ragdoll),
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::GettingUp),
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        o(S::Climbing),
        X,
        X,
        X,
        o(S::Landed),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        o(S::FallingDown),
        X,
        o(S::Climbing),
        X,
        X,
        X,
        o(S::Running),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        o(S::FallingDown),
        X,
        X,
        X,
        X,
        o(S::Freefall),
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Running),
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        o(S::FallingDown),
        X,
        o(S::Climbing),
        X,
        X,
        o(S::Freefall),
        X,
        X,
        X,
        o(S::Ragdoll),
        X,
        X,
        X,
        o(S::RunningNoPhysics),
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        o(S::FallingDown),
        X,
        o(S::Climbing),
        X,
        X,
        o(S::Freefall),
        X,
        X,
        X,
        o(S::Ragdoll),
        X,
        X,
        X,
        o(S::RunningNoPhysics),
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        o(S::StrafingNoPhysics),
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        o(S::Climbing),
        X,
        X,
        o(S::Freefall),
        X,
        o(S::Running),
        o(S::Running),
        X,
        o(S::Running),
        X,
        X,
        X,
        o(S::Running),
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        o(S::RunningNoPhysics),
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        X,
        X,
        o(S::Climbing),
        X,
        X,
        o(S::Freefall),
        X,
        o(S::Running),
        o(S::Running),
        X,
        o(S::Running),
        X,
        X,
        X,
        o(S::Running),
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        o(S::Seated),
        X,
        o(S::PlatformStanding),
        X,
        o(S::FallingDown),
        X,
        X,
        o(S::Running),
        o(S::Climbing),
        o(S::Climbing),
        o(S::Climbing),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        o(S::Swimming),
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        X,
        o(S::Running),
        o(S::PlatformStanding),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        o(S::Jumping),
        X,
        X,
        o(S::Seated),
        X,
        X,
        o(S::Running),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
    ],
    [
        X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    ],
    [
        o(S::Dead),
        o(S::Dead),
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
        X,
    ],
];

#[derive(Clone, Debug)]
pub enum HumanoidEvent {
    StateChanged(HState, HState),
    Running(f32),
    Climbing(f32),
    Swimming(f32),
    Strafing(bool),
    FallingDown(bool),
    Ragdoll(bool),
    GettingUp(bool),
    Jumping(bool),
    FreeFalling(bool),
    Seated(bool, Option<InstanceId>),
    PlatformStanding(bool),
    Died,
    MoveToFinished(bool),
}

#[derive(Clone, Copy, Debug, Default)]
struct Appendages {
    root: Option<InstanceId>,
    torso: Option<InstanceId>,
    head: Option<InstanceId>,
    right_arm: Option<InstanceId>,
    left_arm: Option<InstanceId>,
    right_leg: Option<InstanceId>,
    left_leg: Option<InstanceId>,
}

impl Appendages {
    fn torso_slow(&self) -> Option<InstanceId> {
        self.root.or(self.torso)
    }
}

#[derive(Clone, Copy, Debug)]
struct Floor {
    part: InstanceId,
    touch: Vec3,
    normal: Vec3,
    location: Vec3,
}

#[derive(Clone, Debug)]
pub struct HumanoidSim {
    pub id: InstanceId,
    pub state: HState,
    prior_state: HState,
    pub previous_state: HState,
    pub lua_state: Option<HState>,
    timer: f32,
    no_touch_timer: f32,
    no_floor_timer: f32,
    finished: bool,
    out_of_water: bool,
    floor: Option<Floor>,
    pub last_floor_normal: Vec3,
    facing_ladder: bool,
    ladder_check: i32,
    last_movement_velocity: f32,
    balance_p: f32,
    balance_d: f32,
    balance_tick: i32,
    last_balance_torque: Vec3,
    desired_lin: Vec3,
    desired_rot: Vec3,
    floor_lin: Vec3,
    floor_rot: Vec3,
    desired_altitude: f32,
    initial_linear_velocity: Vec3,
    jump_dir: Vec3,
    last_force: Vec3,
    last_torque: Vec3,
    pub touched_hard: bool,
    pub impact_peak: f32,
    pub impact_hit: bool,
    pub activate_physics: bool,
    pub activate_impulse: Vec3,
    pub is_walking: bool,
    pub walk_timer: f64,
    pub percent_walk_speed: f32,
    pub move_direction: Vec3,
    pub enabled: [bool; NUM_STATES],
    had_neck: bool,
    had_health: bool,
    num_contacts: usize,
    saved_friction: Vec<(InstanceId, f32)>,
    app: Appendages,
    pub owned_locally: bool,
    entered: bool,
    pub jump_power_cache: f32,
    pub auto_jump: bool,
    pub click_to_walk: bool,
    pub player_character: bool,
}

fn heading(v: Vec3) -> f32 {
    (-v.x).atan2(-v.z)
}

fn rad_wrap(a: f32) -> f32 {
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

fn safe_direction(v: Vec3) -> Vec3 {
    v.normalize_or_zero()
}

const MIN_MOVE_VELOCITY: f32 = 0.5;
const MAX_CLIMB_DISTANCE: f32 = 2.45;
const MAX_MOVE_FORCE: Vec3 = Vec3::new(1000.0, 10000.0, 1000.0);
const MIN_MOVE_FORCE: Vec3 = Vec3::new(-1000.0, 0.0, -1000.0);
const MAX_SWIM_FORCE: Vec3 = Vec3::new(10000.0, 1000.0, 10000.0);
const MIN_SWIM_FORCE: Vec3 = Vec3::new(-10000.0, -10000.0, -10000.0);
const FALL_DELAY: f32 = 0.125;
const MAX_LINEAR_MOVE_FORCE: f32 = 143.0;
const MAX_LINEAR_GROUND_MOVE_FORCE: f32 = 500.0;
const RUNNING_KMOVE_P_PGS: f32 = 150.0;
const K_ALTITUDE_P: f32 = 30000.0;
const K_ALTITUDE_D: f32 = 1100.0;
const MAX_TORQUE_COMPONENT: f32 = 4000.0;
const K_TURN_P_ROTATE_PGS: f32 = 450.0;
const K_TURN_P_FREEFALL_PGS: f32 = 375.0;
const K_TURN_SPEED_PGS: f32 = 8.0;
const K_TURN_ACCEL_MAX: f32 = 20000.0 * 6.0;
const AUTO_TURN_SPEED: f32 = 8.0;
const K_JUMP_P: f32 = 500.0;
const PGS_JUMP_FORCE_ADJUSTMENT: f32 = 0.52;
const RAGDOLL_CRITERIA: f32 = 34.0;

impl HumanoidSim {
    pub fn new(id: InstanceId) -> HumanoidSim {
        HumanoidSim {
            id,
            state: HState::Running,
            prior_state: HState::RunningNoPhysics,
            previous_state: HState::Running,
            lua_state: None,
            timer: 0.0,
            no_touch_timer: 0.0,
            no_floor_timer: 0.0,
            finished: false,
            out_of_water: false,
            floor: None,
            last_floor_normal: Vec3::Y,
            facing_ladder: false,
            ladder_check: 0,
            last_movement_velocity: 0.0,
            balance_p: 2250.0,
            balance_d: 50.0,
            balance_tick: -1,
            last_balance_torque: Vec3::ZERO,
            desired_lin: Vec3::ZERO,
            desired_rot: Vec3::ZERO,
            floor_lin: Vec3::ZERO,
            floor_rot: Vec3::ZERO,
            desired_altitude: f32::INFINITY,
            initial_linear_velocity: Vec3::ZERO,
            jump_dir: Vec3::Y,
            last_force: Vec3::ZERO,
            last_torque: Vec3::ZERO,
            touched_hard: false,
            impact_peak: 0.0,
            impact_hit: false,
            activate_physics: false,
            activate_impulse: Vec3::ZERO,
            is_walking: false,
            walk_timer: 0.0,
            percent_walk_speed: 1.0,
            move_direction: Vec3::ZERO,
            enabled: [true; NUM_STATES],
            had_neck: false,
            had_health: false,
            num_contacts: 0,
            saved_friction: Vec::new(),
            app: Appendages::default(),
            owned_locally: false,
            entered: false,
            jump_power_cache: 50.0,
            auto_jump: false,
            click_to_walk: true,
            player_character: false,
        }
    }

    pub fn set_walk_mode(&mut self, walking: bool) {
        self.walk_timer = if walking { 8.0 } else { 0.0 };
        self.is_walking = walking;
    }
}

struct Props {
    health: f32,
    walk_speed: f32,
    jump_power: f32,
    jump: bool,
    sit: bool,
    platform_stand: bool,
    auto_rotate: bool,
    hip_height: f32,
    max_slope_angle: f32,
    walk_direction: Vec3,
    walk_to_point: Vec3,
    walk_to_part: Option<InstanceId>,
    seat_part: Option<InstanceId>,
    auto_jump_enabled: bool,
}

fn read_props(dm: &DataModel, id: InstanceId) -> Props {
    let f = |n: &str, d: f32| dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(d);
    let b = |n: &str| dm.get_bool(id, n);
    let v = |n: &str| {
        dm.get(id, n)
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ZERO)
    };
    Props {
        health: f("Health", 100.0),
        walk_speed: f("WalkSpeed", 16.0),
        jump_power: f("JumpPower", 50.0),
        jump: b("Jump"),
        sit: b("Sit"),
        platform_stand: b("PlatformStand"),
        auto_rotate: b("AutoRotate"),
        hip_height: f("HipHeight", 0.0),
        max_slope_angle: f("MaxSlopeAngle", 89.0),
        walk_direction: v("WalkDirection"),
        walk_to_point: v("WalkToPoint"),
        walk_to_part: dm.get_ref(id, "WalkToPart"),
        seat_part: dm.get_ref(id, "SeatPart"),
        auto_jump_enabled: b("AutoJumpEnabled"),
    }
}

pub enum DmWrite {
    Set(InstanceId, &'static str, Variant),
    Silent(InstanceId, &'static str, Variant),
    BreakJoints(InstanceId),
}

impl PhysicsWorld {
    pub fn upsert_humanoid(&mut self, dm: &DataModel, id: InstanceId) {
        let in_ws = self.workspace.is_some_and(|ws| dm.is_descendant_of(id, ws));
        let parent_model = dm.parent_of(id).filter(|p| dm.is_a(*p, "Model"));
        if in_ws && parent_model.is_some() {
            self.humanoids
                .entry(id)
                .or_insert_with(|| HumanoidSim::new(id));
        } else {
            self.humanoids.remove(&id);
        }
    }

    fn character_of(dm: &DataModel, id: InstanceId) -> Option<InstanceId> {
        dm.parent_of(id).filter(|p| dm.is_a(*p, "Model"))
    }

    fn find_appendages(dm: &DataModel, character: InstanceId) -> Appendages {
        let get = |name: &str| {
            dm.find_first_child(character, name, false)
                .filter(|c| dm.is_a(*c, "BasePart"))
        };
        let r15 = get("UpperTorso").is_some() && get("Torso").is_none();
        if r15 {
            Appendages {
                root: get("HumanoidRootPart"),
                torso: get("UpperTorso"),
                head: get("Head"),
                right_arm: get("RightArm"),
                left_arm: get("LeftArm"),
                right_leg: get("RightLeg"),
                left_leg: get("LeftLeg"),
            }
        } else {
            Appendages {
                root: get("HumanoidRootPart"),
                torso: get("Torso"),
                head: get("Head"),
                right_arm: get("Right Arm"),
                left_arm: get("Left Arm"),
                right_leg: get("Right Leg"),
                left_leg: get("Left Leg"),
            }
        }
    }

    fn has_neck(&self, app: &Appendages) -> bool {
        let (Some(head), Some(torso)) = (app.head, app.torso) else {
            return false;
        };
        self.part_joints.get(&head).is_some_and(|v| {
            v.iter().any(|jid| {
                self.joints.get(jid).is_some_and(|j| {
                    ((j.part0 == Some(head) && j.part1 == Some(torso))
                        || (j.part0 == Some(torso) && j.part1 == Some(head)))
                        && self.active_joint(j).is_some()
                })
            })
        })
    }

    fn part_size(&self, id: Option<InstanceId>) -> Option<Vec3> {
        id.and_then(|p| self.part(p)).map(|p| p.shape.size)
    }

    fn part_cf(&self, id: Option<InstanceId>) -> Option<CFrame> {
        id.and_then(|p| self.part(p)).map(|p| p.cframe)
    }

    fn assembly_index(&self, id: InstanceId) -> Option<usize> {
        self.part(id)
            .map(|p| p.assembly)
            .filter(|a| *a < self.assemblies.len())
    }

    fn point_velocity(&self, id: InstanceId, at: Vec3) -> (Vec3, Vec3) {
        match self.assembly_index(id) {
            Some(a) => {
                let asm = &self.assemblies[a];
                if asm.anchored {
                    let p = self
                        .part(id)
                        .map(|p| (p.velocity, p.rot_velocity))
                        .unwrap_or_default();
                    (
                        p.0 + p
                            .1
                            .cross(at - self.part(id).map(|p| p.cframe.translation).unwrap_or(at)),
                        p.1,
                    )
                } else {
                    (asm.velocity_at(at), asm.angular)
                }
            }
            None => (Vec3::ZERO, Vec3::ZERO),
        }
    }

    fn hit_filter_assembly(&self, own: usize) -> impl Fn(InstanceId) -> bool + '_ {
        move |pid| match self.part(pid) {
            Some(p) => p.can_collide && p.assembly != own,
            None => self.terrain.id == Some(pid),
        }
    }

    fn character_hip_height(&self, h: &HumanoidSim, props: &Props) -> f32 {
        let mut height = 0.0;
        if let Some(s) = self.part_size(h.app.torso_slow()) {
            height += 0.5 * s.y;
        }
        if let Some(s) = self
            .part_size(h.app.left_leg)
            .or_else(|| self.part_size(h.app.right_leg))
        {
            height += s.y;
        }
        let _ = props.hip_height;
        height
    }

    fn find_floor(&self, h: &mut HumanoidSim, old_floor: Option<InstanceId>) {
        let Some(torso) = h.app.torso_slow() else {
            return;
        };
        let Some(tp) = self.part(torso) else {
            return;
        };
        let torso_c = tp.cframe;
        let half = tp.shape.size * 0.4;
        let mut max_distance = 1.0;
        let mut hysteresis = if old_floor.is_some() { 1.5 } else { 1.1 };
        let own = tp.assembly;
        let vy = self
            .assemblies
            .get(own)
            .map_or(0.0, |a| a.velocity_at(torso_c.translation).y.abs());
        if vy > 100.0 {
            hysteresis += vy / 100.0;
        }
        if let Some(s) = self
            .part_size(h.app.left_leg)
            .or_else(|| self.part_size(h.app.right_leg))
        {
            max_distance += hysteresis * s.y;
        }
        let filter = self.hit_filter_assembly(own);
        let mut acc = Vec3::ZERO;
        let mut count = 0;
        let mut found: Option<Floor> = None;
        let mut cast = |offset: Vec3, update: bool, found: &mut Option<Floor>| {
            let origin = torso_c * offset;
            if let Some(hit) = self.raycast(origin, Vec3::new(0.0, -max_distance, 0.0), &filter) {
                acc += hit.position;
                count += 1;
                if update && found.is_none() {
                    *found = Some(Floor {
                        part: hit.part,
                        touch: hit.position,
                        normal: hit.normal,
                        location: Vec3::ZERO,
                    });
                }
            }
        };
        for z in [0.0f32, 1.0, -1.0, 2.0, -2.0] {
            cast(
                Vec3::new(0.0, -half.y, z * half.z),
                z.abs() < 2.0,
                &mut found,
            );
        }
        if found.is_none() {
            for x in [-1.0f32, 1.0] {
                for z in [-1.0f32, 1.0] {
                    cast(Vec3::new(x * half.x, -half.y, z * half.z), true, &mut found);
                }
            }
        }
        if let Some(mut f) = found {
            f.location = acc / count.max(1) as f32;
            h.floor = Some(f);
            h.no_floor_timer = 0.0;
        }
    }

    fn floor_part_valid(&self, h: &HumanoidSim) -> Option<Floor> {
        let f = h.floor?;
        if self.terrain.id == Some(f.part) {
            return Some(f);
        }
        let own = h.app.torso_slow().and_then(|t| self.assembly_index(t))?;
        let fa = self.assembly_index(f.part)?;
        if fa != own { Some(f) } else { None }
    }

    fn floor_point_velocity(&self, h: &HumanoidSim) -> (Vec3, Vec3) {
        match self.floor_part_valid(h) {
            Some(f) => self.point_velocity(f.part, f.touch),
            None => (Vec3::ZERO, Vec3::ZERO),
        }
    }

    fn relative_movement_velocity(&self, h: &HumanoidSim) -> Vec3 {
        let Some(torso) = h.app.torso_slow() else {
            return Vec3::ZERO;
        };
        let Some(p) = self.part(torso) else {
            return Vec3::ZERO;
        };
        let walk = self.point_velocity(torso, p.cframe.translation).0;
        walk - self.floor_point_velocity(h).0
    }

    fn torso_heading(&self, h: &HumanoidSim) -> f32 {
        self.part_cf(h.app.torso_slow())
            .map_or(0.0, |c| heading(c.look_vector()))
    }

    fn delta_to_goal(&self, h: &HumanoidSim, dm: &DataModel, props: &Props) -> Vec3 {
        if !h.is_walking || (h.player_character && !h.click_to_walk) {
            return Vec3::ZERO;
        }
        let Some(torso) = self.part_cf(h.app.torso_slow()) else {
            return Vec3::ZERO;
        };
        let world = match props.walk_to_part {
            Some(part) => {
                if !self
                    .workspace
                    .is_some_and(|ws| dm.is_descendant_of(part, ws))
                {
                    return Vec3::ZERO;
                }
                let cf = dm
                    .get_raw(part, "CFrame")
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default();
                cf * props.walk_to_point
            }
            None => props.walk_to_point,
        };
        let mut answer = world - torso.translation + Vec3::new(0.0, 3.0, 0.0);
        answer.y = 0.0;
        if answer.length() < 0.3 {
            return Vec3::ZERO;
        }
        answer
    }

    fn calc_desired_walk_velocity(
        &self,
        h: &HumanoidSim,
        dm: &DataModel,
        props: &Props,
    ) -> (Vec3, Vec3) {
        let mut intended = props.walk_direction;
        if intended == Vec3::ZERO {
            intended = h.move_direction;
        } else {
            if h.state != HState::Swimming {
                intended.y = 0.0;
            }
            intended = safe_direction(intended);
        }
        if h.is_walking && (!h.player_character || h.click_to_walk) {
            let goal = self.delta_to_goal(h, dm, props);
            intended = if goal == Vec3::ZERO {
                Vec3::ZERO
            } else {
                safe_direction(goal)
            };
        }
        let lin = intended * props.walk_speed * h.percent_walk_speed;
        let mut rot = Vec3::ZERO;
        if props.auto_rotate && lin != Vec3::ZERO {
            let delta = rad_wrap(heading(lin) - self.torso_heading(h));
            rot.y = AUTO_TURN_SPEED * delta;
        }
        (lin, rot)
    }

    fn computed_touched(&mut self, h: &mut HumanoidSim, dm: &DataModel) -> bool {
        let Some(character) = Self::character_of(dm, h.id) else {
            return false;
        };
        for part in [h.app.torso, h.app.head].into_iter().flatten() {
            let touching = self.contacts.values().any(|c| {
                c.overlapping && (c.a == part || c.b == part) && {
                    let other = if c.a == part { c.b } else { c.a };
                    !dm.is_descendant_of(other, character)
                }
            });
            if touching {
                h.no_touch_timer = 0.0;
                return true;
            }
        }
        false
    }

    fn compute_nearly_touched(&self, h: &mut HumanoidSim) -> bool {
        let Some(torso) = h.app.torso else {
            return false;
        };
        let near = self
            .contacts
            .values()
            .filter(|c| (c.a == torso || c.b == torso) && !c.overlapping)
            .count();
        let r = near > h.num_contacts;
        h.num_contacts = near;
        if r {
            h.no_touch_timer = 0.0;
        }
        r
    }

    fn compute_tilt(&self, h: &HumanoidSim) -> f32 {
        self.part_cf(h.app.torso_slow())
            .map_or(1.0, |c| c.vector_to_object_space(Vec3::Y).y)
    }

    fn compute_floor_tilt(&self, h: &HumanoidSim) -> f32 {
        match self.floor_part_valid(h) {
            Some(f) => f.normal.dot(Vec3::Y),
            None => h.last_floor_normal.dot(Vec3::Y),
        }
    }

    fn steep_slope(props: &Props) -> f32 {
        (props.max_slope_angle.clamp(0.0, 89.9).to_radians()).cos()
    }

    fn has_gyro(dm: &DataModel, character: Option<InstanceId>) -> bool {
        character.is_some_and(|c| {
            dm.find_first_descendant_which_is_a(c, "BodyMover")
                .is_some()
        })
    }

    fn compute_event(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        e: HEvent,
    ) -> bool {
        match e {
            HEvent::HasBuoyancy => h
                .app
                .torso
                .is_some_and(|t| self.terrain.buoyant.contains(&t)),
            HEvent::NoBuoyancy => !h
                .app
                .torso
                .is_some_and(|t| self.terrain.buoyant.contains(&t)),
            HEvent::NoHealth => props.health <= 0.0,
            HEvent::NoNeck => !self.has_neck(&h.app),
            HEvent::JumpCmd => {
                (self.compute_floor_tilt(h) > Self::steep_slope(props)
                    || !matches!(h.state, HState::Running | HState::RunningNoPhysics))
                    && props.jump
                    && props.jump_power > 0.0
            }
            HEvent::SitCmd => props.sit,
            HEvent::NoSitCmd => !props.sit,
            HEvent::PlatformStandCmd => props.platform_stand,
            HEvent::NoPlatformStandCmd => !props.platform_stand,
            HEvent::Tipped => self.compute_tilt(h) < 0.7 && !h.out_of_water,
            HEvent::Upright => self.compute_tilt(h) > 0.9,
            HEvent::FaceLadder => h.facing_ladder,
            HEvent::AwayLadder => !h.facing_ladder,
            HEvent::StrafeCmd => false,
            HEvent::NoStrafeCmd => true,
            HEvent::OnFloor => h.floor.is_some() && self.relative_movement_velocity(h).y <= 0.0,
            HEvent::OffFloor => h.floor.is_none(),
            HEvent::OffFloorGrace => h.no_floor_timer > FALL_DELAY,
            HEvent::Touched => self.computed_touched(h, dm),
            HEvent::NearlyTouched => self.compute_nearly_touched(h),
            HEvent::TouchedHard => h.touched_hard,
            HEvent::ActivatePhysics => h.activate_physics,
            HEvent::TimerUp => h.timer <= 0.0,
            HEvent::NoTouchOneSecond => {
                self.computed_touched(h, dm);
                self.compute_nearly_touched(h);
                h.no_touch_timer > 1.0 && !Self::has_gyro(dm, Self::character_of(dm, h.id))
            }
            HEvent::Finished => h.finished,
            HEvent::HasGyro => Self::has_gyro(dm, Self::character_of(dm, h.id)),
        }
    }

    fn uses_event(state: HState, e: HEvent) -> bool {
        TABLE[state as usize][e as usize].is_some()
    }

    fn set_collide(&mut self, part: Option<InstanceId>, value: bool, writes: &mut Vec<DmWrite>) {
        if let Some(p) = part
            && let Some(&i) = self.index.get(&p)
            && self.parts[i].can_collide != value
        {
            self.parts[i].can_collide = value;
            writes.push(DmWrite::Silent(p, "CanCollide", Variant::Bool(value)));
        }
    }

    fn pre_step_collide(&mut self, h: &HumanoidSim, writes: &mut Vec<DmWrite>) {
        let arms = h.state.arms_collide();
        let legs = h.state.legs_collide();
        self.set_collide(h.app.left_leg, legs, writes);
        self.set_collide(h.app.right_leg, legs, writes);
        self.set_collide(h.app.left_arm, arms, writes);
        self.set_collide(h.app.right_arm, arms, writes);
        self.set_collide(h.app.head, true, writes);
        self.set_collide(h.app.torso, h.state.torso_collide(), writes);
    }

    fn pre_step_floor(&mut self, h: &mut HumanoidSim) {
        let old = h.floor.map(|f| f.part);
        h.floor = None;
        let uses_floor = Self::uses_event(h.state, HEvent::OffFloor)
            || Self::uses_event(h.state, HEvent::OnFloor)
            || Self::uses_event(h.state, HEvent::OffFloorGrace);
        if uses_floor {
            self.find_floor(h, old);
        }
        if let Some(f) = h.floor {
            h.last_floor_normal = f.normal;
            if old != Some(f.part) {
                self.wake_part(f.part);
            }
        }
    }

    fn pre_step_simulator(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        dt: f32,
        writes: &mut Vec<DmWrite>,
    ) {
        self.pre_step_floor(h);
        h.timer -= dt;
        h.no_touch_timer += dt;
        h.no_floor_timer += dt;
        self.pre_step_collide(h, writes);
        h.facing_ladder = false;
        if Self::uses_event(h.state, HEvent::FaceLadder)
            || Self::uses_event(h.state, HEvent::AwayLadder)
        {
            if h.ladder_check <= 0 {
                h.facing_ladder = self.find_ladder(h, dm, props);
                h.ladder_check = 2;
            } else {
                h.ladder_check -= 1;
            }
        }
        if !h.facing_ladder && h.auto_jump && h.state.enable_auto_jump() {
            let (wl, _) = self.calc_desired_walk_velocity(h, dm, props);
            if wl != Vec3::ZERO {
                self.do_auto_jump(h, dm, props, writes);
            }
        }
    }

    fn do_auto_jump(
        &self,
        h: &HumanoidSim,
        dm: &DataModel,
        props: &Props,
        writes: &mut Vec<DmWrite>,
    ) {
        if !props.auto_jump_enabled {
            return;
        }
        let Some(torso) = h.app.torso_slow() else {
            return;
        };
        let Some(tp) = self.part(torso) else {
            return;
        };
        let Some(character) = dm.parent_of(torso) else {
            return;
        };
        let cf = tp.cframe;
        let look = cf.look_vector();
        let filter = |p: InstanceId| !dm.is_descendant_of(p, character) && p != character;
        let low = self.raycast(
            cf.translation + Vec3::new(0.0, -tp.shape.size.y / 2.0, 0.0),
            look * 1.5,
            filter,
        );
        let high = self.raycast(
            cf.translation + Vec3::new(0.0, 7.0 - tp.shape.size.y, 0.0),
            look * 1.5,
            filter,
        );
        if let Some(hit) = low
            && high.is_none()
            && self.part(hit.part).is_some_and(|p| p.can_collide)
        {
            writes.push(DmWrite::Set(h.id, "Jump", Variant::Bool(true)));
        }
    }

    fn find_ladder(&self, h: &HumanoidSim, dm: &DataModel, props: &Props) -> bool {
        if !h.enabled[HState::Climbing as usize] {
            return false;
        }
        let Some(torso_id) = h.app.torso_slow() else {
            return false;
        };
        let Some(character) = Self::character_of(dm, h.id) else {
            return false;
        };
        let Some(cf) = self.part_cf(Some(torso_id)) else {
            return false;
        };
        let r15 = dm
            .find_first_child(character, "UpperTorso", false)
            .is_some();
        let search_depth = if r15 { 1.2 } else { 0.7 };
        let search_distance = search_depth * 1.5;
        let low = 2.7f32;
        let radius = 0.5 * search_distance;
        let center = cf.translation + cf.look_vector() * search_distance * 0.5;
        let lo = center + Vec3::new(-radius, -low, -radius);
        let hi = center + Vec3::new(radius, 0.0, radius);
        let near = self.parts_in_region(lo, hi, |p| !dm.is_descendant_of(p, character), usize::MAX);
        if near.is_empty() {
            return false;
        }
        let own = self.assembly_index(torso_id).unwrap_or(usize::MAX);
        let filter = |p: InstanceId| {
            if dm.is_descendant_of(p, character) {
                return false;
            }
            if dm
                .parent_of(p)
                .is_some_and(|par| dm.find_first_child_of_class(par, "Humanoid").is_some())
            {
                return false;
            }
            self.part(p)
                .is_some_and(|pp| pp.can_collide && pp.assembly != own)
        };
        let height_scale = self.character_hip_height(h, props) / 3.0;
        let spacing = (1.0 / 7.0) * height_scale;
        if spacing <= 0.0 {
            return false;
        }
        let top_ray = (low / spacing) as i32 + 10;
        let mut first_space = 0.0f32;
        let mut first_step = 0.0f32;
        let mut second_step = -1.0f32;
        let mut look_for_space = true;
        let mut look_for_step = false;
        for i in 1..top_ray {
            let from_bottom = i as f32 * spacing;
            let origin = cf.translation + Vec3::new(0.0, -low + from_bottom, 0.0);
            let dir = cf.look_vector() * search_distance;
            let hit = self.raycast(origin, dir, filter);
            if let Some(hp) = hit
                && dm.class_of(hp.part) == Some("TrussPart")
            {
                return true;
            }
            let mag = hit.map_or(search_distance, |hp| (hp.position - origin).length());
            if hit.is_some() && mag < search_depth {
                if look_for_space {
                    first_space = from_bottom;
                    look_for_space = false;
                    look_for_step = true;
                } else if !look_for_step && second_step < 0.0 {
                    second_step = from_bottom;
                }
            } else if look_for_step {
                first_step = from_bottom - first_space;
                look_for_step = false;
            }
        }
        first_space < MAX_CLIMB_DISTANCE * height_scale
            && first_step > 0.0
            && first_step < MAX_CLIMB_DISTANCE * height_scale
            && (second_step >= 0.0 || first_space > MAX_CLIMB_DISTANCE * height_scale / 5.0)
    }

    fn enter_state(
        &mut self,
        h: &mut HumanoidSim,
        new: HState,
        old: HState,
        dm: &DataModel,
        writes: &mut Vec<DmWrite>,
    ) {
        self.leave_state(h, writes);
        h.previous_state = old;
        h.state = new;
        h.timer = 0.0;
        h.no_touch_timer = 0.0;
        h.no_floor_timer = 0.0;
        h.finished = false;
        h.out_of_water = false;
        h.floor = None;
        h.last_movement_velocity = 0.0;
        h.lua_state = None;
        h.balance_p = 2250.0;
        h.balance_d = 50.0;
        h.balance_tick = -1;
        h.last_balance_torque = Vec3::ZERO;
        h.desired_lin = Vec3::ZERO;
        h.desired_rot = Vec3::ZERO;
        h.desired_altitude = f32::INFINITY;
        h.jump_dir = Vec3::Y;
        writes.push(DmWrite::Set(h.id, "Jump", Variant::Bool(false)));
        match new {
            HState::FallingDown => h.timer = 3.0,
            HState::Ragdoll => h.timer = 8.0,
            HState::Landed => {
                h.balance_p = 7500.0;
                h.balance_d = 50.0;
                h.timer = 0.05;
            }
            HState::Freefall | HState::Flying => {
                h.balance_p = 5000.0;
                if new == HState::Freefall {
                    for part in [h.app.head, h.app.torso_slow()].into_iter().flatten() {
                        if let Some(&i) = self.index.get(&part) {
                            let f = self.parts[i].legacy_friction;
                            h.saved_friction.push((part, f));
                            self.parts[i].legacy_friction = 0.0;
                        }
                    }
                }
            }
            HState::Jumping => {
                h.balance_p = 5000.0;
                if old == HState::Swimming {
                    h.out_of_water = true;
                } else if old == HState::Climbing
                    && let Some(cf) = self.part_cf(h.app.torso_slow())
                {
                    let mut look = cf.look_vector();
                    look.y = 0.0;
                    h.jump_dir = (Vec3::Y - look.normalize_or_zero()).normalize_or(Vec3::Y);
                }
                h.timer = 0.5;
            }
            HState::GettingUp => {
                h.balance_p = 2250.0;
                h.balance_d = 50.0;
            }
            HState::Dead => {
                if let Some(root) = h.app.torso_slow()
                    && let Some(&i) = self.index.get(&root)
                {
                    self.parts[i].can_collide = true;
                    writes.push(DmWrite::Set(root, "CanCollide", Variant::Bool(true)));
                    self.parts[i].mass = 10.0;
                    self.topology_dirty = true;
                }
            }
            _ => {}
        }
        if h.state.is_running_base() {
            if h.activate_impulse != Vec3::ZERO
                && let Some(a) = h.app.torso_slow().and_then(|t| self.assembly_index(t))
            {
                let asm = &mut self.assemblies[a];
                if asm.mass > 0.0 {
                    asm.linear += h.activate_impulse / asm.mass;
                }
                h.activate_physics = false;
                h.activate_impulse = Vec3::ZERO;
            }
            let rel = self.relative_movement_velocity(h);
            let speed = Vec3::new(rel.x, 0.0, rel.z).length();
            self.fire_movement(h, speed, HumanoidEvent::Running);
        }
        if new == HState::RunningNoPhysics {
            let rel = self.relative_movement_velocity(h);
            let speed = Vec3::new(rel.x, 0.0, rel.z).length();
            self.humanoid_events
                .push((h.id, HumanoidEvent::Running(speed)));
        }
        if new == HState::Swimming {
            self.humanoid_events
                .push((h.id, HumanoidEvent::Swimming(0.0)));
        }
        self.set_kinematic(h, new.is_no_physics());
        let _ = dm;
    }

    fn leave_state(&mut self, h: &mut HumanoidSim, writes: &mut Vec<DmWrite>) {
        match h.state {
            HState::Freefall => {
                for (part, f) in h.saved_friction.drain(..) {
                    if f > 0.0
                        && let Some(&i) = self.index.get(&part)
                    {
                        self.parts[i].legacy_friction = f;
                    }
                }
            }
            HState::Ragdoll => h.touched_hard = false,
            HState::Seated => {
                writes.push(DmWrite::Set(h.id, "SeatPart", Variant::Ref(None)));
                writes.push(DmWrite::Set(h.id, "Sit", Variant::Bool(false)));
            }
            HState::PlatformStanding => {
                writes.push(DmWrite::Set(h.id, "PlatformStand", Variant::Bool(false)));
            }
            _ => {}
        }
        h.saved_friction.clear();
    }

    pub(crate) fn latch_humanoid_impacts(&mut self) {
        let ids: Vec<InstanceId> = self.humanoids.keys().copied().collect();
        for id in ids {
            let Some(mut h) = self.humanoids.remove(&id) else {
                continue;
            };
            if let Some(a) = h.app.torso_slow().and_then(|t| self.assembly_index(t)) {
                let force = self.spring_contact_peak(a);
                if force > h.impact_peak {
                    h.impact_peak = force;
                }
                if force > 0.0 && !h.impact_hit && self.hit_by_high_impact(&h) {
                    h.impact_hit = true;
                }
            }
            self.humanoids.insert(id, h);
        }
    }

    fn spring_contact_peak(&self, a: usize) -> f32 {
        let mut total = 0.0;
        let m1 = self.assemblies[a].mass;
        for c in self.contacts.values() {
            if !c.overlapping {
                continue;
            }
            let (Some(pa), Some(pb)) = (self.part(c.a), self.part(c.b)) else {
                continue;
            };
            let other = if pa.assembly == a {
                pb.assembly
            } else if pb.assembly == a {
                pa.assembly
            } else {
                continue;
            };
            if other == a {
                continue;
            }
            let o = &self.assemblies[other];
            let mu = if o.anchored || o.kinematic || o.mass <= 0.0 {
                m1
            } else {
                m1 * o.mass / (m1 + o.mass)
            };
            let k = joint_k(pa.shape.size, pa.shape.kind == ShapeKind::Ball)
                .min(joint_k(pb.shape.size, pb.shape.kind == ShapeKind::Ball));
            let pose_a = pa.cframe * pa.shape.local;
            let mut approach = 0.0f32;
            for m in &c.manifolds {
                let n = pose_a.rotation * crate::math::from_parry_vec(m.local_n1);
                if n.length_squared() < 0.8 {
                    continue;
                }
                let n = n.normalize();
                for point in &m.points {
                    let at = pose_a * crate::math::from_parry_vec(point.local_p1);
                    let v = n.dot(self.impact_velocity(c.a, at) - self.impact_velocity(c.b, at));
                    approach = approach.max(v);
                }
            }
            total += approach * (k * mu).sqrt();
        }
        total
    }

    pub(crate) fn restore_kinematic_humanoids(&mut self) {
        let torsos: Vec<InstanceId> = self
            .humanoids
            .values()
            .filter(|h| h.state.is_no_physics())
            .filter_map(|h| h.app.torso_slow())
            .collect();
        for t in torsos {
            if let Some(a) = self.assembly_index(t) {
                self.assemblies[a].kinematic = true;
            }
        }
    }

    fn set_kinematic(&mut self, h: &HumanoidSim, on: bool) {
        if let Some(a) = h.app.torso_slow().and_then(|t| self.assembly_index(t))
            && self.assemblies[a].kinematic != on
        {
            self.assemblies[a].kinematic = on;
            self.assemblies[a].sleeping = false;
        }
    }

    fn fire_movement(
        &mut self,
        h: &mut HumanoidSim,
        velocity: f32,
        make: impl Fn(f32) -> HumanoidEvent,
    ) {
        if h.app.torso_slow().is_none() {
            return;
        }
        let clamped = if velocity < MIN_MOVE_VELOCITY {
            0.0
        } else {
            velocity
        };
        let low = h.last_movement_velocity * 0.95;
        let high = h.last_movement_velocity * 1.05;
        if clamped < low || clamped > high {
            self.humanoid_events.push((h.id, make(clamped)));
            h.last_movement_velocity = clamped;
        }
    }

    fn fire_state_events(&mut self, h: &mut HumanoidSim, props: &Props) {
        if h.prior_state != h.state {
            let (prior, cur) = (h.prior_state, h.state);
            self.fire_state_event(h, prior, false, props);
            self.fire_state_event(h, cur, true, props);
            self.humanoid_events
                .push((h.id, HumanoidEvent::StateChanged(prior, cur)));
        }
        h.prior_state = h.state;
        let rel = self.relative_movement_velocity(h);
        match h.state {
            HState::Running
            | HState::RunningSlave
            | HState::RunningNoPhysics
            | HState::StrafingNoPhysics => {
                self.fire_movement(
                    h,
                    Vec3::new(rel.x, 0.0, rel.z).length(),
                    HumanoidEvent::Running,
                );
            }
            HState::Climbing => self.fire_movement(h, rel.y.abs(), HumanoidEvent::Climbing),
            HState::Swimming => self.fire_movement(h, rel.length(), HumanoidEvent::Swimming),
            _ => {}
        }
    }

    fn fire_state_event(
        &mut self,
        h: &mut HumanoidSim,
        state: HState,
        entering: bool,
        props: &Props,
    ) {
        let ev = match state {
            HState::Running | HState::RunningNoPhysics => {
                let rel = self.relative_movement_velocity(h);
                let speed = Vec3::new(rel.x, 0.0, rel.z).length();
                let clamped = if speed < MIN_MOVE_VELOCITY {
                    0.0
                } else {
                    speed
                };
                h.last_movement_velocity = clamped;
                HumanoidEvent::Running(clamped)
            }
            HState::Climbing => HumanoidEvent::Climbing(0.0),
            HState::StrafingNoPhysics => HumanoidEvent::Strafing(entering),
            HState::FallingDown => HumanoidEvent::FallingDown(entering),
            HState::Physics | HState::Ragdoll => HumanoidEvent::Ragdoll(entering),
            HState::GettingUp => HumanoidEvent::GettingUp(entering),
            HState::Jumping => HumanoidEvent::Jumping(entering),
            HState::Flying | HState::Landed | HState::Freefall => {
                HumanoidEvent::FreeFalling(entering)
            }
            HState::Seated => HumanoidEvent::Seated(entering, props.seat_part),
            HState::PlatformStanding => HumanoidEvent::PlatformStanding(entering),
            HState::Dead => HumanoidEvent::Died,
            HState::Swimming => HumanoidEvent::Swimming(0.0),
            HState::RunningSlave => return,
        };
        self.humanoid_events.push((h.id, ev));
    }

    fn do_state_table(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        dt: f32,
        writes: &mut Vec<DmWrite>,
    ) {
        let old = h.state;
        if let Some(ls) = h.lua_state
            && ls != old
        {
            self.enter_state(h, ls, old, dm, writes);
            self.pre_step_simulator(h, dm, props, dt, writes);
            return;
        }
        for e in EVENTS {
            let Some(new) = TABLE[old as usize][e as usize] else {
                continue;
            };
            if new != old && h.enabled[new as usize] && self.compute_event(h, dm, props, e) {
                self.enter_state(h, new, old, dm, writes);
                self.pre_step_simulator(h, dm, props, dt, writes);
                break;
            }
        }
    }

    pub fn humanoid_ui_step(&mut self, dm: &DataModel, long_step: bool, dt: f32) {
        let ids: Vec<InstanceId> = {
            let mut v: Vec<InstanceId> = self.humanoids.keys().copied().collect();
            v.sort();
            v
        };
        let mut writes = Vec::new();
        for id in ids {
            let Some(mut h) = self.humanoids.remove(&id) else {
                continue;
            };
            if !dm.contains(id) {
                continue;
            }
            if !h.owned_locally && !long_step {
                self.humanoids.insert(id, h);
                continue;
            }
            let step_dt = if h.owned_locally { dt } else { dt * 2.0 };
            let props = read_props(dm, id);
            if let Some(character) = Self::character_of(dm, id) {
                h.app = Self::find_appendages(dm, character);
            }
            if long_step && h.is_walking {
                h.walk_timer -= dt as f64 * 2.0;
                if h.walk_timer <= 0.0 {
                    h.set_walk_mode(false);
                    self.humanoid_events
                        .push((id, HumanoidEvent::MoveToFinished(false)));
                } else if self.delta_to_goal(&h, dm, &props).length_squared() < 1.0 {
                    h.set_walk_mode(false);
                    self.humanoid_events
                        .push((id, HumanoidEvent::MoveToFinished(true)));
                }
            }
            let torso_ok = h.app.torso_slow().is_some_and(|t| self.part(t).is_some());
            if !torso_ok {
                self.humanoids.insert(id, h);
                continue;
            }
            if !h.entered {
                h.entered = true;
                let init = h.state;
                self.enter_state(&mut h, init, HState::RunningNoPhysics, dm, &mut writes);
                h.prior_state = HState::RunningNoPhysics;
            }
            if self.has_neck(&h.app) {
                h.had_neck = true;
            }
            if props.health > 0.0 {
                h.had_health = true;
            }
            if let Some(a) = h.app.torso_slow().and_then(|t| self.assembly_index(t)) {
                let asm = &mut self.assemblies[a];
                if !asm.anchored {
                    asm.sleeping = false;
                }
            }
            self.pre_step_simulator(&mut h, dm, &props, step_dt, &mut writes);
            self.do_state_table(&mut h, dm, &props, step_dt, &mut writes);
            self.simulator_step(&mut h, dm, &props, step_dt, &mut writes);
            self.state_step(&mut h, dm, &props, &mut writes);
            h.activate_physics = false;
            h.activate_impulse = Vec3::ZERO;
            self.fire_state_events(&mut h, &props);
            self.humanoids.insert(id, h);
        }
        self.dm_writes.extend(writes);
    }

    fn state_step(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        writes: &mut Vec<DmWrite>,
    ) {
        match h.state {
            HState::Dead => {
                if dm.find_service("NetworkClient").is_none() && props.health != 0.0 {
                    writes.push(DmWrite::Set(h.id, "Health", Variant::Float(0.0)));
                }
            }
            HState::Ragdoll => {
                if h.timer <= 7.0
                    && let Some(t) = h.app.torso_slow()
                    && let Some(p) = self.part(t)
                {
                    let (v, w) = self.point_velocity(t, p.cframe.translation);
                    if v.length() < 1.0 && w.length() < 1.0 {
                        h.timer = 0.0;
                    }
                }
            }
            _ => {}
        }
    }

    fn simulator_step(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        dt: f32,
        writes: &mut Vec<DmWrite>,
    ) {
        match h.state {
            HState::Dead => {
                if h.had_neck
                    && h.had_health
                    && let Some(c) = Self::character_of(dm, h.id)
                {
                    writes.push(DmWrite::BreakJoints(c));
                }
            }
            HState::Freefall | HState::Swimming => {
                let (wl, wr) = self.calc_desired_walk_velocity(h, dm, props);
                h.desired_lin = wl + h.initial_linear_velocity;
                h.desired_rot = wr;
                if h.desired_lin.length() < 0.1 {
                    h.desired_lin = Vec3::ZERO;
                    h.desired_rot = Vec3::ZERO;
                } else if h.state == HState::Freefall {
                    h.desired_rot = Vec3::ZERO;
                    if wl.length() > 0.1 && props.auto_rotate {
                        let err = rad_wrap(heading(h.desired_lin) - self.torso_heading(h));
                        h.desired_rot.y = if err.abs() > 0.2 {
                            K_TURN_SPEED_PGS * err.signum()
                        } else {
                            0.25 * K_TURN_SPEED_PGS * err.signum() * err.abs()
                        };
                    }
                }
                h.initial_linear_velocity *= 1.0 - (0.05 * dt / (1.0 / 30.0));
            }
            s if s.is_running_base() => {
                let floor = self.floor_part_valid(h);
                h.desired_altitude = if floor.is_some() {
                    floor.map_or(f32::INFINITY, |f| f.location.y)
                        + self.character_hip_height(h, props)
                } else {
                    f32::INFINITY
                };
                let (fl, fr) = self.floor_point_velocity(h);
                h.floor_lin = fl;
                h.floor_rot = fr;
                if floor.is_some() || s == HState::Climbing || h.previous_state == HState::Climbing
                {
                    let (l, r) = self.calc_desired_walk_velocity(h, dm, props);
                    h.desired_lin = l;
                    h.desired_rot = r;
                }
                if h.facing_ladder {
                    let mut fwd = self
                        .part_cf(h.app.torso_slow())
                        .map_or(Vec3::NEG_Z, |c| c.look_vector());
                    fwd.y = 0.0;
                    let fwd = fwd.normalize_or_zero();
                    let dot = fwd.dot(h.desired_lin.normalize_or_zero());
                    let move_dir = if dot < -0.2 { -1.0 } else { 1.0 };
                    if move_dir > 0.0 || floor.is_none() {
                        let speed = h.desired_lin.length();
                        h.desired_lin = Vec3::new(
                            0.0,
                            if speed < 0.1 {
                                0.01 * move_dir
                            } else {
                                0.7 * speed * move_dir
                            },
                            0.0,
                        );
                        h.desired_rot = Vec3::ZERO;
                    }
                }
                if let Some(f) = floor {
                    let n = f.normal;
                    let dot = n.dot(Vec3::Y);
                    if dot < Self::steep_slope(props) {
                        let down = -(Vec3::Y - n * dot);
                        if down.length() > 1e-5 {
                            let down = down.normalize();
                            let onto = h.desired_lin - n * h.desired_lin.dot(n);
                            h.desired_lin = onto + down * props.walk_speed - down * onto.dot(down);
                        }
                    }
                }
            }
            s if s.is_no_physics() => self.moving_no_physics_step(h, dm, props, dt),
            _ => {}
        }
    }

    fn moving_no_physics_step(
        &mut self,
        h: &mut HumanoidSim,
        dm: &DataModel,
        props: &Props,
        dt: f32,
    ) {
        let Some(torso) = h.app.torso_slow() else {
            return;
        };
        let Some(a) = self.assembly_index(torso) else {
            return;
        };
        if self.parts[self.assemblies[a].root].id != torso || self.assemblies[a].anchored {
            return;
        }
        let mut current = self.part(torso).map(|p| p.cframe).unwrap_or_default();
        let (wl, wr) = self.calc_desired_walk_velocity(h, dm, props);
        let (fl, fr) = self.floor_point_velocity(h);
        let mut dl = wl + fl;
        let dr = wr + fr;
        let floor = self.floor_part_valid(h);
        if let Some(f) = floor {
            let n = f.normal;
            let dot = n.dot(Vec3::Y);
            if dot < Self::steep_slope(props) {
                let down = -(Vec3::Y - n * dot);
                if down.length() > 1e-5 {
                    let down = down.normalize();
                    let onto = dl - n * dl.dot(n);
                    dl = onto + down * props.walk_speed - down * onto.dot(down);
                }
            }
            let desired_alt = f.location.y + self.character_hip_height(h, props);
            let err = desired_alt - current.translation.y;
            if err.abs() > 0.0 {
                dl.y += err / dt;
            }
        }
        let cur_v = self.point_velocity(torso, current.translation).0;
        let xz = Vec3::new(dl.x - cur_v.x, 0.0, dl.z - cur_v.z) * RUNNING_KMOVE_P_PGS;
        let max_h = if floor.is_some() {
            MAX_LINEAR_GROUND_MOVE_FORCE
        } else {
            MAX_LINEAR_MOVE_FORCE
        };
        if let Some(f) = floor {
            let kfric = self.floor_friction(f);
            let d = Vec3::new(dl.x - cur_v.x, 0.0, dl.z - cur_v.z) * kfric;
            dl.x = cur_v.x + d.x;
            dl.z = cur_v.z + d.z;
        }
        if xz.length_squared() > max_h * max_h {
            let d = xz.normalize_or_zero() * max_h * dt;
            if (d.x >= 0.0 && d.x <= dl.x - cur_v.x) || (d.x < 0.0 && d.x >= dl.x - cur_v.x) {
                dl.x = cur_v.x + d.x;
            }
            if (d.z >= 0.0 && d.z <= dl.z - cur_v.z) || (d.z < 0.0 && d.z >= dl.z - cur_v.z) {
                dl.z = cur_v.z + d.z;
            }
        }
        current.translation += dl * dt;
        if dr.y != 0.0 {
            current.rotation = Mat3::from_rotation_y(dr.y * dt) * current.rotation;
        }
        let asm = &mut self.assemblies[a];
        let offset = asm.com_local;
        asm.orientation = current.rotation;
        asm.position = current * offset;
        asm.linear = dl;
        asm.angular = dr;
        asm.moved = true;
        self.sync_part_frames(a);
        if dl.length() > 2.0
            && let Some(f) = floor
        {
            self.wake_part(f.part);
        }
        if let Some(f) = floor
            && let Some(fa) = self.assembly_index(f.part)
            && self.assemblies[fa].is_moving()
        {
            let mass = self.assemblies[a].mass.min(self.assemblies[fa].mass);
            let impulse = -mass * crate::world::GRAVITY * dt * 0.5;
            let fasm = &mut self.assemblies[fa];
            if fasm.mass > 0.0 {
                fasm.linear += Vec3::new(0.0, impulse / fasm.mass, 0.0);
            }
        }
    }

    fn floor_friction(&self, f: Floor) -> f32 {
        match self.mode {
            crate::world::PhysicalMode::Legacy => {
                self.part(f.part).map_or(0.3, |p| p.legacy_friction)
            }
            crate::world::PhysicalMode::New => self.part(f.part).map_or(0.3, |p| p.friction),
        }
    }

    pub fn humanoid_forces(&mut self) {
        let ids: Vec<InstanceId> = {
            let mut v: Vec<InstanceId> = self.humanoids.keys().copied().collect();
            v.sort();
            v
        };
        for id in ids {
            let Some(mut h) = self.humanoids.remove(&id) else {
                continue;
            };
            if h.entered {
                self.compute_humanoid_force(&mut h);
            }
            self.humanoids.insert(id, h);
        }
    }

    fn compute_humanoid_force(&mut self, h: &mut HumanoidSim) {
        let Some(torso) = h.app.torso_slow() else {
            return;
        };
        let Some(a) = self.assembly_index(torso) else {
            return;
        };
        if !self.assemblies[a].is_moving() || self.assemblies[a].kinematic {
            return;
        }
        let root_cf = self.assemblies[a].root_cframe();
        h.last_torque = root_cf.vector_to_object_space(self.assemblies[a].torque);
        h.last_force = self.assemblies[a].force;
        let (impact, impact_hit) = if h.state.is_no_physics() {
            (0.0, false)
        } else {
            (
                std::mem::take(&mut h.impact_peak),
                std::mem::take(&mut h.impact_hit),
            )
        };
        match h.state {
            HState::Running | HState::RunningSlave => {
                if h.touched_hard {
                    return;
                }
                let crit = RAGDOLL_CRITERIA * 6000.0;
                let tripped = if self.spring_kernel_place {
                    (h.last_force.length_squared() > crit * crit
                        || h.last_torque.length_squared() > crit * crit
                        || impact > crit)
                        && impact_hit
                } else {
                    (h.last_force.length_squared() > crit * crit
                        || h.last_torque.length_squared() > crit * crit)
                        && self.hit_by_high_impact(h)
                };
                if tripped {
                    h.touched_hard = true;
                    return;
                }
                self.balance(h, a);
                self.running_force(h, a);
            }
            HState::Landed | HState::Climbing => {
                self.balance(h, a);
                self.running_force(h, a);
            }
            HState::GettingUp | HState::Flying => self.balance(h, a),
            HState::Freefall => {
                self.balance(h, a);
                self.freefall_force(h, a);
            }
            HState::Jumping => {
                self.balance(h, a);
                self.jumping_force(h, a);
            }
            HState::Swimming => self.swimming_force(h, a),
            _ => {}
        }
    }

    fn impact_velocity(&self, id: InstanceId, at: Vec3) -> Vec3 {
        match self.assembly_index(id) {
            Some(a) if self.spring_kernel_place && !self.assemblies[a].anchored => {
                let asm = &self.assemblies[a];
                asm.pre_solve_linear + asm.pre_solve_angular.cross(at - asm.position)
            }
            _ => self.point_velocity(id, at).0,
        }
    }

    fn hit_by_high_impact(&self, h: &HumanoidSim) -> bool {
        let Some(torso) = h.app.torso_slow() else {
            return false;
        };
        for c in self.contacts.values() {
            if !c.overlapping || (c.a != torso && c.b != torso) {
                continue;
            }
            let Some(pa) = self.part(c.a) else {
                continue;
            };
            let pose_a = pa.cframe * pa.shape.local;
            let other = if c.a == torso { c.b } else { c.a };
            for m in &c.manifolds {
                let n = pose_a.rotation * crate::math::from_parry_vec(m.local_n1);
                if n.length_squared() < 0.8 {
                    continue;
                }
                let n = n.normalize();
                for point in &m.points {
                    let at = pose_a * crate::math::from_parry_vec(point.local_p1);
                    let va = self.impact_velocity(c.a, at);
                    let vb = self.impact_velocity(c.b, at);
                    let approach = n.dot(va - vb);
                    if approach <= RAGDOLL_CRITERIA {
                        continue;
                    }
                    let human = self.impact_velocity(torso, at);
                    let object = self.impact_velocity(other, at);
                    if n.dot(human).abs() >= n.dot(object).abs() {
                        continue;
                    }
                    return true;
                }
            }
        }
        false
    }

    fn balance(&mut self, h: &mut HumanoidSim, a: usize) {
        if h.balance_tick > 0 {
            h.balance_tick -= 1;
            self.assemblies[a].torque += h.last_balance_torque;
            return;
        }
        let Some(torso_cf) = self.part_cf(h.app.torso_slow()) else {
            return;
        };
        let asm = &self.assemblies[a];
        let y_axis = torso_cf.rotation.y_axis;
        let tilt_world = Vec3::Y.cross(y_axis);
        let ang_vel = asm.angular;
        let ext = asm.torque;
        let root = asm.root_cframe();
        let tilt_root = root.vector_to_object_space(tilt_world);
        let ang_root = root.vector_to_object_space(ang_vel);
        let ext_root = root.vector_to_object_space(ext);
        let control = -(asm.inertia_local * tilt_root) * h.balance_p;
        let mut torque_root = ext_root + control;
        let ib = asm.ibody();
        for i in [0usize, 2] {
            let max = MAX_TORQUE_COMPONENT * ib[i];
            if (control[i] - ext_root[i]).abs() < max {
                torque_root[i] = control[i];
            }
        }
        torque_root -= ib * ang_root * h.balance_d;
        let torque_world = root.vector_to_world_space(torque_root);
        let added = torque_world - ext;
        self.assemblies[a].torque += added;
        h.last_balance_torque = added;
        h.balance_tick = 1;
    }

    fn apply_floor_force(&mut self, floor: Option<Floor>, force: Vec3) {
        let Some(f) = floor else {
            return;
        };
        let Some(fa) = self.assembly_index(f.part) else {
            return;
        };
        let asm = &mut self.assemblies[fa];
        if asm.is_moving() && !asm.kinematic {
            asm.force += force;
            asm.torque += (f.touch - asm.position).cross(force);
        }
    }

    fn running_force(&mut self, h: &mut HumanoidSim, a: usize) {
        let floor = self.floor_part_valid(h);
        {
            let asm = &mut self.assemblies[a];
            let ib = asm.ibody();
            let mut ty =
                K_TURN_P_ROTATE_PGS * ib.y * (h.floor_rot.y + h.desired_rot.y - asm.angular.y);
            ty = ty.clamp(-1e5, 1e5);
            asm.torque += Vec3::new(0.0, ty, 0.0);
        }
        let mass = self.assemblies[a].mass;
        if floor.is_some() && h.desired_altitude < f32::INFINITY {
            let torso_y = self
                .part_cf(h.app.torso_slow())
                .map_or(0.0, |c| c.translation.y);
            let asm = &self.assemblies[a];
            let y_accel = K_ALTITUDE_P * (h.desired_altitude - torso_y)
                - K_ALTITUDE_D * (asm.linear.y - h.floor_lin.y);
            if y_accel > 0.0 {
                let current = asm.force.y / asm.mass.max(1e-6);
                if y_accel > current {
                    let delta = (mass * (y_accel - current)).clamp(-1e7, 1e7);
                    self.assemblies[a].force += Vec3::new(0.0, delta * 0.1, 0.0);
                    self.apply_floor_force(floor, Vec3::new(0.0, -delta * 0.1, 0.0));
                }
            }
        }
        let asm = &self.assemblies[a];
        let desired_v = h.floor_lin + h.desired_lin;
        let current_v = asm.linear;
        let mut desired_accel = (desired_v - current_v) * RUNNING_KMOVE_P_PGS;
        let current_accel = asm.force / asm.mass.max(1e-6);
        if h.desired_lin.y <= 0.0 && !h.facing_ladder {
            desired_accel.y = current_accel.y;
        }
        let mut delta = desired_accel - current_accel;
        if !h.facing_ladder {
            delta.y = delta.y.clamp(MIN_MOVE_FORCE.y, MAX_MOVE_FORCE.y);
        }
        let mut horiz = Vec3::new(delta.x, 0.0, delta.z);
        let max_h = if floor.is_some() {
            MAX_LINEAR_GROUND_MOVE_FORCE
        } else {
            MAX_LINEAR_MOVE_FORCE
        };
        if horiz.length_squared() > max_h * max_h {
            horiz = horiz.normalize_or_zero() * max_h;
            delta.x = horiz.x;
            delta.z = horiz.z;
        }
        let mut force = delta * mass;
        if let Some(f) = floor {
            let k = self.floor_friction(f);
            force.x *= k;
            force.z *= k;
        }
        self.assemblies[a].force += force;
        if force.y > 0.0 {
            self.apply_floor_force(floor, -force * 0.1);
        }
    }

    fn freefall_force(&mut self, h: &mut HumanoidSim, a: usize) {
        let asm = &mut self.assemblies[a];
        let ib = asm.ibody();
        let mut ty = K_TURN_P_FREEFALL_PGS * ib.y * (h.desired_rot.y - asm.angular.y);
        let tmax = ib.y * K_TURN_ACCEL_MAX;
        ty -= asm.torque.y;
        ty = ty.clamp(-tmax, tmax);
        asm.torque += Vec3::new(0.0, ty, 0.0);
        let mut accel = (h.desired_lin - asm.linear) * RUNNING_KMOVE_P_PGS;
        accel.y = accel.y.clamp(MIN_MOVE_FORCE.y, MAX_MOVE_FORCE.y);
        let mut horiz = Vec3::new(accel.x, 0.0, accel.z);
        if horiz.length_squared() > MAX_LINEAR_MOVE_FORCE * MAX_LINEAR_MOVE_FORCE {
            horiz = horiz.normalize_or_zero() * MAX_LINEAR_MOVE_FORCE;
            accel.x = horiz.x;
            accel.z = horiz.z;
        }
        let mut f = accel * asm.mass;
        f.y = 0.0;
        asm.force += f;
    }

    fn jumping_force(&mut self, h: &mut HumanoidSim, a: usize) {
        if h.finished {
            return;
        }
        let jump_power = h.jump_power_cache;
        let asm = &self.assemblies[a];
        let jump_v = asm.linear.dot(h.jump_dir);
        let y_accel = K_JUMP_P * (jump_power - jump_v);
        if y_accel <= 0.0 || self.find_ceiling(h) {
            h.finished = true;
            return;
        }
        let floor = self.floor_part_valid(h);
        let mass = asm.mass;
        let new_y = mass * y_accel;
        if floor.is_some() {
            let current_y = asm.force.y;
            if new_y > current_y {
                let diff = new_y - current_y;
                self.assemblies[a].force += h.jump_dir * new_y * PGS_JUMP_FORCE_ADJUSTMENT;
                self.apply_floor_force(
                    floor,
                    Vec3::new(0.0, -diff * PGS_JUMP_FORCE_ADJUSTMENT, 0.0),
                );
            }
        } else {
            self.assemblies[a].force += h.jump_dir * new_y * 0.1;
        }
    }

    fn find_ceiling(&self, h: &HumanoidSim) -> bool {
        let Some(torso) = h.app.torso_slow() else {
            return false;
        };
        let Some(tp) = self.part(torso) else {
            return false;
        };
        let cf = tp.cframe;
        let half = tp.shape.size * 0.4;
        let max = match self.part_size(h.app.head) {
            Some(hs) => hs.y + tp.shape.size.y * 1.5,
            None => tp.shape.size.y * 1.5,
        };
        let own = tp.assembly;
        let filter = |p: InstanceId| {
            self.part(p).is_some_and(|pp| {
                pp.can_collide && pp.assembly != own && !self.accoutrement_parts.contains(&p)
            })
        };
        let try_ray = |offset: Vec3| {
            self.raycast(cf * offset, Vec3::new(0.0, max, 0.0), filter)
                .is_some()
        };
        if try_ray(Vec3::new(0.0, -half.y, 0.0)) {
            return true;
        }
        for x in [-1.0f32, 1.0] {
            for z in [-1.0f32, 1.0] {
                if try_ray(Vec3::new(x * half.x, -half.y, z * half.z)) {
                    return true;
                }
            }
        }
        false
    }

    fn swimming_force(&mut self, h: &mut HumanoidSim, a: usize) {
        let Some(torso_cf) = self.part_cf(h.app.torso_slow()) else {
            return;
        };
        let mut desired = h.desired_lin;
        let mag = desired.length();
        let horiz = Vec3::new(desired.x, 0.0, desired.z);
        if desired.y * 2.0 < horiz.length() && desired.y * 2.0 > -horiz.length() {
            desired = horiz.normalize_or_zero() * mag;
        }
        let asm = &mut self.assemblies[a];
        if mag > 10.0 {
            let accel = (desired - asm.linear) * (35.0 * asm.mass);
            asm.force += accel.clamp(MIN_SWIM_FORCE, MAX_SWIM_FORCE);
        }
        let pitch_axis = torso_cf.rotation.y_axis;
        let ext = asm.torque;
        let (p_pitch, tilt) = if desired.length_squared() < 5.0 {
            (500.0, Vec3::Y.cross(pitch_axis))
        } else {
            (2000.0, desired.normalize_or_zero().cross(pitch_axis))
        };
        let root = asm.root_cframe();
        let ang_root = root.vector_to_object_space(asm.angular);
        let mut control = -(asm.inertia_local * root.vector_to_object_space(tilt)) * p_pitch;
        control -= asm.ibody() * ang_root * 50.0;
        control += root.vector_to_object_space(ext);
        let world = root.vector_to_world_space(control);
        asm.torque += world - ext;
    }

    pub fn humanoid_debug(&self, id: InstanceId) -> String {
        match self.humanoids.get(&id) {
            Some(h) => format!(
                "state={:?} floor={:?} alt={} app={:?} kin={:?}",
                h.state,
                h.floor.map(|f| (f.part, f.location)),
                h.desired_altitude,
                h.app,
                h.app
                    .torso_slow()
                    .and_then(|t| self.assembly_index(t))
                    .map(|a| (
                        self.assemblies[a].kinematic,
                        self.parts[self.assemblies[a].root].id
                    ))
            ),
            None => "none".into(),
        }
    }

    pub fn humanoid(&self, id: InstanceId) -> Option<&HumanoidSim> {
        self.humanoids.get(&id)
    }

    pub fn humanoid_mut(&mut self, id: InstanceId) -> Option<&mut HumanoidSim> {
        self.humanoids.get_mut(&id)
    }

    pub fn refresh_humanoid_cache(&mut self, dm: &DataModel) {
        self.accoutrement_parts.clear();
        let players: Vec<InstanceId> = dm
            .find_service("Players")
            .map(|p| {
                dm.children_of(p)
                    .iter()
                    .copied()
                    .filter(|c| dm.is_a(*c, "Player"))
                    .collect()
            })
            .unwrap_or_default();
        let characters: HashSet<InstanceId> = players
            .iter()
            .filter_map(|p| dm.get_ref(*p, "Character"))
            .collect();
        for h in self.humanoids.values_mut() {
            h.jump_power_cache = dm
                .get(h.id, "JumpPower")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(50.0);
            h.player_character = dm.parent_of(h.id).is_some_and(|c| characters.contains(&c));
            h.owned_locally =
                self.local_character.is_some() && dm.parent_of(h.id) == self.local_character;
        }
        let ids: Vec<InstanceId> = self.parts.iter().map(|p| p.id).collect();
        for id in ids {
            if dm.parent_of(id).is_some_and(|p| dm.is_a(p, "Accoutrement")) {
                self.accoutrement_parts.insert(id);
            }
        }
    }

    pub fn apply_dm_writes(&mut self, dm: &mut DataModel) {
        let writes = std::mem::take(&mut self.dm_writes);
        for w in writes {
            match w {
                DmWrite::Set(id, name, v) => {
                    if dm.contains(id) && dm.get(id, name).ok().as_ref() != Some(&v) {
                        if name == "SeatPart" {
                            let _ = dm.set(id, name, v);
                        } else {
                            dm.set_unchecked(id, name, v);
                        }
                    }
                }
                DmWrite::Silent(id, name, v) => {
                    if dm.contains(id) {
                        dm.store_silent(id, name, v);
                    }
                }
                DmWrite::BreakJoints(model) => {
                    if !dm.contains(model) {
                        continue;
                    }
                    let parts: Vec<InstanceId> = dm
                        .descendants(model)
                        .into_iter()
                        .filter(|d| dm.is_a(*d, "BasePart"))
                        .collect();
                    for p in parts {
                        for j in self.joints_of(p) {
                            if dm.contains(j) {
                                let _ = dm.destroy(j);
                            }
                            self.remove_joint_record(j);
                            self.topology_dirty = true;
                        }
                    }
                }
            }
        }
    }
}
