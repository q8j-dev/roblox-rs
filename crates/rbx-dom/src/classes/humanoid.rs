use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

pub struct HumanoidBehavior;

impl Behavior for HumanoidBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["MoveDirection", "Torso", "LeftLeg", "RightLeg"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "MoveDirection" => dm.get_raw(id, "MoveDirectionInternal").cloned(),
            "Torso" => Some(Variant::Ref(appendage(
                dm,
                id,
                &["HumanoidRootPart", "Torso", "UpperTorso"],
            ))),
            "LeftLeg" => Some(Variant::Ref(appendage(dm, id, &["Left Leg", "LeftLeg"]))),
            "RightLeg" => Some(Variant::Ref(appendage(dm, id, &["Right Leg", "RightLeg"]))),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        let f = || value.as_f32();
        match prop {
            "Torso" | "LeftLeg" | "RightLeg" => SetOutcome::Handled,
            "Health" => {
                let Some(v) = f() else {
                    return SetOutcome::Unhandled;
                };
                let max = dm
                    .get(id, "MaxHealth")
                    .ok()
                    .and_then(|m| m.as_f32())
                    .unwrap_or(100.0);
                SetOutcome::Replace(Variant::Float(v.clamp(0.0, max.max(0.0))))
            }
            "JumpPower" => match f() {
                Some(v) => SetOutcome::Replace(Variant::Float(v.clamp(0.0, 1000.0))),
                None => SetOutcome::Unhandled,
            },
            "MaxSlopeAngle" => match f() {
                Some(v) => SetOutcome::Replace(Variant::Float(v.clamp(0.0, 89.9))),
                None => SetOutcome::Unhandled,
            },
            "NameDisplayDistance" | "HealthDisplayDistance" => match f() {
                Some(v) => SetOutcome::Replace(Variant::Float(v.max(0.0))),
                None => SetOutcome::Unhandled,
            },
            _ => SetOutcome::Unhandled,
        }
    }
}

fn appendage(dm: &DataModel, id: InstanceId, names: &[&str]) -> Option<InstanceId> {
    let parent = dm.parent_of(id)?;
    names.iter().find_map(|n| {
        dm.children_of(parent)
            .iter()
            .copied()
            .find(|c| dm.name_of(*c) == *n && dm.is_a(*c, "BasePart"))
    })
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Humanoid", &HumanoidBehavior);
}
