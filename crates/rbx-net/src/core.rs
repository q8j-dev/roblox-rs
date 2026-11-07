use crate::scope;
use crate::wire::{Message, NetId, NewInstance, PeerId};
use rbx_dom::{DataModel, InstanceId, Variant, coerce_to, database, default_value};
use std::collections::{HashMap, HashSet};

pub const NIL: NetId = NetId(0);
const SINGLETONS: &[&str] = &["Terrain"];

#[derive(Default)]
pub struct IdMap {
    pub me: PeerId,
    to_net: HashMap<InstanceId, NetId>,
    to_local: HashMap<NetId, InstanceId>,
}

impl IdMap {
    pub fn net(&self, id: InstanceId) -> NetId {
        self.to_net
            .get(&id)
            .copied()
            .unwrap_or_else(|| NetId::new(self.me, id.raw()))
    }

    pub fn net_opt(&self, id: Option<InstanceId>) -> NetId {
        id.map(|i| self.net(i)).unwrap_or(NIL)
    }

    pub fn local(&self, dm: &DataModel, n: NetId) -> Option<InstanceId> {
        if n == NIL {
            return None;
        }
        if let Some(&l) = self.to_local.get(&n) {
            return dm.contains(l).then_some(l);
        }
        if n.peer() != self.me {
            return None;
        }
        let id = InstanceId::from_raw(n.local())?;
        (!self.to_net.contains_key(&id) && dm.contains(id)).then_some(id)
    }

    pub fn bind(&mut self, local: InstanceId, net: NetId) {
        if net == NetId::new(self.me, local.raw()) {
            return;
        }
        if let Some(old) = self.to_net.insert(local, net) {
            self.to_local.remove(&old);
        }
        self.to_local.insert(net, local);
    }

    pub fn forget(&mut self, local: InstanceId) {
        if let Some(n) = self.to_net.remove(&local) {
            self.to_local.remove(&n);
        }
    }

    pub fn len(&self) -> usize {
        self.to_net.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_net.is_empty()
    }
}

#[derive(Default)]
pub struct View {
    pub viewer: Option<InstanceId>,
    pub known: HashMap<InstanceId, Option<InstanceId>>,
    fresh: HashSet<InstanceId>,
    sent: HashSet<(InstanceId, &'static str)>,
}

impl View {
    pub fn begin(&mut self) {
        self.fresh.clear();
        self.sent.clear();
    }

    pub fn knows(&self, id: InstanceId) -> bool {
        self.known.contains_key(&id)
    }

    pub fn forget_tree(&mut self, dm: &DataModel, id: InstanceId) {
        self.known.remove(&id);
        for d in dm.descendants(id) {
            self.known.remove(&d);
        }
    }
}

pub fn replicated_property(dm: &DataModel, id: InstanceId, name: &str) -> Option<&'static str> {
    if name == "Name" {
        return Some("Name");
    }
    let layout = dm.instance(id)?.layout;
    let canonical = layout.canonical(name);
    let desc = layout.descriptor(canonical)?;
    if !desc.can_replicate() || layout.computed.contains(canonical) {
        return None;
    }
    layout.slot(canonical)?;
    Some(desc.name.as_str())
}

#[derive(Default)]
pub struct Core {
    pub ids: IdMap,
    pending_refs: HashMap<NetId, Vec<(InstanceId, &'static str)>>,
    doomed: Vec<InstanceId>,
}

impl Core {
    pub fn collect(&mut self, dm: &DataModel) {
        for id in std::mem::take(&mut self.doomed) {
            if dm.parent_of(id).is_none() {
                self.ids.forget(id);
            }
        }
    }

    pub fn encode_value(&self, value: &Variant) -> Variant {
        match value {
            Variant::Ref(Some(id)) => Variant::Ref(InstanceId::from_raw(self.ids.net(*id).0)),
            other => other.clone(),
        }
    }

    fn decode_value(
        &mut self,
        dm: &DataModel,
        value: Variant,
        target: InstanceId,
        prop: &'static str,
    ) -> Variant {
        match value {
            Variant::Ref(Some(raw)) => {
                let net = NetId(raw.raw());
                match self.ids.local(dm, net) {
                    Some(l) => Variant::Ref(Some(l)),
                    None => {
                        self.pending_refs
                            .entry(net)
                            .or_default()
                            .push((target, prop));
                        Variant::Ref(None)
                    }
                }
            }
            other => other,
        }
    }

    pub fn properties(&self, dm: &DataModel, id: InstanceId, all: bool) -> Vec<(String, Variant)> {
        let Some(inst) = dm.instance(id) else {
            return Vec::new();
        };
        let layout = inst.layout;
        let mut out = Vec::new();
        for (slot, desc) in layout.slots.iter().enumerate() {
            if !desc.can_replicate() || layout.computed.contains(desc.name.as_str()) {
                continue;
            }
            let value = inst.value(slot);
            if !all && *value == default_value(inst.class, desc) {
                continue;
            }
            out.push((desc.name.clone(), self.encode_value(value)));
        }
        out
    }

    pub fn property_value(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        if prop == "Name" {
            return Some(Variant::String(dm.name_of(id).to_string()));
        }
        let inst = dm.instance(id)?;
        let v = inst.get_stored(prop)?;
        Some(self.encode_value(v))
    }

    fn snapshot_into(
        &self,
        dm: &DataModel,
        view: &View,
        id: InstanceId,
        out: &mut Vec<NewInstance>,
    ) {
        let Some(class) = dm.class_of(id) else {
            return;
        };
        out.push(NewInstance {
            id: self.ids.net(id),
            class: class.to_string(),
            name: dm.name_of(id).to_string(),
            parent: self.ids.net_opt(dm.parent_of(id)),
            properties: self.properties(dm, id, false),
        });
        for &c in dm.children_of(id) {
            if !view.knows(c) && scope::in_scope(dm, c, view.viewer) {
                self.snapshot_into(dm, view, c, out);
            }
        }
    }

    pub fn container_message(&self, dm: &DataModel, id: InstanceId) -> Option<Message> {
        let c = scope::container(dm, id)?;
        Some(Message::Service {
            id: self.ids.net(id),
            class: c.class.to_string(),
            properties: if c.properties {
                self.properties(dm, id, false)
            } else {
                Vec::new()
            },
        })
    }

    pub fn sync(&self, dm: &DataModel, view: &mut View, id: InstanceId, out: &mut Vec<Message>) {
        let should = scope::in_scope(dm, id, view.viewer);
        let known = view.known.get(&id).copied();
        let parent = dm.parent_of(id);
        match (should, known) {
            (true, None) => {
                if let Some(msg) = self.container_message(dm, id) {
                    out.push(msg);
                    view.known.insert(id, parent);
                    view.fresh.insert(id);
                    let mut items = Vec::new();
                    for &c in dm.children_of(id) {
                        if !view.knows(c) && scope::in_scope(dm, c, view.viewer) {
                            self.snapshot_into(dm, view, c, &mut items);
                        }
                    }
                    self.mark(dm, view, &items);
                    if !items.is_empty() {
                        out.push(Message::New(items));
                    }
                    return;
                }
                if parent.is_some_and(|p| !view.knows(p)) {
                    return;
                }
                let mut items = Vec::new();
                self.snapshot_into(dm, view, id, &mut items);
                self.mark(dm, view, &items);
                out.push(Message::New(items));
            }
            (true, Some(old)) => {
                if old != parent {
                    view.known.insert(id, parent);
                    out.push(Message::Parent {
                        id: self.ids.net(id),
                        parent: self.ids.net_opt(parent),
                    });
                }
            }
            (false, Some(_)) => {
                view.forget_tree(dm, id);
                out.push(Message::Delete {
                    id: self.ids.net(id),
                });
            }
            (false, None) => {}
        }
    }

    fn mark(&self, dm: &DataModel, view: &mut View, items: &[NewInstance]) {
        for item in items {
            if let Some(l) = self.ids.local(dm, item.id) {
                view.known.insert(l, dm.parent_of(l));
                view.fresh.insert(l);
            }
        }
    }

    pub fn property_changed(
        &self,
        dm: &DataModel,
        view: &mut View,
        id: InstanceId,
        prop: &'static str,
        out: &mut Vec<Message>,
    ) {
        if !view.knows(id) || view.fresh.contains(&id) {
            return;
        }
        if let Some(c) = scope::container(dm, id)
            && !c.properties
        {
            return;
        }
        let Some(name) = replicated_property(dm, id, prop) else {
            return;
        };
        if !view.sent.insert((id, name)) {
            return;
        }
        if let Some(value) = self.property_value(dm, id, name) {
            out.push(Message::Property {
                id: self.ids.net(id),
                name: name.to_string(),
                value,
            });
        }
    }

    fn resolve_refs(&mut self, dm: &mut DataModel, net: NetId, local: InstanceId) {
        if let Some(list) = self.pending_refs.remove(&net) {
            for (inst, prop) in list {
                if dm.contains(inst) {
                    dm.set_unchecked(inst, prop, Variant::Ref(Some(local)));
                }
            }
        }
    }

    pub fn bind_service(
        &mut self,
        dm: &mut DataModel,
        net: NetId,
        class: &str,
    ) -> Option<InstanceId> {
        scope::container_of_class(class)?;
        let id = dm.get_service(class).ok()?;
        self.ids.bind(id, net);
        self.resolve_refs(dm, net, id);
        Some(id)
    }

    pub fn apply_properties(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        props: Vec<(String, Variant)>,
    ) {
        for (name, value) in props {
            self.apply_property(dm, id, &name, value);
        }
    }

    pub fn apply_property(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        name: &str,
        value: Variant,
    ) -> bool {
        let Some(prop) = replicated_property(dm, id, name) else {
            return false;
        };
        if prop == "Name" {
            if let Variant::String(s) = value {
                dm.set_name(id, &s);
                return true;
            }
            return false;
        }
        let Some(desc) = dm.instance(id).and_then(|i| i.layout.descriptor(prop)) else {
            return false;
        };
        let value = self.decode_value(dm, value, id, prop);
        match coerce_to(&desc.ty, value) {
            Ok(v) => {
                dm.set_unchecked(id, prop, v);
                true
            }
            Err(_) => false,
        }
    }

    pub fn create(
        &mut self,
        dm: &mut DataModel,
        item: &NewInstance,
        parent: InstanceId,
    ) -> Option<InstanceId> {
        if let Some(existing) = self.ids.local(dm, item.id) {
            return Some(existing);
        }
        let class = database().class(&item.class)?.name.as_str();
        let reuse = SINGLETONS.contains(&class).then(|| {
            dm.children_of(parent)
                .iter()
                .copied()
                .find(|&c| dm.class_of(c) == Some(class))
        });
        let id = match reuse.flatten() {
            Some(id) => id,
            None => dm.create_internal(class).ok()?,
        };
        self.ids.bind(id, item.id);
        dm.set_name(id, &item.name);
        self.resolve_refs(dm, item.id, id);
        Some(id)
    }

    pub fn set_parent(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
        parent: Option<InstanceId>,
    ) -> bool {
        if dm.parent_of(id) == parent {
            return true;
        }
        dm.set_parent_ignore_lock(id, parent).is_ok()
    }

    pub fn delete(&mut self, dm: &mut DataModel, id: InstanceId) {
        let mut all = dm.descendants(id);
        all.push(id);
        if let Some(i) = dm.instance(id)
            && i.parent_locked
        {
            dm.lock_parent(id, false);
        }
        let _ = dm.destroy(id);
        self.doomed.extend(all);
    }

    pub fn event_name(dm: &DataModel, id: InstanceId, name: &str) -> Option<&'static str> {
        let class = dm.class_of(id)?;
        database().find_event(class, name).map(|e| e.name.as_str())
    }
}
