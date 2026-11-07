use crate::world::{JointKind, PhysicsWorld};
use rbx_dom::{DataModel, InstanceId};
use rbx_math::Vec3;

const THROTTLE_STEER_RIGHT_SPEED_TURN: [[[[[i32; 2]; 2]; 2]; 3]; 3] = [
    [
        [[[0, -1], [0, 0]], [[-1, -1], [0, 0]]],
        [[[-1, -1], [0, 0]], [[-1, -1], [0, 0]]],
        [[[-1, -1], [0, 0]], [[0, 1], [0, 0]]],
    ],
    [
        [[[-1, 0], [0, 0]], [[1, 0], [0, 0]]],
        [[[0, 0], [0, 0]], [[0, 0], [0, 0]]],
        [[[1, 0], [0, 0]], [[-1, 0], [0, 0]]],
    ],
    [
        [[[0, 1], [0, 0]], [[1, 1], [0, 0]]],
        [[[1, 1], [0, 0]], [[1, 1], [0, 0]]],
        [[[1, 1], [0, 0]], [[0, 1], [0, 0]]],
    ],
];

pub fn lookup_function(
    throttle: i32,
    steer: i32,
    on_right: bool,
    over_max_speed: bool,
    over_turn_speed: bool,
    joint_velocity: f32,
) -> i32 {
    let th = (throttle + 1).clamp(0, 2) as usize;
    let st = (steer + 1).clamp(0, 2) as usize;
    let forward_backward = THROTTLE_STEER_RIGHT_SPEED_TURN[th][st][on_right as usize]
        [over_max_speed as usize][over_turn_speed as usize];
    if forward_backward != 0 {
        if on_right {
            forward_backward
        } else {
            -forward_backward
        }
    } else if joint_velocity > 0.0 {
        -1
    } else {
        1
    }
}

struct SeatControl {
    throttle: i32,
    steer: i32,
    max_speed: f32,
    turn_speed: f32,
    torque: f32,
}

fn read_control(dm: &DataModel, id: InstanceId) -> SeatControl {
    let int = |n: &str| {
        dm.get(id, n)
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            .round() as i32
    };
    let float = |n: &str, d: f32| dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(d);
    SeatControl {
        throttle: int("Throttle").clamp(-1, 1),
        steer: int("Steer").clamp(-1, 1),
        max_speed: float("MaxSpeed", 25.0),
        turn_speed: float("TurnSpeed", 1.0),
        torque: float("Torque", 10.0),
    }
}

pub struct SeatHinge {
    pub joint: InstanceId,
    pub on_right: bool,
    pub axle_pointing_in: bool,
}

impl PhysicsWorld {
    pub fn vehicle_seat_hinges(&self, seat: InstanceId) -> Vec<SeatHinge> {
        let Some(&si) = self.index.get(&seat) else {
            return Vec::new();
        };
        let seat_asm = self.parts[si].assembly;
        let Some(asm) = self.assemblies.get(seat_asm) else {
            return Vec::new();
        };
        if asm.anchored {
            return Vec::new();
        }
        let seat_cf = self.parts[si].cframe;
        let seat_right = seat_cf.right_vector();
        let mut ids: Vec<InstanceId> = self
            .joints
            .values()
            .filter(|j| j.kind == JointKind::Rotate)
            .map(|j| j.id)
            .collect();
        ids.sort();
        let mut out = Vec::new();
        for id in ids {
            let j = &self.joints[&id];
            let Some((a, b)) = self.active_joint(j) else {
                continue;
            };
            if self.parts[a].assembly != seat_asm && self.parts[b].assembly != seat_asm {
                continue;
            }
            let coord0 = self.parts[a].cframe * j.c0;
            let seat_to_joint = coord0.translation - seat_cf.translation;
            let on_right = seat_right.dot(seat_to_joint) > 0.0;
            let axle = coord0.look_vector();
            let right_dot_axle = seat_right.dot(axle);
            if right_dot_axle.abs() <= 0.8 {
                continue;
            }
            let negative_dot = right_dot_axle < 0.0;
            out.push(SeatHinge {
                joint: id,
                on_right,
                axle_pointing_in: on_right == negative_dot,
            });
        }
        out
    }

    pub(crate) fn vehicle_seat_forces(&mut self, dm: &DataModel) {
        let seats: Vec<InstanceId> = self.vehicle_seats.iter().copied().collect();
        for seat in seats {
            let Some(&si) = self.index.get(&seat) else {
                continue;
            };
            let hinges = self.vehicle_seat_hinges(seat);
            if hinges.is_empty() {
                continue;
            }
            let control = read_control(dm, seat);
            let (linear, rotational) = self.part_velocity(si);
            let over_max_speed = linear.length() > control.max_speed;
            let over_turn_speed = rotational.y.abs() > control.turn_speed;
            let smoother = rbx_base::flags::flag_bool("SmootherVehicleSeatControlSystem", false);
            let wake = control.throttle != 0 || control.steer != 0;
            for h in hinges {
                let j = &self.joints[&h.joint];
                let Some((a, b)) = self.active_joint(j) else {
                    continue;
                };
                let axle = (self.parts[a].cframe * j.c0).look_vector();
                let (_, wa) = self.part_velocity(a);
                let (_, wb) = self.part_velocity(b);
                let mut joint_velocity = (wa - wb).dot(axle);
                if !h.axle_pointing_in {
                    joint_velocity = -joint_velocity;
                }
                let mut polarity = lookup_function(
                    control.throttle,
                    control.steer,
                    h.on_right,
                    over_max_speed,
                    over_turn_speed,
                    joint_velocity,
                );
                if !h.axle_pointing_in {
                    polarity = -polarity;
                }
                let mut gain = 1.0f32;
                if smoother {
                    if control.steer == 0 {
                        let previous = self
                            .seat_axle_velocities
                            .insert(h.joint, joint_velocity)
                            .unwrap_or(0.0);
                        let mut correction =
                            (rotational.y * control.throttle as f32 * 5.0).clamp(-1.0, 1.0);
                        if h.on_right {
                            correction = -correction;
                        }
                        correction = correction.max(0.0);
                        if control.throttle == 0 {
                            gain = (joint_velocity.abs() / 10.0).clamp(0.0, 1.0);
                            if joint_velocity * previous < 0.0 {
                                let delta = (previous - joint_velocity).abs();
                                gain *= joint_velocity.abs() / delta;
                            }
                        } else if control.max_speed > 0.0 {
                            gain =
                                ((linear.length() - control.max_speed).abs() - 0.5).clamp(0.0, 1.0);
                        }
                        gain += correction;
                    } else if control.turn_speed != 0.0 {
                        gain = 2.0 * (control.turn_speed - rotational.y.abs()) / control.turn_speed
                            + 1.0;
                    }
                }
                let applied = gain * control.torque * 1000.0;
                let torque: Vec3 = axle * (polarity as f32 * applied);
                let (aa, ab) = (self.parts[a].assembly, self.parts[b].assembly);
                for (ai, t) in [(aa, torque), (ab, -torque)] {
                    let Some(asm) = self.assemblies.get_mut(ai) else {
                        continue;
                    };
                    if asm.anchored {
                        continue;
                    }
                    asm.torque += t;
                    if wake && asm.sleeping {
                        asm.sleeping = false;
                        asm.history.wake();
                    }
                }
            }
        }
    }
}
