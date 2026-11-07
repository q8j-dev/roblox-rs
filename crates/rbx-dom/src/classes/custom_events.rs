use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

#[derive(Default)]
pub struct CustomEvents {
    receivers: HashMap<InstanceId, Vec<InstanceId>>,
    source_of: HashMap<InstanceId, InstanceId>,
    value: HashMap<InstanceId, f32>,
}

impl DataModel {
    pub fn custom_event_value(&self, event: InstanceId) -> f32 {
        self.get_raw(event, "PersistedCurrentValue")
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0)
    }

    pub fn custom_receiver_value(&self, receiver: InstanceId) -> f32 {
        self.custom.value.get(&receiver).copied().unwrap_or(0.0)
    }

    pub fn custom_attached_receivers(&self, event: InstanceId) -> Vec<InstanceId> {
        self.custom
            .receivers
            .get(&event)
            .cloned()
            .unwrap_or_default()
    }

    fn receiver_value_changed(&mut self, receiver: InstanceId, value: f32) {
        self.custom.value.insert(receiver, value);
        self.fire_signal(receiver, "SourceValueChanged", vec![Variant::Float(value)]);
    }

    pub fn custom_event_set_value(&mut self, event: InstanceId, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.set_unchecked(event, "PersistedCurrentValue", Variant::Float(v));
        self.notify_changed(event, "PersistedCurrentValue");
        for r in self.custom_attached_receivers(event) {
            if self.custom_receiver_value(r) != v {
                self.receiver_value_changed(r, v);
            }
        }
    }

    pub fn custom_set_source(&mut self, receiver: InstanceId, source: Option<InstanceId>) {
        let current = self.custom.source_of.get(&receiver).copied();
        if current == source {
            return;
        }
        if let Some(old) = current {
            if let Some(list) = self.custom.receivers.get_mut(&old)
                && let Some(pos) = list.iter().position(|r| *r == receiver)
            {
                list.remove(pos);
                self.fire_signal(
                    old,
                    "ReceiverDisconnected",
                    vec![Variant::Ref(Some(receiver))],
                );
            }
            self.custom.source_of.remove(&receiver);
            self.fire_signal(receiver, "EventDisconnected", vec![Variant::Ref(Some(old))]);
        }
        let source = source.filter(|s| self.class_of(*s) == Some("CustomEvent"));
        if let Some(new) = source {
            let list = self.custom.receivers.entry(new).or_default();
            if !list.contains(&receiver) {
                list.push(receiver);
                self.fire_signal(new, "ReceiverConnected", vec![Variant::Ref(Some(receiver))]);
            }
            self.custom.source_of.insert(receiver, new);
            let v = self.custom_event_value(new);
            if self.custom_receiver_value(receiver) != v {
                self.receiver_value_changed(receiver, v);
            }
            self.fire_signal(receiver, "EventConnected", vec![Variant::Ref(Some(new))]);
        }
        if source.is_none() && self.custom_receiver_value(receiver) != 0.0 {
            self.receiver_value_changed(receiver, 0.0);
        }
        self.set_unchecked(receiver, "Source", Variant::Ref(source));
        self.notify_changed(receiver, "Source");
    }

    fn custom_left_world(&mut self, id: InstanceId) {
        match self.class_of(id) {
            Some("CustomEventReceiver") => self.custom_set_source(id, None),
            Some("CustomEvent") => {
                for r in self.custom_attached_receivers(id) {
                    self.custom_set_source(r, None);
                }
            }
            _ => {}
        }
    }
}

fn left_world(dm: &DataModel, id: InstanceId, old_parent: Option<InstanceId>) -> bool {
    old_parent.is_some_and(|p| dm.in_tree(p) || p == dm.root()) && !dm.in_tree(id)
}

pub struct CustomEventBehavior;

impl Behavior for CustomEventBehavior {
    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        _child: InstanceId,
        old_parent: Option<InstanceId>,
        _new_parent: Option<InstanceId>,
    ) {
        if left_world(dm, id, old_parent) {
            dm.custom_left_world(id);
        }
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        dm.custom_left_world(id);
        dm.custom.receivers.remove(&id);
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        if prop == "PersistedCurrentValue" {
            let v = value.as_f32().unwrap_or(0.0).clamp(0.0, 1.0);
            dm.set_unchecked(id, "PersistedCurrentValue", Variant::Float(v));
            return SetOutcome::Handled;
        }
        SetOutcome::Unhandled
    }
}

pub struct CustomEventReceiverBehavior;

impl Behavior for CustomEventReceiverBehavior {
    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        if prop == "Source" {
            let source = match value {
                Variant::Ref(r) => *r,
                _ => None,
            };
            dm.custom_set_source(id, source);
            return SetOutcome::Handled;
        }
        SetOutcome::Unhandled
    }

    fn changed(&self, dm: &mut DataModel, id: InstanceId, prop: &'static str) {
        if prop.is_empty() {
            let source = match dm.get_raw(id, "Source") {
                Some(Variant::Ref(r)) => *r,
                _ => None,
            };
            dm.set_unchecked(id, "Source", Variant::Ref(None));
            dm.custom.source_of.remove(&id);
            dm.custom_set_source(id, source);
        }
    }

    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        _child: InstanceId,
        old_parent: Option<InstanceId>,
        _new_parent: Option<InstanceId>,
    ) {
        if left_world(dm, id, old_parent) {
            dm.custom_left_world(id);
        }
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        dm.custom_set_source(id, None);
        dm.custom.value.remove(&id);
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("CustomEvent", &CustomEventBehavior);
    map.insert("CustomEventReceiver", &CustomEventReceiverBehavior);
}
