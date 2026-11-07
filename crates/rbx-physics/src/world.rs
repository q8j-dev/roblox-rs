use crate::broadphase::SpatialIndex;
use crate::math::{rotate_about_z, safe_inverse, to_pose};
use crate::shape::{PartShape, ShapeKind, rotate_inertia, shift_inertia};
use crate::solver::{Constraint, ConstraintKind, Solver};
use parry3d::bounding_volume::Aabb;
use parry3d::query::{ContactManifold, ContactManifoldsWorkspace};
use rbx_dom::{DataModel, DomEvent, EventCursor, InstanceId, Variant};
use rbx_math::materials::material_physics;
use rbx_math::{CFrame, Mat3, NormalId, Vec3, Vec4, normal_id_to_vector, vector_to_normal_id};
use std::collections::{HashMap, HashSet, VecDeque};

pub const GRAVITY: f32 = 196.2;
pub const WORLD_STEPS_PER_SEC: u32 = 240;
pub const WORLD_STEPS_PER_UI_STEP: u32 = 4;
pub const WORLD_STEPS_PER_LONG_STEP: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalMode {
    Legacy,
    New,
}

#[derive(Clone)]
pub struct Part {
    pub id: InstanceId,
    pub shape: PartShape,
    pub cframe: CFrame,
    pub anchored: bool,
    pub can_collide: bool,
    pub mass: f32,
    pub friction: f32,
    pub elasticity: f32,
    pub friction_weight: f32,
    pub elasticity_weight: f32,
    pub legacy_friction: f32,
    pub legacy_elasticity: f32,
    pub material: u32,
    pub surfaces: [u32; 6],
    pub surface_inputs: [(u32, f32, f32); 6],
    pub velocity: Vec3,
    pub rot_velocity: Vec3,
    pub assembly: usize,
    pub offset: CFrame,
    pub aabb: Aabb,
    pub sort_size: u64,
    pub moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    Weld,
    Snap,
    ManualWeld,
    Glue,
    ManualGlue,
    Motor,
    Motor6D,
    VelocityMotor,
    Rotate,
    RotateP,
    RotateV,
}

impl JointKind {
    pub fn from_class(class: &str) -> Option<JointKind> {
        Some(match class {
            "Weld" => JointKind::Weld,
            "Snap" => JointKind::Snap,
            "ManualWeld" => JointKind::ManualWeld,
            "Glue" => JointKind::Glue,
            "ManualGlue" => JointKind::ManualGlue,
            "Motor" => JointKind::Motor,
            "Motor6D" => JointKind::Motor6D,
            "VelocityMotor" => JointKind::VelocityMotor,
            "Rotate" => JointKind::Rotate,
            "RotateP" => JointKind::RotateP,
            "RotateV" => JointKind::RotateV,
            _ => return None,
        })
    }

    pub fn is_rigid(self) -> bool {
        matches!(
            self,
            JointKind::Weld
                | JointKind::Snap
                | JointKind::ManualWeld
                | JointKind::Motor
                | JointKind::Motor6D
                | JointKind::VelocityMotor
        )
    }

    pub fn is_motor(self) -> bool {
        matches!(
            self,
            JointKind::Motor | JointKind::Motor6D | JointKind::VelocityMotor
        )
    }
}

pub struct Joint {
    pub id: InstanceId,
    pub kind: JointKind,
    pub part0: Option<InstanceId>,
    pub part1: Option<InstanceId>,
    pub c0: CFrame,
    pub c1: CFrame,
    pub in_world: bool,
    pub max_velocity: f32,
    pub desired_angle: f32,
    pub current_angle: f32,
    pub base_angle: f32,
    pub ui_value: f32,
    pub stepping_angle: f32,
    pub glue_faces: [Vec3; 4],
    pub constraints: Vec<Constraint>,
    pub angle_dirty: bool,
    pub pose_offset: Vec3,
    pub pose_axis_angle: Vec3,
    pub pose_angle: f32,
    pub pose_mask_weight: f32,
    pub pose_freshness: u32,
    pub current_offset: Vec3,
    pub current_axis_angle: Vec3,
}

impl Joint {
    pub fn child_in_parent(&self, parent_is_part0: bool) -> CFrame {
        let (pc, cc) = if parent_is_part0 {
            (self.c0, self.c1)
        } else {
            (self.c1, self.c0)
        };
        match self.kind {
            JointKind::Motor6D => {
                let aa = self.current_axis_angle;
                let angle = aa.length();
                let rot = if angle > 0.0 {
                    Mat3::from_axis_angle(aa / angle, angle)
                } else {
                    Mat3::IDENTITY
                };
                pc * CFrame::new(rot, self.current_offset) * cc.inverse()
            }
            JointKind::Motor | JointKind::VelocityMotor => {
                CFrame::new(
                    rotate_about_z(pc.rotation, self.current_angle),
                    pc.translation,
                ) * cc.inverse()
            }
            _ => pc * cc.inverse(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MoverKind {
    Position {
        p: f32,
        d: f32,
        max_force: Vec3,
        position: Vec3,
    },
    Velocity {
        p: f32,
        max_force: Vec3,
        velocity: Vec3,
    },
    Gyro {
        p: f32,
        d: f32,
        max_torque: Vec3,
        cframe: CFrame,
    },
    AngularVelocity {
        p: f32,
        max_torque: Vec3,
        angular_velocity: Vec3,
    },
    Force {
        force: Vec3,
    },
    Thrust {
        force: Vec3,
        location: Vec3,
    },
    Rocket(RocketParams),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RocketParams {
    pub active: bool,
    pub cartoon_factor: f32,
    pub max_speed: f32,
    pub max_thrust: f32,
    pub max_torque: Vec3,
    pub target: Option<InstanceId>,
    pub target_offset: Vec3,
    pub target_radius: f32,
    pub thrust_p: f32,
    pub thrust_d: f32,
    pub turn_p: f32,
    pub turn_d: f32,
}

pub struct Mover {
    pub id: InstanceId,
    pub part: Option<InstanceId>,
    pub kind: MoverKind,
    pub reached: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SleepHistory {
    samples: VecDeque<(Vec3, Vec4)>,
    steps_since_sample: u32,
    awake_steps: u32,
    max_deviation_sq: f32,
}

const HISTORY_SIZE: usize = 15;
const SAMPLE_SKIP: u32 = 8;
const SLEEP_TOLERANCE: f32 = 0.1;

impl SleepHistory {
    fn new() -> SleepHistory {
        SleepHistory {
            awake_steps: HISTORY_SIZE as u32,
            ..SleepHistory::default()
        }
    }

    pub(crate) fn wake(&mut self) {
        self.awake_steps = HISTORY_SIZE as u32;
    }

    pub(crate) fn sample(&mut self, coord: (Vec3, Vec4)) -> bool {
        self.steps_since_sample += 1;
        if self.steps_since_sample >= SAMPLE_SKIP {
            self.steps_since_sample = 0;
            self.awake_steps = self.awake_steps.saturating_sub(1);
            if self.samples.len() == HISTORY_SIZE {
                self.samples.pop_front();
            }
            self.samples.push_back(coord);
            let n = self.samples.len() as f32;
            let (sp, sq) = self
                .samples
                .iter()
                .fold((Vec3::ZERO, Vec4::ZERO), |(a, b), (p, q)| (a + *p, b + *q));
            let avg = (sp / n, sq / n);
            self.max_deviation_sq = self
                .samples
                .iter()
                .map(|(p, q)| (*p - avg.0).length_squared() + (*q - avg.1).length_squared())
                .fold(0.0, f32::max);
        }
        self.not_moving()
    }

    fn not_moving(&self) -> bool {
        self.awake_steps == 0 && self.max_deviation_sq < SLEEP_TOLERANCE * SLEEP_TOLERANCE
    }

    fn prevents_neighbor_sleep(&self) -> bool {
        self.max_deviation_sq > 2.0 * SLEEP_TOLERANCE * SLEEP_TOLERANCE
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TreeEdge {
    pub joint: InstanceId,
    pub parent: usize,
    pub child: usize,
    pub parent_is_part0: bool,
}

pub struct Assembly {
    pub parts: Vec<usize>,
    pub root: usize,
    pub anchored: bool,
    pub mass: f32,
    pub com_local: Vec3,
    pub inertia_local: Mat3,
    pub inv_inertia_local: Mat3,
    pub position: Vec3,
    pub orientation: Mat3,
    pub linear: Vec3,
    pub angular: Vec3,
    pub uid: u64,
    pub sleeping: bool,
    pub sleep_checking: bool,
    pub woke: bool,
    pub history: SleepHistory,
    pub tree: Vec<TreeEdge>,
    pub force: Vec3,
    pub torque: Vec3,
    pub radius: f32,
    pub moved: bool,
    pub kinematic: bool,
}

impl Assembly {
    pub(crate) fn wake_from_sleep(&mut self) {
        if self.sleeping {
            self.sleeping = false;
            self.sleep_checking = false;
            self.woke = true;
            self.linear = Vec3::ZERO;
            self.angular = Vec3::ZERO;
        }
        self.history.wake();
    }

    pub(crate) fn fall_asleep(&mut self, checking: bool) {
        self.sleeping = true;
        self.sleep_checking = checking;
        self.linear = Vec3::ZERO;
        self.angular = Vec3::ZERO;
    }

    pub(crate) fn high_velocity(&self) -> bool {
        let over = |v: Vec3| v.abs().max_element() > 1.0;
        over(self.linear) || over(self.angular * self.radius)
    }

    pub fn root_cframe(&self) -> CFrame {
        CFrame::new(
            self.orientation,
            self.position - self.orientation * self.com_local,
        )
    }

    pub fn is_moving(&self) -> bool {
        !self.anchored && !self.sleeping
    }

    pub fn velocity_at(&self, p: Vec3) -> Vec3 {
        self.linear + self.angular.cross(p - self.position)
    }

    pub fn inv_inertia_world(&self) -> Mat3 {
        rotate_inertia(self.inv_inertia_local, self.orientation)
    }

    pub fn history_prevents_sleep(&self) -> bool {
        self.history.prevents_neighbor_sleep()
    }

    pub fn ibody(&self) -> Vec3 {
        Vec3::new(
            self.inertia_local.x_axis.x,
            self.inertia_local.y_axis.y,
            self.inertia_local.z_axis.z,
        )
    }
}

pub struct ContactPair {
    pub a: InstanceId,
    pub b: InstanceId,
    pub manifolds: Vec<ContactManifold<(), ()>>,
    pub workspace: Option<ContactManifoldsWorkspace>,
    pub collisions: Vec<Constraint>,
    pub last_long_step: i64,
    pub overlapping: bool,
    pub seen: u64,
}

#[derive(Default, Debug, Clone)]
pub struct StepEvents {
    pub touched: Vec<(InstanceId, InstanceId)>,
    pub touch_ended: Vec<(InstanceId, InstanceId)>,
    pub fallen: Vec<InstanceId>,
    pub reached_target: Vec<InstanceId>,
    pub broken_joints: Vec<InstanceId>,
    pub humanoid: Vec<(InstanceId, crate::humanoid::HumanoidEvent)>,
}

#[derive(Clone, Debug)]
pub struct NetworkRoot {
    pub id: InstanceId,
    pub sleeping: bool,
    pub cframe: CFrame,
    pub velocity: Vec3,
    pub rot_velocity: Vec3,
    pub parts: Vec<InstanceId>,
}

pub struct PhysicsWorld {
    pub parts: Vec<Part>,
    pub index: HashMap<InstanceId, usize>,
    pub joints: HashMap<InstanceId, Joint>,
    pub movers: HashMap<InstanceId, Mover>,
    pub assemblies: Vec<Assembly>,
    pub contacts: HashMap<(InstanceId, InstanceId), ContactPair>,
    pub terrain: crate::terrain::TerrainCollider,
    pub water_viscosity: f32,
    pub solver: Solver,
    pub mode: PhysicalMode,
    pub fallen_parts_destroy_height: f32,
    pub workspace: Option<InstanceId>,
    pub joints_service: Option<InstanceId>,
    pub step_count: u64,
    pub accumulator: f64,
    pub time: f64,
    pub topology_dirty: bool,
    pub spatial: SpatialIndex,
    pub spatial_dirty: Vec<InstanceId>,
    pub mover_wake: HashMap<InstanceId, (Vec3, Vec3)>,
    pub humanoids: HashMap<InstanceId, crate::humanoid::HumanoidSim>,
    pub humanoid_events: Vec<(InstanceId, crate::humanoid::HumanoidEvent)>,
    pub dm_writes: Vec<crate::humanoid::DmWrite>,
    pub accoutrement_parts: HashSet<InstanceId>,
    pub recursive_wake: Vec<usize>,
    pub external_wake: Vec<usize>,
    pub vehicle_seats: std::collections::BTreeSet<InstanceId>,
    pub part_joints: HashMap<InstanceId, std::collections::BTreeSet<InstanceId>>,
    pub local_character: Option<InstanceId>,
    cursor: Option<EventCursor>,
    pending_events: StepEvents,
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        PhysicsWorld::new()
    }
}

pub fn physical_mode(value: u32) -> PhysicalMode {
    use rbx_base::flags::flag_bool;
    if flag_bool("NewPhysicalPropertiesForcedOnAll", false) {
        return PhysicalMode::New;
    }
    if !flag_bool("MaterialPropertiesEnabled", false) {
        return PhysicalMode::Legacy;
    }
    let threshold = if flag_bool("MaterialPropertiesNewIsDefault", false) {
        1
    } else {
        2
    };
    if value >= threshold {
        PhysicalMode::New
    } else {
        PhysicalMode::Legacy
    }
}

fn get_vec3(dm: &DataModel, id: InstanceId, name: &str) -> Vec3 {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_vec3())
        .unwrap_or(Vec3::ZERO)
}

fn get_cframe(dm: &DataModel, id: InstanceId, name: &str) -> CFrame {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_cframe())
        .unwrap_or_default()
}

fn get_enum(dm: &DataModel, id: InstanceId, name: &str) -> u32 {
    dm.get(id, name).ok().and_then(|v| v.as_enum()).unwrap_or(0)
}

const SURFACE_PROPS: [(&str, &str, &str, &str); 6] = [
    (
        "RightSurface",
        "RightSurfaceInput",
        "RightParamA",
        "RightParamB",
    ),
    ("TopSurface", "TopSurfaceInput", "TopParamA", "TopParamB"),
    (
        "BackSurface",
        "BackSurfaceInput",
        "BackParamA",
        "BackParamB",
    ),
    (
        "LeftSurface",
        "LeftSurfaceInput",
        "LeftParamA",
        "LeftParamB",
    ),
    (
        "BottomSurface",
        "BottomSurfaceInput",
        "BottomParamA",
        "BottomParamB",
    ),
    (
        "FrontSurface",
        "FrontSurfaceInput",
        "FrontParamA",
        "FrontParamB",
    ),
];

pub fn shape_kind_of(dm: &DataModel, id: InstanceId) -> ShapeKind {
    match dm.class_of(id).unwrap_or("") {
        "WedgePart" => ShapeKind::Wedge,
        "CornerWedgePart" => ShapeKind::CornerWedge,
        _ if dm.is_a(id, "PartOperation") => ShapeKind::Compound,
        _ => {
            if dm.is_a(id, "Part") {
                match get_enum(dm, id, "shape") {
                    0 => ShapeKind::Ball,
                    2 => ShapeKind::Cylinder,
                    _ => ShapeKind::Block,
                }
            } else {
                ShapeKind::Block
            }
        }
    }
}

pub fn part_size(dm: &DataModel, id: InstanceId) -> Vec3 {
    dm.get_raw(id, "size")
        .and_then(|v| v.as_vec3())
        .unwrap_or_else(|| get_vec3(dm, id, "Size"))
}

fn sort_size(size: Vec3, multiplier: u64) -> u64 {
    let mut s = [size.x, size.y, size.z];
    s.sort_by(|a, b| b.total_cmp(a));
    let planar = s[0] * s[1];
    (planar * 50.0).floor().max(0.0) as u64 * multiplier + 1
}

fn size_multiplier(dm: &DataModel, id: InstanceId) -> u64 {
    if matches!(
        dm.class_of(id),
        Some("Seat" | "VehicleSeat" | "SkateboardPlatform")
    ) {
        return 20;
    }
    match dm.name_of(id) {
        "HumanoidRootPart" => 10,
        "Torso" => 5,
        _ => 1,
    }
}

impl PhysicsWorld {
    pub fn new() -> PhysicsWorld {
        PhysicsWorld {
            parts: Vec::new(),
            index: HashMap::new(),
            joints: HashMap::new(),
            movers: HashMap::new(),
            assemblies: Vec::new(),
            contacts: HashMap::new(),
            terrain: Default::default(),
            water_viscosity: 0.02,
            solver: Solver::default(),
            mode: PhysicalMode::New,
            fallen_parts_destroy_height: -500.0,
            workspace: None,
            joints_service: None,
            step_count: 0,
            accumulator: 0.0,
            time: 0.0,
            topology_dirty: true,
            spatial: SpatialIndex::new(16.0),
            spatial_dirty: Vec::new(),
            mover_wake: HashMap::new(),
            humanoids: HashMap::new(),
            humanoid_events: Vec::new(),
            dm_writes: Vec::new(),
            accoutrement_parts: HashSet::new(),
            recursive_wake: Vec::new(),
            external_wake: Vec::new(),
            vehicle_seats: Default::default(),
            part_joints: HashMap::new(),
            local_character: None,
            cursor: None,
            pending_events: StepEvents::default(),
        }
    }

    pub fn attach(&mut self, dm: &mut DataModel) {
        if self.cursor.is_none() {
            self.cursor = Some(dm.subscribe());
        }
        self.workspace = dm.find_service("Workspace");
        self.joints_service = dm.find_service("JointsService");
        self.read_workspace(dm);
        let mut all = Vec::new();
        if let Some(ws) = self.workspace {
            all.extend(dm.descendants(ws));
        }
        if let Some(js) = self.joints_service {
            all.extend(dm.descendants(js));
        }
        for id in all {
            self.reconsider(dm, id);
        }
        self.topology_dirty = true;
    }

    pub fn detach(&mut self, dm: &mut DataModel) {
        if let Some(c) = self.cursor.take() {
            dm.events.unsubscribe(c);
        }
    }

    fn read_workspace(&mut self, dm: &DataModel) {
        if let Some(ws) = self.workspace {
            self.fallen_parts_destroy_height = dm
                .get(ws, "FallenPartsDestroyHeight")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(-500.0);
            self.mode = physical_mode(get_enum(dm, ws, "PhysicalPropertiesMode"));
        }
    }

    pub fn part(&self, id: InstanceId) -> Option<&Part> {
        self.index.get(&id).map(|&i| &self.parts[i])
    }

    pub fn assembly_of(&self, id: InstanceId) -> Option<&Assembly> {
        let p = self.part(id)?;
        self.assemblies.get(p.assembly)
    }

    fn in_workspace(&self, dm: &DataModel, id: InstanceId) -> bool {
        match self.workspace {
            Some(ws) => dm.is_descendant_of(id, ws),
            None => false,
        }
    }

    pub fn sync(&mut self, dm: &mut DataModel) {
        self.terrain.sync(dm);
        let Some(cursor) = self.cursor else {
            return;
        };
        let events = dm.drain_events(cursor);
        if events.is_empty() {
            return;
        }
        let mut reconsider: Vec<InstanceId> = Vec::new();
        let mut seen: HashSet<InstanceId> = HashSet::new();
        let mut props: Vec<(InstanceId, &'static str)> = Vec::new();
        for e in events {
            match e {
                DomEvent::AncestryChanged { id, .. } => {
                    if seen.insert(id) {
                        reconsider.push(id);
                    }
                }
                DomEvent::Destroyed { id } => {
                    if seen.insert(id) {
                        reconsider.push(id);
                    }
                }
                DomEvent::PropertyChanged { id, property } => props.push((id, property)),
                _ => {}
            }
        }
        if self.workspace.is_none() {
            self.workspace = dm.find_service("Workspace");
        }
        if self.joints_service.is_none() {
            self.joints_service = dm.find_service("JointsService");
        }
        for id in reconsider {
            self.reconsider(dm, id);
        }
        for (id, prop) in props {
            self.property_changed(dm, id, prop);
        }
        self.flush_spatial();
    }

    pub fn reconsider(&mut self, dm: &DataModel, id: InstanceId) {
        if !dm.contains(id) {
            self.remove_part(id);
            if self.remove_joint_record(id) {
                self.topology_dirty = true;
            }
            self.movers.remove(&id);
            self.humanoids.remove(&id);
            return;
        }
        let class = dm.class_of(id).unwrap_or("");
        if dm.is_a(id, "BasePart") && class != "Terrain" {
            if self.in_workspace(dm, id) {
                self.upsert_part(dm, id);
            } else {
                self.remove_part(id);
            }
        } else if let Some(kind) = JointKind::from_class(class) {
            self.upsert_joint(dm, id, kind);
        } else if dm.is_a(id, "BodyMover") {
            self.upsert_mover(dm, id);
        } else if class == "Humanoid" {
            self.upsert_humanoid(dm, id);
        }
    }

    fn joint_in_world(&self, dm: &DataModel, id: InstanceId) -> bool {
        if let Some(p) = dm.parent_of(id)
            && Some(p) == self.joints_service
        {
            return true;
        }
        self.in_workspace(dm, id)
    }

    pub fn upsert_joint(&mut self, dm: &DataModel, id: InstanceId, kind: JointKind) {
        let mut part0 = dm.get_ref(id, "Part0");
        let mut part1 = dm.get_ref(id, "Part1");
        if kind == JointKind::VelocityMotor {
            if part0.is_none() {
                part0 = dm
                    .parent_of(id)
                    .and_then(|f| {
                        if dm.is_a(f, "BasePart") {
                            Some(f)
                        } else {
                            dm.parent_of(f)
                        }
                    })
                    .filter(|p| dm.is_a(*p, "BasePart"));
            }
            if part1.is_none() {
                part1 = dm
                    .get_ref(id, "Hole")
                    .and_then(|h| dm.parent_of(h))
                    .filter(|p| dm.is_a(*p, "BasePart"));
            }
        }
        let c0 = get_cframe(dm, id, "C0");
        let c1 = get_cframe(dm, id, "C1");
        let in_world = self.joint_in_world(dm, id);
        let f = |n: &str| dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(0.0);
        let glue_faces = if matches!(kind, JointKind::Glue) {
            [
                get_vec3(dm, id, "F0"),
                get_vec3(dm, id, "F1"),
                get_vec3(dm, id, "F2"),
                get_vec3(dm, id, "F3"),
            ]
        } else {
            [Vec3::ZERO; 4]
        };
        let entry = self.joints.entry(id).or_insert_with(|| Joint {
            id,
            kind,
            part0: None,
            part1: None,
            c0: CFrame::default(),
            c1: CFrame::default(),
            in_world: false,
            max_velocity: 0.0,
            desired_angle: 0.0,
            current_angle: 0.0,
            base_angle: 0.0,
            ui_value: 0.0,
            stepping_angle: 0.0,
            glue_faces,
            constraints: Vec::new(),
            angle_dirty: false,
            pose_offset: Vec3::ZERO,
            pose_axis_angle: Vec3::ZERO,
            pose_angle: 0.0,
            pose_mask_weight: 1.0,
            pose_freshness: 0,
            current_offset: Vec3::ZERO,
            current_axis_angle: Vec3::ZERO,
        });
        let changed = entry.part0 != part0
            || entry.part1 != part1
            || entry.c0 != c0
            || entry.c1 != c1
            || entry.in_world != in_world
            || entry.kind != kind;
        let old_parts = [entry.part0, entry.part1];
        if changed {
            entry.constraints.clear();
        }
        entry.kind = kind;
        entry.part0 = part0;
        entry.part1 = part1;
        entry.c0 = c0;
        entry.c1 = c1;
        entry.in_world = in_world;
        entry.glue_faces = glue_faces;
        if kind.is_motor() {
            entry.max_velocity = f("MaxVelocity");
            entry.desired_angle = f("DesiredAngle");
            entry.current_angle = f("CurrentAngle");
            if kind == JointKind::Motor6D {
                entry.current_axis_angle = Vec3::new(0.0, 0.0, entry.current_angle);
            }
        }
        if matches!(kind, JointKind::RotateP | JointKind::RotateV) {
            entry.base_angle = f("BaseAngle");
        }
        if changed {
            self.topology_dirty = true;
            let new_parts = [part0, part1];
            for p in old_parts.into_iter().flatten() {
                if !new_parts.contains(&Some(p))
                    && let Some(v) = self.part_joints.get_mut(&p)
                {
                    v.remove(&id);
                }
            }
            let mut added: Option<InstanceId> = None;
            for p in new_parts.into_iter().flatten() {
                if old_parts.contains(&Some(p)) || added == Some(p) {
                    continue;
                }
                self.part_joints.entry(p).or_default().insert(id);
                added = Some(p);
            }
        }
    }

    pub fn remove_joint_record(&mut self, id: InstanceId) -> bool {
        let Some(j) = self.joints.remove(&id) else {
            return false;
        };
        for p in [j.part0, j.part1].into_iter().flatten() {
            if let Some(v) = self.part_joints.get_mut(&p) {
                v.remove(&id);
                if v.is_empty() {
                    self.part_joints.remove(&p);
                }
            }
        }
        self.topology_dirty = true;
        true
    }

    pub fn upsert_mover(&mut self, dm: &DataModel, id: InstanceId) {
        let part = dm.parent_of(id).filter(|p| dm.is_a(*p, "BasePart"));
        let f = |n: &str| dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(0.0);
        let v = |n: &str| get_vec3(dm, id, n);
        let kind = match dm.class_of(id).unwrap_or("") {
            "BodyPosition" => MoverKind::Position {
                p: f("P"),
                d: f("D"),
                max_force: v("MaxForce"),
                position: v("Position"),
            },
            "BodyVelocity" => MoverKind::Velocity {
                p: f("P"),
                max_force: v("MaxForce"),
                velocity: v("Velocity"),
            },
            "BodyGyro" => MoverKind::Gyro {
                p: f("P"),
                d: f("D"),
                max_torque: v("MaxTorque"),
                cframe: get_cframe(dm, id, "CFrame"),
            },
            "BodyAngularVelocity" => MoverKind::AngularVelocity {
                p: f("P"),
                max_torque: v("MaxTorque"),
                angular_velocity: v("AngularVelocity"),
            },
            "BodyForce" => MoverKind::Force { force: v("Force") },
            "BodyThrust" => MoverKind::Thrust {
                force: v("Force"),
                location: v("Location"),
            },
            "RocketPropulsion" => MoverKind::Rocket(RocketParams {
                active: dm.get_bool(id, "Active"),
                cartoon_factor: f("CartoonFactor"),
                max_speed: f("MaxSpeed"),
                max_thrust: f("MaxThrust"),
                max_torque: v("MaxTorque"),
                target: dm.get_ref(id, "Target"),
                target_offset: v("TargetOffset"),
                target_radius: f("TargetRadius"),
                thrust_p: f("ThrustP"),
                thrust_d: f("ThrustD"),
                turn_p: f("TurnP"),
                turn_d: f("TurnD"),
            }),
            _ => return,
        };
        let entry = self.movers.entry(id).or_insert(Mover {
            id,
            part,
            kind,
            reached: false,
        });
        if entry.kind != kind {
            entry.reached = false;
        }
        entry.part = part;
        entry.kind = kind;
        if let Some(p) = part {
            self.wake_part(p);
        }
    }

    fn read_part(&self, dm: &DataModel, id: InstanceId) -> Part {
        let kind = shape_kind_of(dm, id);
        let size = part_size(dm, id);
        let shape = if kind == ShapeKind::Compound {
            crate::csg::union_shape(dm, id, size)
        } else {
            PartShape::new(kind, size)
        };
        let material = get_enum(dm, id, "Material");
        let mp = material_physics(material);
        let custom = dm
            .get(id, "CustomPhysicalProperties")
            .ok()
            .and_then(|v| match v {
                Variant::PhysicalProperties(p) => p.custom,
                _ => None,
            });
        let (density, friction, elasticity, fw, ew) = match custom {
            Some(c) => (
                c.density,
                c.friction,
                c.elasticity,
                c.friction_weight,
                c.elasticity_weight,
            ),
            None => (
                mp.density,
                mp.friction,
                mp.elasticity,
                mp.friction_weight,
                mp.elasticity_weight,
            ),
        };
        let mass = match self.mode {
            PhysicalMode::Legacy => shape.volume,
            PhysicalMode::New => shape.volume * density,
        };
        let mut surfaces = [0u32; 6];
        let mut surface_inputs = [(0u32, 0.0f32, 0.0f32); 6];
        for (i, (s, inp, a, b)) in SURFACE_PROPS.iter().enumerate() {
            surfaces[i] = get_enum(dm, id, s);
            let pa = dm.get(id, a).ok().and_then(|v| v.as_f32()).unwrap_or(0.0);
            let pb = dm.get(id, b).ok().and_then(|v| v.as_f32()).unwrap_or(0.0);
            surface_inputs[i] = (get_enum(dm, id, inp), pa, pb);
        }
        let cframe = dm
            .get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
        let aabb = shape
            .collider
            .compute_aabb(&to_pose(&(cframe * shape.local)));
        Part {
            id,
            sort_size: sort_size(size, size_multiplier(dm, id)),
            material,
            shape,
            cframe,
            anchored: dm.get_bool(id, "Anchored"),
            can_collide: dm.get_bool(id, "CanCollide"),
            mass: mass.max(1e-6),
            friction,
            elasticity,
            friction_weight: fw,
            elasticity_weight: ew,
            legacy_friction: dm
                .get(id, "Friction")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.3),
            legacy_elasticity: dm
                .get(id, "Elasticity")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.5),
            surfaces,
            surface_inputs,
            velocity: get_vec3(dm, id, "Velocity"),
            rot_velocity: get_vec3(dm, id, "RotVelocity"),
            assembly: usize::MAX,
            offset: CFrame::default(),
            aabb,
            moved: false,
        }
    }

    pub fn upsert_part(&mut self, dm: &DataModel, id: InstanceId) {
        let mut part = self.read_part(dm, id);
        self.spatial.update(id, part.aabb);
        match self.index.get(&id) {
            Some(&i) => {
                let old = &self.parts[i];
                part.assembly = old.assembly;
                part.offset = old.offset;
                self.parts[i] = part;
            }
            None => {
                self.index.insert(id, self.parts.len());
                self.parts.push(part);
            }
        }
        self.topology_dirty = true;
        if dm.class_of(id) == Some("VehicleSeat") {
            self.vehicle_seats.insert(id);
        }
    }

    pub fn remove_part(&mut self, id: InstanceId) {
        let Some(i) = self.index.remove(&id) else {
            return;
        };
        self.spatial.remove(id);
        self.vehicle_seats.remove(&id);
        self.parts.swap_remove(i);
        if i < self.parts.len() {
            let moved = self.parts[i].id;
            self.index.insert(moved, i);
        }
        self.contacts.retain(|k, _| k.0 != id && k.1 != id);
        self.topology_dirty = true;
    }

    fn property_changed(&mut self, dm: &DataModel, id: InstanceId, prop: &'static str) {
        if Some(id) == self.workspace {
            if matches!(prop, "FallenPartsDestroyHeight" | "PhysicalPropertiesMode") {
                let old = self.mode;
                self.read_workspace(dm);
                if old != self.mode {
                    let ids: Vec<_> = self.parts.iter().map(|p| p.id).collect();
                    for p in ids {
                        self.upsert_part(dm, p);
                    }
                }
            }
            return;
        }
        if let Some(&i) = self.index.get(&id) {
            match prop {
                "CFrame" => {
                    let cf = dm
                        .get_raw(id, "CFrame")
                        .and_then(|v| v.as_cframe())
                        .unwrap_or_default();
                    self.move_part_external(i, cf);
                }
                "Velocity" | "RotVelocity" => {
                    let v = get_vec3(dm, id, prop);
                    self.set_part_velocity(i, prop == "Velocity", v);
                }
                "CanCollide" => {
                    self.parts[i].can_collide = dm.get_bool(id, "CanCollide");
                    self.wake_index(i);
                }
                "Anchored"
                | "size"
                | "shape"
                | "Material"
                | "CustomPhysicalProperties"
                | "Friction"
                | "Elasticity" => {
                    self.upsert_part(dm, id);
                }
                _ => {
                    if SURFACE_PROPS
                        .iter()
                        .any(|s| s.0 == prop || s.1 == prop || s.2 == prop || s.3 == prop)
                    {
                        let p = self.read_part(dm, id);
                        self.parts[i].surfaces = p.surfaces;
                        self.parts[i].surface_inputs = p.surface_inputs;
                    }
                }
            }
            return;
        }
        if let Some(j) = self.joints.get(&id) {
            let kind = j.kind;
            match prop {
                "DesiredAngle" | "MaxVelocity" => {
                    let v = dm
                        .get(id, prop)
                        .ok()
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0);
                    if let Some(j) = self.joints.get_mut(&id) {
                        if prop == "DesiredAngle" {
                            j.desired_angle = v;
                        } else {
                            j.max_velocity = v;
                        }
                    }
                    self.wake_joint(id);
                }
                "CurrentAngle" => {
                    let v = dm
                        .get(id, prop)
                        .ok()
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0);
                    if let Some(j) = self.joints.get_mut(&id)
                        && j.current_angle != v
                    {
                        j.current_angle = v;
                        if j.kind == JointKind::Motor6D {
                            j.current_axis_angle =
                                Vec3::new(0.0, 0.0, v * j.pose_mask_weight) + j.pose_axis_angle;
                        }
                        j.angle_dirty = true;
                    }
                    self.wake_joint(id);
                }
                "BaseAngle" => {
                    let v = dm
                        .get(id, prop)
                        .ok()
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0);
                    if let Some(j) = self.joints.get_mut(&id) {
                        j.base_angle = v;
                    }
                }
                _ => self.upsert_joint(dm, id, kind),
            }
            return;
        }
        if self.movers.contains_key(&id) {
            self.upsert_mover(dm, id);
        }
        if let Some(h) = self.humanoids.get_mut(&id) {
            match prop {
                "WalkToPoint" => {
                    h.set_walk_mode(true);
                    h.move_direction = Vec3::ZERO;
                }
                "WalkToPart" => {
                    let w = h.is_walking;
                    h.set_walk_mode(w);
                }
                "WalkDirection" => {
                    let v = dm
                        .get(id, "WalkDirection")
                        .ok()
                        .and_then(|v| v.as_vec3())
                        .unwrap_or(Vec3::ZERO);
                    if v != Vec3::ZERO {
                        h.is_walking = false;
                        h.move_direction = Vec3::ZERO;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn wake_part(&mut self, id: InstanceId) {
        if let Some(&i) = self.index.get(&id) {
            self.wake_index(i);
        }
    }

    fn wake_index(&mut self, i: usize) {
        let a = self.parts[i].assembly;
        if let Some(asm) = self.assemblies.get_mut(a)
            && !asm.anchored
        {
            asm.sleeping = false;
            asm.history.wake();
        }
    }

    pub fn apply_motor_pose(
        &mut self,
        id: InstanceId,
        offset: Vec3,
        axis_angle: Vec3,
        weight: f32,
        mask_weight: f32,
    ) {
        let Some(j) = self.joints.get_mut(&id) else {
            return;
        };
        match j.kind {
            JointKind::Motor | JointKind::VelocityMotor => {
                j.pose_angle = axis_angle.z * weight;
            }
            JointKind::Motor6D => {
                j.pose_offset = offset * weight;
                j.pose_axis_angle = axis_angle * weight;
            }
            _ => return,
        }
        j.pose_mask_weight = mask_weight;
        j.pose_freshness = 32;
    }

    fn wake_joint(&mut self, id: InstanceId) {
        if let Some(j) = self.joints.get(&id) {
            let ps = [j.part0, j.part1];
            for p in ps.into_iter().flatten() {
                self.wake_part(p);
            }
        }
    }

    fn move_part_external(&mut self, i: usize, cf: CFrame) {
        let a = self.parts[i].assembly;
        if self.topology_dirty || a >= self.assemblies.len() {
            self.parts[i].cframe = cf;
            self.spatial_dirty.push(self.parts[i].id);
            self.parts[i].moved = true;
            return;
        }
        let offset = self.parts[i].offset;
        let root_cf = (cf * offset.inverse()).orthonormalized();
        let asm = &mut self.assemblies[a];
        asm.orientation = root_cf.rotation;
        asm.position = root_cf * asm.com_local;
        asm.sleeping = false;
        asm.history.wake();
        let members = asm.parts.clone();
        for p in members {
            let pcf = if p == i {
                cf
            } else {
                root_cf * self.parts[p].offset
            };
            self.parts[p].cframe = pcf;
            self.spatial_dirty.push(self.parts[p].id);
            self.parts[p].moved = p != i;
            let id = self.parts[p].id;
            self.solver.clear_body_cache(id.raw());
            for c in self.contacts.values_mut() {
                if c.a == id || c.b == id {
                    c.workspace = None;
                    c.manifolds.clear();
                }
            }
        }
        self.assemblies[a].moved = true;
    }

    fn set_part_velocity(&mut self, i: usize, linear: bool, v: Vec3) {
        if linear {
            self.parts[i].velocity = v;
        } else {
            self.parts[i].rot_velocity = v;
        }
        let a = self.parts[i].assembly;
        if self.topology_dirty || a >= self.assemblies.len() {
            return;
        }
        let pos = self.parts[i].cframe.translation;
        let asm = &mut self.assemblies[a];
        if asm.anchored {
            return;
        }
        if linear {
            asm.linear = v - asm.angular.cross(pos - asm.position);
        } else {
            let vp = asm.velocity_at(pos);
            asm.angular = v;
            asm.linear = vp - v.cross(pos - asm.position);
        }
        asm.sleeping = false;
        asm.history.wake();
    }

    pub fn apply_network_state(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        cframe: CFrame,
        linear: Vec3,
        angular: Vec3,
    ) {
        let Some(&i) = self.index.get(&id) else {
            return;
        };
        if self.parts[i].anchored {
            return;
        }
        dm.store_silent(id, "CFrame", Variant::CFrame(cframe));
        dm.store_silent(id, "Velocity", Variant::Vector3(linear));
        dm.store_silent(id, "RotVelocity", Variant::Vector3(angular));
        self.move_part_external(i, cframe);
        self.set_part_velocity(i, false, angular);
        self.set_part_velocity(i, true, linear);
    }

    pub fn network_roots(&self) -> Vec<NetworkRoot> {
        let mut out = Vec::new();
        for a in &self.assemblies {
            if a.anchored || a.parts.is_empty() || a.root >= self.parts.len() {
                continue;
            }
            let root = &self.parts[a.root];
            let pos = root.cframe.translation;
            out.push(NetworkRoot {
                id: root.id,
                sleeping: a.sleeping,
                cframe: root.cframe,
                velocity: a.velocity_at(pos),
                rot_velocity: a.angular,
                parts: a.parts.iter().map(|&p| self.parts[p].id).collect(),
            });
        }
        out
    }

    pub fn active_joint(&self, j: &Joint) -> Option<(usize, usize)> {
        if !j.in_world {
            return None;
        }
        let a = *self.index.get(&j.part0?)?;
        let b = *self.index.get(&j.part1?)?;
        if a == b { None } else { Some((a, b)) }
    }

    pub fn rebuild_assemblies(&mut self) {
        if !self.topology_dirty {
            return;
        }
        self.topology_dirty = false;
        let n = self.parts.len();
        let mut old_vel: Vec<(Vec3, Vec3, bool)> = vec![(Vec3::ZERO, Vec3::ZERO, false); n];
        for (i, p) in self.parts.iter().enumerate() {
            if let Some(a) = self
                .assemblies
                .get(p.assembly)
                .filter(|_| p.assembly != usize::MAX)
            {
                if a.parts.contains(&i) {
                    old_vel[i] = (a.velocity_at(p.cframe.translation), a.angular, a.sleeping);
                } else {
                    old_vel[i] = (p.velocity, p.rot_velocity, false);
                }
            } else {
                old_vel[i] = (p.velocity, p.rot_velocity, false);
            }
        }
        let mut adj: Vec<Vec<(usize, InstanceId, bool)>> = vec![Vec::new(); n];
        let mut joint_ids: Vec<InstanceId> = self.joints.keys().copied().collect();
        joint_ids.sort();
        for jid in &joint_ids {
            let j = &self.joints[jid];
            if !j.kind.is_rigid() {
                continue;
            }
            if let Some((a, b)) = self.active_joint(j) {
                adj[a].push((b, *jid, true));
                adj[b].push((a, *jid, false));
            }
        }
        let mut comp = vec![usize::MAX; n];
        let mut assemblies: Vec<Assembly> = Vec::new();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| self.parts[i].id);
        for &start in &order {
            if comp[start] != usize::MAX {
                continue;
            }
            let idx = assemblies.len();
            let mut members = vec![start];
            comp[start] = idx;
            let mut k = 0;
            while k < members.len() {
                let p = members[k];
                k += 1;
                for &(q, _, _) in &adj[p] {
                    if comp[q] == usize::MAX {
                        comp[q] = idx;
                        members.push(q);
                    }
                }
            }
            let anchored = members.iter().any(|&p| self.parts[p].anchored);
            let root = *members
                .iter()
                .max_by(|&&x, &&y| {
                    let px = &self.parts[x];
                    let py = &self.parts[y];
                    (px.anchored, px.sort_size, std::cmp::Reverse(px.id)).cmp(&(
                        py.anchored,
                        py.sort_size,
                        std::cmp::Reverse(py.id),
                    ))
                })
                .unwrap_or(&start);
            let mut tree = Vec::new();
            let mut visited = HashSet::new();
            visited.insert(root);
            let mut queue = VecDeque::new();
            queue.push_back(root);
            while let Some(p) = queue.pop_front() {
                let mut edges = adj[p].clone();
                edges.sort_by_key(|e| e.1);
                for (q, jid, p_is_part0) in edges {
                    if visited.insert(q) {
                        tree.push(TreeEdge {
                            joint: jid,
                            parent: p,
                            child: q,
                            parent_is_part0: p_is_part0,
                        });
                        queue.push_back(q);
                    }
                }
            }
            assemblies.push(Assembly {
                parts: members,
                root,
                anchored,
                mass: 0.0,
                com_local: Vec3::ZERO,
                inertia_local: Mat3::IDENTITY,
                inv_inertia_local: Mat3::IDENTITY,
                position: Vec3::ZERO,
                orientation: Mat3::IDENTITY,
                linear: Vec3::ZERO,
                angular: Vec3::ZERO,
                uid: self.parts[root].id.raw(),
                sleeping: false,
                sleep_checking: false,
                woke: false,
                history: SleepHistory::new(),
                tree,
                force: Vec3::ZERO,
                torque: Vec3::ZERO,
                radius: 0.0,
                moved: true,
                kinematic: false,
            });
        }
        for (i, c) in comp.iter().enumerate() {
            self.parts[i].assembly = *c;
        }
        self.assemblies = assemblies;
        for a in 0..self.assemblies.len() {
            self.snap_tree(a);
            self.compute_mass_properties(a);
            let root = self.assemblies[a].root;
            let (v, w, sleeping) = old_vel[root];
            let rp = self.parts[root].cframe.translation;
            let asm = &mut self.assemblies[a];
            if asm.anchored {
                asm.linear = self.parts[root].velocity;
                asm.angular = self.parts[root].rot_velocity;
            } else {
                asm.angular = w;
                asm.linear = v + w.cross(asm.position - rp);
                let all_sleeping = asm.parts.iter().all(|&p| old_vel[p].2);
                asm.sleeping = sleeping && all_sleeping;
            }
        }
        self.contacts.retain(|_, c| {
            let (Some(&a), Some(&b)) = (self.index.get(&c.a), self.index.get(&c.b)) else {
                return false;
            };
            self.parts[a].assembly != self.parts[b].assembly
        });
        self.flush_spatial();
    }

    fn snap_tree(&mut self, a: usize) {
        let tree = self.assemblies[a].tree.clone();
        for e in &tree {
            let Some(j) = self.joints.get(&e.joint) else {
                continue;
            };
            let rel = j.child_in_parent(e.parent_is_part0);
            if !self.parts[e.child].anchored {
                let cf = self.parts[e.parent].cframe * rel;
                if cf != self.parts[e.child].cframe {
                    self.parts[e.child].cframe = cf;
                    self.spatial_dirty.push(self.parts[e.child].id);
                    self.parts[e.child].moved = true;
                }
            }
        }
    }

    pub fn compute_mass_properties(&mut self, a: usize) {
        let root = self.assemblies[a].root;
        let root_cf = self.parts[root].cframe.orthonormalized();
        let inv_root = root_cf.inverse();
        let members = self.assemblies[a].parts.clone();
        let mut mass = 0.0f32;
        let mut weighted = Vec3::ZERO;
        let mut radius = 0.0f32;
        for &p in &members {
            let part = &mut self.parts[p];
            part.offset = (inv_root * part.cframe).orthonormalized();
            let m = part.mass;
            mass += m;
            weighted += (part.offset * part.shape.com) * m;
        }
        let com_local = if mass > 0.0 {
            weighted / mass
        } else {
            Vec3::ZERO
        };
        let mut inertia = Mat3::ZERO;
        for &p in &members {
            let part = &self.parts[p];
            let local = rotate_inertia(part.shape.unit_inertia * part.mass, part.offset.rotation);
            let part_com = part.offset * part.shape.com;
            inertia += shift_inertia(local, part.mass, part_com - com_local);
            let origin = part.offset.translation;
            radius = radius.max((origin - com_local).length() + part.shape.extents_radius());
        }
        let asm = &mut self.assemblies[a];
        asm.mass = mass;
        asm.com_local = com_local;
        asm.inertia_local = inertia;
        asm.inv_inertia_local = safe_inverse(&inertia);
        asm.orientation = root_cf.rotation;
        asm.position = root_cf * com_local;
        asm.radius = radius;
    }

    pub fn update_assembly_offsets(&mut self, a: usize) {
        let root_cf = self.assemblies[a].root_cframe();
        let tree = self.assemblies[a].tree.clone();
        let root = self.assemblies[a].root;
        self.parts[root].cframe = root_cf;
        self.spatial_dirty.push(self.parts[root].id);
        for e in &tree {
            let Some(j) = self.joints.get(&e.joint) else {
                continue;
            };
            let rel = j.child_in_parent(e.parent_is_part0);
            self.parts[e.child].cframe = self.parts[e.parent].cframe * rel;
            self.spatial_dirty.push(self.parts[e.child].id);
        }
        let v = self.assemblies[a].linear;
        let w = self.assemblies[a].angular;
        let old_com = self.assemblies[a].position;
        self.compute_mass_properties(a);
        let asm = &mut self.assemblies[a];
        asm.linear = v + w.cross(asm.position - old_com);
        for &p in &asm.parts {
            self.parts[p].moved = true;
        }
        asm.moved = true;
    }

    pub fn sync_part_frames(&mut self, a: usize) {
        let root_cf = self.assemblies[a].root_cframe();
        let members = self.assemblies[a].parts.clone();
        for p in members {
            let cf = root_cf * self.parts[p].offset;
            self.parts[p].cframe = cf;
            self.spatial_dirty.push(self.parts[p].id);
            self.parts[p].moved = true;
        }
    }

    pub fn candidates(&self, aabb: &parry3d::bounding_volume::Aabb) -> Vec<usize> {
        let mut ids = Vec::new();
        self.spatial.query(aabb, &mut ids);
        ids.iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect()
    }

    pub fn flush_spatial(&mut self) {
        let dirty = std::mem::take(&mut self.spatial_dirty);
        for id in dirty {
            if let Some(&i) = self.index.get(&id) {
                let p = &mut self.parts[i];
                p.aabb = p
                    .shape
                    .collider
                    .compute_aabb(&to_pose(&(p.cframe * p.shape.local)));
                let aabb = p.aabb;
                self.spatial.update(id, aabb);
            }
        }
    }

    pub fn take_events(&mut self) -> StepEvents {
        let mut e = std::mem::take(&mut self.pending_events);
        e.humanoid = std::mem::take(&mut self.humanoid_events);
        e
    }

    pub(crate) fn events_mut(&mut self) -> &mut StepEvents {
        &mut self.pending_events
    }

    pub fn write_back(&mut self, dm: &mut DataModel) {
        self.apply_dm_writes(dm);
        for a in 0..self.assemblies.len() {
            if !self.assemblies[a].moved {
                continue;
            }
            self.assemblies[a].moved = false;
            let members = self.assemblies[a].parts.clone();
            for p in members {
                let asm = &self.assemblies[a];
                let part = &self.parts[p];
                let (v, w) = if asm.anchored {
                    (part.velocity, part.rot_velocity)
                } else {
                    (asm.velocity_at(part.cframe.translation), asm.angular)
                };
                let id = part.id;
                let cf = part.cframe;
                let moved = part.moved;
                if moved {
                    dm.store_silent(id, "CFrame", Variant::CFrame(cf));
                }
                if !asm.anchored {
                    dm.store_silent(id, "Velocity", Variant::Vector3(v));
                    dm.store_silent(id, "RotVelocity", Variant::Vector3(w));
                    let part = &mut self.parts[p];
                    part.velocity = v;
                    part.rot_velocity = w;
                }
                self.parts[p].moved = false;
            }
        }
        let motors: Vec<(InstanceId, f32)> = self
            .joints
            .values()
            .filter(|j| j.kind.is_motor())
            .map(|j| (j.id, j.current_angle))
            .collect();
        for (id, angle) in motors {
            if dm.get_raw(id, "CurrentAngle").and_then(|v| v.as_f32()) != Some(angle) {
                dm.store_silent(id, "CurrentAngle", Variant::Float(angle));
            }
        }
    }

    pub fn num_awake_parts(&self) -> usize {
        self.assemblies
            .iter()
            .filter(|a| a.is_moving())
            .map(|a| a.parts.len())
            .sum()
    }

    pub fn surface_normal_id(c: &CFrame) -> NormalId {
        vector_to_normal_id(c.rotation.z_axis)
    }

    pub fn face_normal(id: NormalId) -> Vec3 {
        normal_id_to_vector(id)
    }

    pub fn ensure_joint_constraints(&mut self, jid: InstanceId) {
        let Some(j) = self.joints.get(&jid) else {
            return;
        };
        if !j.constraints.is_empty() {
            return;
        }
        let kind = j.kind;
        let c0 = j.c0;
        let c1 = j.c1;
        let glue = j.glue_faces;
        let mut out = Vec::new();
        match kind {
            JointKind::Rotate | JointKind::RotateP | JointKind::RotateV => {
                out.push(Constraint::new(ConstraintKind::BallInSocket {
                    pivot_a: c0.translation,
                    pivot_b: c1.translation,
                }));
                out.push(Constraint::new(ConstraintKind::align2_axes(
                    c0.look_vector(),
                    c1.look_vector(),
                )));
                match kind {
                    JointKind::RotateP => {
                        out.push(Constraint::new(ConstraintKind::align2_axes(
                            c0.up_vector(),
                            c1.up_vector(),
                        )));
                    }
                    JointKind::RotateV => {
                        out.push(Constraint::new(ConstraintKind::AngularVelocity {
                            axis_b: c1.look_vector(),
                            max_force: 0.0,
                            desired: 0.0,
                        }));
                    }
                    _ => {}
                }
            }
            JointKind::Glue | JointKind::ManualGlue => {
                let (Some(p0), Some(p1)) = (j.part0, j.part1) else {
                    return;
                };
                let (Some(&a), Some(&b)) = (self.index.get(&p0), self.index.get(&p1)) else {
                    return;
                };
                let corners = if glue.iter().any(|f| *f != Vec3::ZERO) {
                    glue
                } else {
                    let size_a = self.parts[a].shape.size;
                    let n = c0.look_vector();
                    let (u, v) = crate::math::orthonormal_basis(n);
                    let ext = |d: Vec3| (size_a * 0.5).dot(d.abs());
                    let eu = ext(u);
                    let ev = ext(v);
                    [
                        u * eu + v * ev,
                        u * eu - v * ev,
                        -u * eu - v * ev,
                        -u * eu + v * ev,
                    ]
                };
                let n_local = c0.look_vector();
                let overlap = {
                    let d = (corners[0] - corners[2]).abs();
                    let mut dims = [d.x, d.y, d.z];
                    dims.sort_by(|x, y| y.total_cmp(x));
                    (dims[0], dims[1])
                };
                let max_force = glue_max_force(overlap.0, overlap.1);
                let pa = &self.parts[a];
                let pb = &self.parts[b];
                for corner in corners {
                    let in_a = c0 * corner;
                    let world = pa.cframe * in_a;
                    let in_b = pb.cframe.point_to_object_space(world);
                    out.push(Constraint::new(ConstraintKind::breakable_ball_in_socket(
                        in_a, in_b, n_local, max_force,
                    )));
                }
            }
            _ => {}
        }
        if let Some(j) = self.joints.get_mut(&jid) {
            j.constraints = out;
        }
    }
}

pub fn glue_max_force(grid1: f32, grid2: f32) -> f32 {
    const MEASURED: [f32; 7] = [0.0, 1.0980, 2.1340, 2.4270, 3.1910, 4.5710, 4.6810];
    let g1 = (grid1.round() as i32).max(1);
    let g2 = (grid2.round() as i32).max(1);
    let overlap = g1.max(g2);
    let width = g1.min(g2);
    let edge = if (overlap as usize) < MEASURED.len() {
        MEASURED[overlap as usize]
    } else {
        MEASURED[MEASURED.len() - 1] * overlap as f32 / MEASURED.len() as f32
    };
    let point = edge * 0.5 * width as f32;
    let lego_grid_mass = 0.4 * 0.001 * 0.333_333_33;
    point / lego_grid_mass * 20.0
}
