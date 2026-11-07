use crate::behavior::{Behavior, SetOutcome};
use crate::classes::tool::{character_humanoid, right_arm_grip};
use crate::datamodel::DataModel;
use rbx_math::{CFrame, Vec3};
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

const SLEEP_TIME: f64 = 3.0;

#[derive(Default)]
pub struct Seats {
    sleep_time: HashMap<InstanceId, f64>,
    occupant: HashMap<InstanceId, InstanceId>,
    seat_part: HashMap<InstanceId, InstanceId>,
}

fn is_seat(dm: &DataModel, id: InstanceId) -> bool {
    matches!(dm.class_of(id), Some("Seat" | "VehicleSeat"))
}

fn humanoid_from_body_part(dm: &DataModel, part: Option<InstanceId>) -> Option<InstanceId> {
    character_humanoid(dm, dm.parent_of(part?))
}

fn torso_of_humanoid(dm: &DataModel, humanoid: InstanceId) -> Option<InstanceId> {
    let character = dm.parent_of(humanoid)?;
    ["HumanoidRootPart", "Torso"].iter().find_map(|n| {
        dm.children_of(character)
            .iter()
            .copied()
            .find(|c| dm.name_of(*c) == *n && dm.is_a(*c, "BasePart"))
    })
}

fn in_workspace(dm: &DataModel, id: InstanceId) -> bool {
    dm.find_service("Workspace")
        .is_some_and(|ws| dm.is_descendant_of(id, ws))
}

fn get_bool(dm: &DataModel, id: InstanceId, name: &str) -> bool {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

impl DataModel {
    fn is_child_seat_weld(&self, child: InstanceId) -> bool {
        self.name_of(child) == "SeatWeld" && self.is_a(child, "Weld")
    }

    fn find_seat_weld(&self, seat: InstanceId) -> Option<InstanceId> {
        self.children_of(seat)
            .iter()
            .copied()
            .find(|c| self.is_child_seat_weld(*c))
    }

    fn seat_sleep_time_up(&self, seat: InstanceId) -> bool {
        match self.seats.sleep_time.get(&seat) {
            Some(t) => self.game_time - t > SLEEP_TIME,
            None => true,
        }
    }

    fn local_humanoid(&self) -> Option<InstanceId> {
        let player = self.local_player?;
        let character = self.get_ref(player, "Character")?;
        character_humanoid(self, Some(character))
    }

    pub fn seats_on_touched(&mut self, seat: InstanceId, other: InstanceId) {
        if !is_seat(self, seat) || !self.in_tree(seat) {
            return;
        }
        let Some(h) = humanoid_from_body_part(self, Some(other)) else {
            return;
        };
        let Some(torso) = torso_of_humanoid(self, h) else {
            return;
        };
        let dead = self
            .get(h, "Health")
            .ok()
            .and_then(|v| v.as_f32())
            .is_some_and(|v| v <= 0.0);
        if self.seat_sleep_time_up(seat)
            && self.find_seat_weld(seat).is_none()
            && !get_bool(self, h, "Sit")
            && !dead
            && !get_bool(self, seat, "Disabled")
            && in_workspace(self, torso)
            && in_workspace(self, seat)
        {
            self.create_seat_weld(seat, h);
        }
    }

    pub fn create_seat_weld(&mut self, seat: InstanceId, h: InstanceId) {
        let Some(torso) = torso_of_humanoid(self, h) else {
            return;
        };
        if rbx_base::flags::flag_bool("FixSeatingWhileSitting", false) && get_bool(self, h, "Sit") {
            return;
        }
        let size = |id: InstanceId| {
            self.get(id, "Size")
                .ok()
                .and_then(|v| v.as_vec3())
                .unwrap_or(Vec3::ONE)
        };
        let seat_offset = size(seat).y * 0.5;
        let torso_offset = size(torso).y * 0.5;
        let fix = rbx_base::flags::flag_bool("FixAnchoredSeatingPosition", false);
        let rotation = right_arm_grip().rotation;
        let seat_coord = CFrame::new(rotation, Vec3::new(0.0, seat_offset, 0.0));
        let torso_coord = CFrame::new(
            rotation,
            Vec3::new(0.0, if fix { -(torso_offset + 0.5) } else { -1.5 }, 0.0),
        );
        let _ = self.set(torso, "Velocity", Variant::Vector3(Vec3::ZERO));
        if fix {
            let seat_cf = self
                .get(seat, "CFrame")
                .ok()
                .and_then(|v| v.as_cframe())
                .unwrap_or_default();
            let cf = seat_cf
                * CFrame::from_translation(Vec3::new(0.0, seat_offset + torso_offset + 0.5, 0.0));
            let _ = self.set(torso, "CFrame", Variant::CFrame(cf));
        }
        let Ok(weld) = self.create("Weld") else {
            return;
        };
        self.set_name(weld, "SeatWeld");
        let _ = self.set(weld, "Part0", Variant::Ref(Some(seat)));
        let _ = self.set(weld, "Part1", Variant::Ref(Some(torso)));
        let _ = self.set(weld, "C0", Variant::CFrame(seat_coord));
        let _ = self.set(weld, "C1", Variant::CFrame(torso_coord));
        let _ = self.set(weld, "Archivable", Variant::Bool(false));
        let _ = self.set_parent(weld, Some(seat));
    }

    pub fn seat_destroy_welds(&mut self, seat: InstanceId) {
        while let Some(w) = self.find_seat_weld(seat) {
            if self.set_parent(w, None).is_err() {
                break;
            }
        }
    }

    pub fn humanoid_done_sitting(&mut self, humanoid: InstanceId, seat: Option<InstanceId>) {
        let Some(seat) = seat else {
            return;
        };
        if !self.contains(seat) || !is_seat(self, seat) {
            return;
        }
        if self.seats.occupant.get(&seat) == Some(&humanoid) {
            self.seat_destroy_welds(seat);
        }
    }

    fn set_occupant(&mut self, seat: InstanceId, humanoid: Option<InstanceId>) {
        if self.seats.occupant.get(&seat).copied() == humanoid {
            return;
        }
        match humanoid {
            Some(h) => self.seats.occupant.insert(seat, h),
            None => self.seats.occupant.remove(&seat),
        };
        self.notify_changed(seat, "Occupant");
    }

    pub(crate) fn humanoid_seat_part(&self, humanoid: InstanceId) -> Option<InstanceId> {
        self.seats
            .seat_part
            .get(&humanoid)
            .copied()
            .filter(|s| self.contains(*s))
    }

    pub(crate) fn set_humanoid_seat_part(
        &mut self,
        humanoid: InstanceId,
        seat: Option<InstanceId>,
    ) {
        if self.seats.seat_part.get(&humanoid).copied() == seat {
            return;
        }
        match seat {
            Some(s) => self.seats.seat_part.insert(humanoid, s),
            None => self.seats.seat_part.remove(&humanoid),
        };
        self.notify_changed(humanoid, "SeatPart");
    }

    fn seat_seated_changed(
        &mut self,
        seat: InstanceId,
        seated: bool,
        humanoid: Option<InstanceId>,
    ) {
        let Some(h) = humanoid else {
            return;
        };
        if Some(h) != self.local_humanoid() {
            return;
        }
        if self.class_of(seat) == Some("VehicleSeat") {
            let camera = self
                .find_service("Workspace")
                .and_then(|ws| self.get_ref(ws, "CurrentCamera"));
            if let Some(cam) = camera {
                if seated {
                    let _ = self.set(cam, "CameraType", Variant::Enum(5));
                    let _ = self.set(cam, "CameraSubject", Variant::Ref(Some(seat)));
                    if let Some(torso) = torso_of_humanoid(self, h) {
                        let tcf = self
                            .get(torso, "CFrame")
                            .ok()
                            .and_then(|v| v.as_cframe())
                            .unwrap_or_default();
                        let pos =
                            -tcf.look_vector() * 15.0 + tcf.translation + Vec3::new(0.0, 10.0, 0.0);
                        let _ = self.set(
                            cam,
                            "CoordinateFrame",
                            Variant::CFrame(CFrame::from_translation(pos)),
                        );
                    }
                } else {
                    let _ = self.set(cam, "CameraSubject", Variant::Ref(Some(h)));
                    let _ = self.set(cam, "CameraType", Variant::Enum(5));
                }
            }
            if !seated {
                let _ = self.set(h, "Sit", Variant::Bool(false));
            }
            let _ = self.set(seat, "Throttle", Variant::Int(0));
            let _ = self.set(seat, "Steer", Variant::Int(0));
        } else if !seated {
            let _ = self.set(h, "Sit", Variant::Bool(false));
        }
    }
}

pub struct SeatBehavior;

impl Behavior for SeatBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["Occupant"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "Occupant" => Some(Variant::Ref(
                dm.seats
                    .occupant
                    .get(&id)
                    .copied()
                    .filter(|h| dm.contains(*h)),
            )),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        match prop {
            "Disabled" => {
                if value.as_bool() == Some(true) && !get_bool(dm, id, "Disabled") {
                    while let Some(w) = dm.find_seat_weld(id) {
                        if let Some(h) = humanoid_from_body_part(dm, dm.get_ref(w, "Part1")) {
                            let _ = dm.set(h, "Sit", Variant::Bool(false));
                        }
                        if dm.set_parent(w, None).is_err() {
                            break;
                        }
                    }
                }
                SetOutcome::Unhandled
            }
            "Occupant" => SetOutcome::Handled,
            "Throttle" | "Steer" if dm.class_of(id) == Some("VehicleSeat") => {
                match value.as_f64() {
                    Some(v) => SetOutcome::Replace(Variant::Int((v as i32).clamp(-1, 1))),
                    None => SetOutcome::Unhandled,
                }
            }
            _ => SetOutcome::Unhandled,
        }
    }

    fn child_added(&self, dm: &mut DataModel, id: InstanceId, child: InstanceId) {
        if !dm.is_child_seat_weld(child) {
            return;
        }
        let others: Vec<InstanceId> = dm
            .children_of(id)
            .iter()
            .copied()
            .filter(|c| *c != child && dm.is_child_seat_weld(*c))
            .collect();
        for o in others {
            let _ = dm.set_parent(o, None);
        }
        if let Some(h) = humanoid_from_body_part(dm, dm.get_ref(child, "Part1")) {
            dm.set_occupant(id, Some(h));
            dm.set_humanoid_seat_part(h, Some(id));
            let _ = dm.set(h, "Sit", Variant::Bool(true));
            dm.seat_seated_changed(id, true, Some(h));
        }
    }

    fn child_removed(&self, dm: &mut DataModel, id: InstanceId, child: InstanceId) {
        if !dm.is_child_seat_weld(child) {
            return;
        }
        dm.set_occupant(id, None);
        let now = dm.game_time;
        dm.seats.sleep_time.insert(id, now);
        let h = humanoid_from_body_part(dm, dm.get_ref(child, "Part1"));
        dm.seat_seated_changed(id, false, h);
    }

    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        _child: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        if old_parent.is_some_and(|p| dm.in_tree(p)) && !new_parent.is_some_and(|p| dm.in_tree(p)) {
            let h = dm
                .find_seat_weld(id)
                .and_then(|w| humanoid_from_body_part(dm, dm.get_ref(w, "Part1")));
            dm.seat_seated_changed(id, false, h);
        }
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        dm.seats.sleep_time.remove(&id);
        dm.seats.occupant.remove(&id);
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Seat", &SeatBehavior);
    map.insert("VehicleSeat", &SeatBehavior);
}
