use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

pub struct HumanoidBehavior;

impl Behavior for HumanoidBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["MoveDirection"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "MoveDirection" => dm.get_raw(id, "MoveDirectionInternal").cloned(),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        let f = || value.as_f32();
        match prop {
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

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Humanoid", &HumanoidBehavior);
}
