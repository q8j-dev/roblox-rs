use crate::behavior::{SetOutcome, behaviors_for};
use crate::classes::is_service_class;
use crate::defaults::default_value;
use crate::events::{DomEvent, EventCursor, EventLog};
use crate::layout::{ClassLayout, layout_for};
use rbx_reflection::{InstanceId, ValueType, Variant, database};
use std::collections::HashMap;

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum DomError {
    #[error("{0} is not a valid class name")]
    UnknownClass(String),
    #[error("Unable to create an Instance of type \"{0}\"")]
    NotCreatable(String),
    #[error("{0} is not a valid member of {1}")]
    UnknownProperty(String, String),
    #[error("{0}")]
    Message(String),
    #[error("instance no longer exists")]
    Gone,
    #[error("Unable to cast value to {0}")]
    BadValue(String),
    #[error("{0} is read-only")]
    ReadOnly(String),
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub id: InstanceId,
    pub class: &'static str,
    pub layout: &'static ClassLayout,
    pub name: String,
    pub parent: Option<InstanceId>,
    pub children: Vec<InstanceId>,
    pub archivable: bool,
    pub roblox_locked: bool,
    pub parent_locked: bool,
    pub external_refs: u32,
    pub(crate) values: Vec<Variant>,
    setting_parent: bool,
}

impl Instance {
    pub fn value(&self, slot: usize) -> &Variant {
        &self.values[slot]
    }

    pub fn get_stored(&self, name: &str) -> Option<&Variant> {
        self.layout.slot(name).map(|s| &self.values[s])
    }
}

#[allow(unused_variables)]
pub trait WorldHooks: Send {
    fn destroy_joints(&mut self, dm: &mut DataModel, id: InstanceId) {}
    fn join(&mut self, dm: &mut DataModel, id: InstanceId) {}
    fn safe_move(&mut self, dm: &mut DataModel, id: InstanceId) {}
    fn mechanism_root_for_script(
        &mut self,
        dm: &mut DataModel,
        id: InstanceId,
    ) -> Option<InstanceId> {
        None
    }
    fn pv_changed_from_reflection(&mut self, dm: &mut DataModel, id: InstanceId) {}
    fn joint_changed(&mut self, dm: &mut DataModel, id: InstanceId) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Output,
    Info,
    Warning,
    Error,
}

pub struct DataModel {
    instances: HashMap<InstanceId, Instance>,
    next_id: u64,
    root: InstanceId,
    pub events: EventLog,
    services: HashMap<&'static str, InstanceId>,
    pub game_time: f64,
    pub local_player: Option<InstanceId>,
    world_hooks: Option<Box<dyn WorldHooks>>,
    pub(crate) terrain_data: HashMap<InstanceId, crate::classes::terrain::TerrainData>,
    pub(crate) history: crate::history::History,
    pub(crate) custom: crate::classes::custom_events::CustomEvents,
    pub(crate) tools: crate::classes::tool::Tools,
    pub(crate) seats: crate::classes::seat::Seats,
    pub backend_processing: bool,
    messages: Vec<(MessageLevel, String)>,
    pub frontend_processing: bool,
    signals: std::collections::VecDeque<(InstanceId, &'static str, Vec<Variant>, u64)>,
    remote: Vec<crate::remote::RemoteCall>,
    deliveries: Vec<crate::remote::RemoteDelivery>,
    pub gui_view: crate::gui::GuiView,
}

impl Default for DataModel {
    fn default() -> Self {
        DataModel::new()
    }
}

fn class_static(name: &str) -> Option<&'static str> {
    database().class(name).map(|c| c.name.as_str())
}

impl DataModel {
    pub fn new() -> DataModel {
        Self::with_root_class("DataModel")
    }

    pub fn with_root_class(class: &str) -> DataModel {
        let mut dm = DataModel {
            instances: HashMap::new(),
            next_id: 1,
            root: match InstanceId::from_raw(1) {
                Some(id) => id,
                None => unreachable!(),
            },
            events: EventLog::default(),
            services: HashMap::new(),
            game_time: 0.0,
            local_player: None,
            world_hooks: None,
            terrain_data: HashMap::new(),
            history: Default::default(),
            custom: Default::default(),
            tools: Default::default(),
            seats: Default::default(),
            backend_processing: true,
            messages: Vec::new(),
            frontend_processing: true,
            signals: Default::default(),
            remote: Vec::new(),
            deliveries: Vec::new(),
            gui_view: crate::gui::GuiView::default(),
        };
        let root = match dm.create_internal(class) {
            Ok(id) => id,
            Err(_) => match dm.create_internal("Model") {
                Ok(id) => id,
                Err(e) => panic!("cannot create root instance: {e}"),
            },
        };
        dm.root = root;
        if let Some(r) = dm.instances.get_mut(&root) {
            r.parent_locked = true;
            if class == "DataModel" {
                r.name = "Game".to_string();
            }
        }
        dm
    }

    pub fn root(&self) -> InstanceId {
        self.root
    }

    pub fn set_world_hooks(&mut self, hooks: Box<dyn WorldHooks>) {
        self.world_hooks = Some(hooks);
    }

    pub fn take_world_hooks(&mut self) -> Option<Box<dyn WorldHooks>> {
        self.world_hooks.take()
    }

    pub fn with_world_hooks<R: Default>(
        &mut self,
        f: impl FnOnce(&mut dyn WorldHooks, &mut DataModel) -> R,
    ) -> R {
        let Some(mut hooks) = self.world_hooks.take() else {
            return R::default();
        };
        let r = f(hooks.as_mut(), self);
        if self.world_hooks.is_none() {
            self.world_hooks = Some(hooks);
        }
        r
    }

    pub fn subscribe(&mut self) -> EventCursor {
        self.events.subscribe()
    }

    pub fn drain_events(&mut self, cursor: EventCursor) -> Vec<DomEvent> {
        self.events.drain(cursor)
    }

    pub fn drain_events_from(&mut self, cursor: EventCursor) -> (u64, Vec<DomEvent>) {
        self.events.drain_from(cursor)
    }

    pub fn fire_signal(&mut self, id: InstanceId, event: &'static str, args: Vec<Variant>) {
        let seq = self.events.position();
        self.signals.push_back((id, event, args, seq));
    }

    pub fn queue_remote(&mut self, call: crate::remote::RemoteCall) {
        self.remote.push(call);
    }

    pub fn drain_remote(&mut self) -> Vec<crate::remote::RemoteCall> {
        std::mem::take(&mut self.remote)
    }

    pub fn deliver_remote(&mut self, delivery: crate::remote::RemoteDelivery) {
        self.deliveries.push(delivery);
    }

    pub fn drain_deliveries(&mut self) -> Vec<crate::remote::RemoteDelivery> {
        std::mem::take(&mut self.deliveries)
    }

    pub fn standard_out(&mut self, level: MessageLevel, text: impl Into<String>) {
        self.messages.push((level, text.into()));
    }

    pub fn drain_messages(&mut self) -> Vec<(MessageLevel, String)> {
        std::mem::take(&mut self.messages)
    }

    pub fn drain_signals(&mut self) -> Vec<(InstanceId, &'static str, Vec<Variant>, u64)> {
        self.signals.drain(..).collect()
    }

    pub fn event_position(&self) -> u64 {
        self.events.position()
    }

    pub fn contains(&self, id: InstanceId) -> bool {
        self.instances.contains_key(&id)
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(&id)
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.instances.keys().copied()
    }

    pub fn class_of(&self, id: InstanceId) -> Option<&'static str> {
        self.instances.get(&id).map(|i| i.class)
    }

    pub fn name_of(&self, id: InstanceId) -> &str {
        self.instances
            .get(&id)
            .map(|i| i.name.as_str())
            .unwrap_or("")
    }

    pub fn parent_of(&self, id: InstanceId) -> Option<InstanceId> {
        self.instances.get(&id).and_then(|i| i.parent)
    }

    pub fn children_of(&self, id: InstanceId) -> &[InstanceId] {
        self.instances
            .get(&id)
            .map(|i| i.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_a(&self, id: InstanceId, class: &str) -> bool {
        match self.class_of(id) {
            Some(c) => c == class || database().is_a(c, class),
            None => false,
        }
    }

    pub fn is_ancestor_of(&self, ancestor: InstanceId, descendant: Option<InstanceId>) -> bool {
        let mut cur = descendant.and_then(|d| self.parent_of(d));
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent_of(c);
        }
        false
    }

    pub fn is_descendant_of(&self, id: InstanceId, ancestor: InstanceId) -> bool {
        self.is_ancestor_of(ancestor, Some(id))
    }

    pub fn in_tree(&self, id: InstanceId) -> bool {
        id == self.root || self.is_descendant_of(id, self.root)
    }

    pub fn ancestors(&self, id: InstanceId) -> Vec<InstanceId> {
        let mut out = Vec::new();
        let mut cur = self.parent_of(id);
        while let Some(c) = cur {
            out.push(c);
            cur = self.parent_of(c);
        }
        out
    }

    pub fn descendants(&self, id: InstanceId) -> Vec<InstanceId> {
        let mut out = Vec::new();
        let mut stack: Vec<InstanceId> = self.children_of(id).iter().rev().copied().collect();
        while let Some(c) = stack.pop() {
            out.push(c);
            for &g in self.children_of(c).iter().rev() {
                stack.push(g);
            }
        }
        out
    }

    pub fn find_first_ancestor_of_class(&self, id: InstanceId, class: &str) -> Option<InstanceId> {
        self.ancestors(id)
            .into_iter()
            .find(|&a| self.is_a(a, class))
    }

    pub fn full_name(&self, id: InstanceId) -> String {
        let Some(inst) = self.instances.get(&id) else {
            return String::new();
        };
        match inst.parent {
            Some(p) if !self.is_service_provider(p) => {
                format!("{}.{}", self.full_name(p), inst.name)
            }
            _ => inst.name.clone(),
        }
    }

    fn is_service_provider(&self, id: InstanceId) -> bool {
        matches!(
            self.class_of(id),
            Some("DataModel" | "ServiceProvider" | "GlobalSettings" | "UserSettings" | "Settings")
        )
    }

    pub fn find_first_child(
        &self,
        id: InstanceId,
        name: &str,
        recursive: bool,
    ) -> Option<InstanceId> {
        let children = self.children_of(id);
        if let Some(c) = children.iter().copied().find(|&c| self.name_of(c) == name) {
            return Some(c);
        }
        if recursive {
            for &c in children {
                if let Some(found) = self.find_first_child(c, name, true) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub fn find_first_child_of_class(&self, id: InstanceId, class: &str) -> Option<InstanceId> {
        self.children_of(id)
            .iter()
            .copied()
            .find(|&c| self.class_of(c) == Some(class))
    }

    pub fn find_first_child_which_is_a(&self, id: InstanceId, class: &str) -> Option<InstanceId> {
        self.children_of(id)
            .iter()
            .copied()
            .find(|&c| self.is_a(c, class))
    }

    pub fn find_first_descendant_which_is_a(
        &self,
        id: InstanceId,
        class: &str,
    ) -> Option<InstanceId> {
        for &c in self.children_of(id) {
            if self.is_a(c, class) {
                return Some(c);
            }
            if let Some(d) = self.find_first_descendant_which_is_a(c, class) {
                return Some(d);
            }
        }
        None
    }

    fn alloc_id(&mut self) -> InstanceId {
        let id = InstanceId::from_raw(self.next_id).unwrap_or(self.root);
        self.next_id += 1;
        id
    }

    pub fn create(&mut self, class: &str) -> Result<InstanceId, DomError> {
        let desc = database()
            .class(class)
            .ok_or_else(|| DomError::UnknownClass(class.to_string()))?;
        if !desc.creatable {
            return Err(DomError::NotCreatable(class.to_string()));
        }
        self.create_internal(class)
    }

    pub fn create_internal(&mut self, class: &str) -> Result<InstanceId, DomError> {
        let class = class_static(class).ok_or_else(|| DomError::UnknownClass(class.to_string()))?;
        let layout = layout_for(class).ok_or_else(|| DomError::UnknownClass(class.to_string()))?;
        let values = layout
            .slots
            .iter()
            .map(|p| default_value(class, p))
            .collect();
        let id = self.alloc_id();
        let name = crate::defaults::default_name(class);
        self.instances.insert(
            id,
            Instance {
                id,
                class,
                layout,
                name,
                parent: None,
                children: Vec::new(),
                archivable: true,
                roblox_locked: false,
                parent_locked: false,
                external_refs: 0,
                values,
                setting_parent: false,
            },
        );
        for b in behaviors_for(class).iter().rev() {
            b.init(self, id);
        }
        self.events.push(DomEvent::Created { id });
        Ok(id)
    }

    pub fn retain(&mut self, id: InstanceId) {
        if let Some(i) = self.instances.get_mut(&id) {
            i.external_refs += 1;
        }
    }

    pub fn release(&mut self, id: InstanceId) {
        if let Some(i) = self.instances.get_mut(&id) {
            i.external_refs = i.external_refs.saturating_sub(1);
        }
    }

    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        loop {
            let dead: Vec<InstanceId> = self
                .instances
                .values()
                .filter(|i| i.parent.is_none() && i.id != self.root && i.external_refs == 0)
                .map(|i| i.id)
                .collect();
            if dead.is_empty() {
                return freed;
            }
            for id in dead {
                let children = self.children_of(id).to_vec();
                for c in children.into_iter().rev() {
                    self.detach_for_delete(id, c);
                }
                self.instances.remove(&id);
                freed += 1;
            }
        }
    }

    fn detach_for_delete(&mut self, parent: InstanceId, child: InstanceId) {
        self.signal_descendant_removing(child, Some(parent), None);
        if let Some(p) = self.instances.get_mut(&parent) {
            p.children.retain(|&c| c != child);
        }
        if let Some(c) = self.instances.get_mut(&child) {
            c.parent = None;
        }
        self.events.push(DomEvent::ChildRemoved { parent, child });
        self.ancestor_changed(child, child, Some(parent), None);
        self.events.push(DomEvent::PropertyChanged {
            id: child,
            property: "Parent",
        });
    }

    pub fn get_service(&mut self, class: &str) -> Result<InstanceId, DomError> {
        if let Some(id) = self.find_service(class) {
            return Ok(id);
        }
        let id = self.create_internal(class)?;
        self.set_parent(id, Some(self.root))?;
        if let Some(i) = self.instances.get_mut(&id) {
            i.parent_locked = true;
        }
        Ok(id)
    }

    pub fn find_service(&self, class: &str) -> Option<InstanceId> {
        if let Some(&id) = self.services.get(class)
            && self.parent_of(id) == Some(self.root)
        {
            return Some(id);
        }
        self.children_of(self.root)
            .iter()
            .copied()
            .find(|&c| self.class_of(c) == Some(class))
    }

    pub(crate) fn register_service(&mut self, id: InstanceId) {
        if let Some(class) = self.class_of(id)
            && is_service_class(class)
        {
            self.services.insert(class, id);
        }
    }

    pub fn set_parent(
        &mut self,
        id: InstanceId,
        new_parent: Option<InstanceId>,
    ) -> Result<(), DomError> {
        self.set_parent_impl(id, new_parent, false)
    }

    pub fn set_parent_ignore_lock(
        &mut self,
        id: InstanceId,
        new_parent: Option<InstanceId>,
    ) -> Result<(), DomError> {
        self.set_parent_impl(id, new_parent, true)
    }

    fn set_parent_impl(
        &mut self,
        id: InstanceId,
        new_parent: Option<InstanceId>,
        ignore_lock: bool,
    ) -> Result<(), DomError> {
        let Some(inst) = self.instances.get(&id) else {
            return Err(DomError::Gone);
        };
        let old_parent = inst.parent;
        if new_parent == old_parent {
            return Ok(());
        }
        if let Some(p) = new_parent
            && !self.instances.contains_key(&p)
        {
            return Err(DomError::Gone);
        }
        if !ignore_lock && inst.parent_locked {
            return Err(DomError::Message(format!(
                "The Parent property of {} is locked, current parent: {}, new parent {}",
                self.full_name(id),
                old_parent
                    .map(|p| self.name_of(p).to_string())
                    .unwrap_or_else(|| "NULL".into()),
                new_parent
                    .map(|p| self.name_of(p).to_string())
                    .unwrap_or_else(|| "NULL".into())
            )));
        }
        if new_parent == Some(id) {
            return Err(DomError::Message(format!(
                "Attempt to set {} as its own parent",
                self.full_name(id)
            )));
        }
        if self.is_ancestor_of(id, new_parent) {
            return Err(DomError::Message(format!(
                "Attempt to set parent of {} to {} would result in circular reference",
                self.full_name(id),
                new_parent.map(|p| self.full_name(p)).unwrap_or_default()
            )));
        }
        if inst.setting_parent {
            tracing::warn!(
                "Something unexpectedly tried to set the parent of {} to {} while trying to set the parent of {}. Current parent is {}.",
                inst.name,
                new_parent
                    .map(|p| self.name_of(p).to_string())
                    .unwrap_or_else(|| "NULL".into()),
                inst.name,
                old_parent
                    .map(|p| self.name_of(p).to_string())
                    .unwrap_or_else(|| "NULL".into())
            );
            return Ok(());
        }
        let class = inst.class;
        for b in behaviors_for(class) {
            b.verify_set_parent(self, id, new_parent)
                .map_err(DomError::Message)?;
        }
        self.verify_set_ancestor(id, new_parent)?;
        if let Some(p) = new_parent {
            let pclass = self.class_of(p).unwrap_or("Instance");
            for b in behaviors_for(pclass) {
                b.verify_add_child(self, p, id).map_err(DomError::Message)?;
            }
            self.verify_add_descendant(p, id)?;
        }
        if let Some(i) = self.instances.get_mut(&id) {
            i.setting_parent = true;
        }
        if let Some(op) = old_parent {
            if !self.contains_or_is(op, new_parent) {
                self.signal_descendant_removing(id, Some(op), new_parent);
            }
            if let Some(o) = self.instances.get_mut(&op)
                && let Some(pos) = o.children.iter().position(|&c| c == id)
            {
                o.children.remove(pos);
            }
        }
        if let Some(np) = new_parent
            && let Some(n) = self.instances.get_mut(&np)
        {
            n.children.push(id);
        }
        if let Some(i) = self.instances.get_mut(&id) {
            i.parent = new_parent;
        }
        self.history.record(crate::history::Change::Parent {
            id,
            old: old_parent,
            new: new_parent,
        });
        if let Some(op) = old_parent {
            self.events.push(DomEvent::ChildRemoved {
                parent: op,
                child: id,
            });
            let oclass = self.class_of(op).unwrap_or("Instance");
            for b in behaviors_for(oclass) {
                b.child_removed(self, op, id);
            }
            self.tools_child_changed(op, id, false);
        }
        if let Some(np) = new_parent {
            let nclass = self.class_of(np).unwrap_or("Instance");
            for b in behaviors_for(nclass) {
                b.child_added(self, np, id);
            }
            self.tools_child_changed(np, id, true);
            if !self.contains_or_is(np, old_parent) {
                self.signal_descendant_added(id, Some(np), old_parent);
            }
            self.events.push(DomEvent::ChildAdded {
                parent: np,
                child: id,
            });
            if np == self.root {
                self.register_service(id);
            }
        }
        self.ancestor_changed(id, id, old_parent, new_parent);
        self.events.push(DomEvent::PropertyChanged {
            id,
            property: "Parent",
        });
        if let Some(i) = self.instances.get_mut(&id) {
            i.setting_parent = false;
        }
        Ok(())
    }

    fn contains_or_is(&self, ancestor: InstanceId, node: Option<InstanceId>) -> bool {
        let mut cur = node;
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent_of(c);
        }
        false
    }

    fn verify_set_ancestor(
        &self,
        id: InstanceId,
        new_parent: Option<InstanceId>,
    ) -> Result<(), DomError> {
        for d in self.descendants(id) {
            let class = self.class_of(d).unwrap_or("Instance");
            for b in behaviors_for(class) {
                b.verify_set_ancestor(self, d, new_parent, id)
                    .map_err(DomError::Message)?;
            }
        }
        Ok(())
    }

    fn verify_add_descendant(
        &self,
        new_parent: InstanceId,
        id: InstanceId,
    ) -> Result<(), DomError> {
        let old_parent = self.parent_of(id);
        let mut cur = self.parent_of(new_parent);
        while let Some(p) = cur {
            if Some(p) == old_parent || self.is_ancestor_of(p, old_parent) {
                break;
            }
            let class = self.class_of(p).unwrap_or("Instance");
            for b in behaviors_for(class) {
                b.verify_add_child(self, p, id).map_err(DomError::Message)?;
            }
            cur = self.parent_of(p);
        }
        Ok(())
    }

    fn signal_descendant_added(
        &mut self,
        id: InstanceId,
        begin: Option<InstanceId>,
        old_parent: Option<InstanceId>,
    ) {
        let mut cur = begin;
        while let Some(p) = cur {
            if Some(p) == old_parent || self.is_ancestor_of(p, old_parent) {
                break;
            }
            self.events.push(DomEvent::DescendantAdded {
                ancestor: p,
                descendant: id,
            });
            let class = self.class_of(p).unwrap_or("Instance");
            for b in behaviors_for(class) {
                b.descendant_added(self, p, id);
            }
            cur = self.parent_of(p);
        }
        for c in self.children_of(id).to_vec() {
            self.signal_descendant_added(c, begin, old_parent);
        }
    }

    fn signal_descendant_removing(
        &mut self,
        id: InstanceId,
        begin: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        let mut cur = begin;
        while let Some(p) = cur {
            if Some(p) == new_parent || self.is_ancestor_of(p, new_parent) {
                break;
            }
            self.events.push(DomEvent::DescendantRemoving {
                ancestor: p,
                descendant: id,
            });
            let class = self.class_of(p).unwrap_or("Instance");
            for b in behaviors_for(class) {
                b.descendant_removing(self, p, id);
            }
            cur = self.parent_of(p);
        }
        for c in self.children_of(id).to_vec() {
            self.signal_descendant_removing(c, begin, new_parent);
        }
    }

    fn ancestor_changed(
        &mut self,
        id: InstanceId,
        child: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        for c in self.children_of(id).to_vec() {
            self.ancestor_changed(c, child, old_parent, new_parent);
        }
        let class = self.class_of(id).unwrap_or("Instance");
        for b in behaviors_for(class) {
            b.ancestry_changed(self, id, child, old_parent, new_parent);
        }
        self.events.push(DomEvent::AncestryChanged {
            id,
            child,
            parent: new_parent,
        });
    }

    pub fn lock_parent(&mut self, id: InstanceId, locked: bool) {
        if let Some(i) = self.instances.get_mut(&id) {
            i.parent_locked = locked;
        }
    }

    pub fn remove(&mut self, id: InstanceId) -> Result<(), DomError> {
        let children = self.children_of(id).to_vec();
        self.set_parent(id, None)?;
        for c in children {
            let _ = self.remove(c);
        }
        Ok(())
    }

    pub fn destroy(&mut self, id: InstanceId) -> Result<(), DomError> {
        let Some(inst) = self.instances.get(&id) else {
            return Ok(());
        };
        if inst.parent_locked && inst.parent.is_some() {
            return Err(DomError::Message(format!(
                "The Parent property of {} is locked",
                self.full_name(id)
            )));
        }
        let class = inst.class;
        for b in behaviors_for(class) {
            b.destroying(self, id);
        }
        let children = self.children_of(id).to_vec();
        let was_locked = self
            .instances
            .get(&id)
            .map(|i| i.parent_locked)
            .unwrap_or(false);
        if let Some(i) = self.instances.get_mut(&id) {
            i.parent_locked = true;
        }
        if let Err(e) = self.set_parent_impl(id, None, true) {
            if !was_locked {
                self.lock_parent(id, false);
            }
            return Err(e);
        }
        for c in children.into_iter() {
            let _ = self.destroy(c);
        }
        self.events.push(DomEvent::Destroyed { id });
        Ok(())
    }

    pub fn destroy_all_children(&mut self, id: InstanceId) -> Result<(), DomError> {
        while let Some(&c) = self.children_of(id).last() {
            self.destroy(c)?;
            if self.children_of(id).last() == Some(&c) {
                return Err(DomError::Message(format!(
                    "The Parent property of {} is locked",
                    self.full_name(c)
                )));
            }
        }
        Ok(())
    }

    pub fn set_name(&mut self, id: InstanceId, name: &str) {
        let truncated: String = if name.len() > 100 {
            let mut end = 100;
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name[..end].to_string()
        } else {
            name.to_string()
        };
        let Some(inst) = self.instances.get_mut(&id) else {
            return;
        };
        if inst.name != truncated {
            let old = std::mem::replace(&mut inst.name, truncated.clone());
            self.history.record(crate::history::Change::Name {
                id,
                old,
                new: truncated,
            });
            self.events.push(DomEvent::PropertyChanged {
                id,
                property: "Name",
            });
            let class = self.class_of(id).unwrap_or("Instance");
            for b in behaviors_for(class) {
                b.changed(self, id, "Name");
            }
        }
    }

    pub fn property_descriptor(
        &self,
        id: InstanceId,
        name: &str,
    ) -> Option<&'static rbx_reflection::PropertyDescriptor> {
        let inst = self.instances.get(&id)?;
        inst.layout.descriptor(name)
    }

    pub fn get(&self, id: InstanceId, name: &str) -> Result<Variant, DomError> {
        let inst = self.instances.get(&id).ok_or(DomError::Gone)?;
        match name {
            "Name" => return Ok(Variant::String(inst.name.clone())),
            "Parent" => return Ok(Variant::Ref(inst.parent)),
            "Archivable" | "archivable" => return Ok(Variant::Bool(inst.archivable)),
            "ClassName" | "className" => return Ok(Variant::String(inst.class.to_string())),
            "RobloxLocked" => return Ok(Variant::Bool(inst.roblox_locked)),
            "DataCost" => return Ok(Variant::Int(self.data_cost(id))),
            _ => {}
        }
        let layout = inst.layout;
        let canonical = layout.canonical(name);
        if layout.computed.contains(canonical) || layout.computed.contains(name) {
            for b in behaviors_for(inst.class) {
                if let Some(v) = b.get(self, id, name) {
                    return Ok(v);
                }
                if canonical != name
                    && let Some(v) = b.get(self, id, canonical)
                {
                    return Ok(v);
                }
            }
            if let Some(d) = layout.descriptor(name)
                && let Some(t) = d.ty.variant_type()
            {
                return Ok(Variant::default_for(t));
            }
            return Err(DomError::UnknownProperty(
                name.to_string(),
                inst.class.to_string(),
            ));
        }
        for b in behaviors_for(inst.class) {
            if let Some(v) = b.get(self, id, name) {
                return Ok(v);
            }
        }
        match layout.slot(name) {
            Some(slot) => Ok(match &inst.values[slot] {
                Variant::Ref(Some(r)) if !self.instances.contains_key(r) => Variant::Ref(None),
                v => v.clone(),
            }),
            None => Err(DomError::UnknownProperty(
                name.to_string(),
                inst.class.to_string(),
            )),
        }
    }

    pub fn get_ref(&self, id: InstanceId, name: &str) -> Option<InstanceId> {
        self.get(id, name).ok().and_then(|v| v.as_ref()).flatten()
    }

    pub fn get_f32(&self, id: InstanceId, name: &str) -> f32 {
        self.get(id, name)
            .ok()
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0)
    }

    pub fn get_bool(&self, id: InstanceId, name: &str) -> bool {
        self.get(id, name)
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn get_raw(&self, id: InstanceId, name: &str) -> Option<&Variant> {
        let inst = self.instances.get(&id)?;
        inst.layout.slot(name).map(|s| &inst.values[s])
    }

    fn data_cost(&self, id: InstanceId) -> i32 {
        let mut total = 4;
        for c in self.children_of(id) {
            total += self.data_cost(*c);
        }
        total
    }

    pub fn coerce_for(
        &self,
        id: InstanceId,
        name: &str,
        value: Variant,
    ) -> Result<Variant, DomError> {
        let desc = self.property_descriptor(id, name).ok_or_else(|| {
            DomError::UnknownProperty(
                name.to_string(),
                self.class_of(id).unwrap_or("").to_string(),
            )
        })?;
        coerce_to(&desc.ty, value)
    }

    pub fn set(&mut self, id: InstanceId, name: &str, value: Variant) -> Result<(), DomError> {
        let inst = self.instances.get(&id).ok_or(DomError::Gone)?;
        match name {
            "Name" => {
                let s = match value {
                    Variant::String(s) | Variant::ProtectedString(s) | Variant::Content(s) => s,
                    other => {
                        return Err(DomError::BadValue(format!("string, got {:?}", other.ty())));
                    }
                };
                self.set_name(id, &s);
                return Ok(());
            }
            "Parent" => {
                let p = value
                    .as_ref()
                    .ok_or_else(|| DomError::BadValue("Instance".into()))?;
                return self.set_parent(id, p);
            }
            "Archivable" | "archivable" => {
                let b = value
                    .as_bool()
                    .ok_or_else(|| DomError::BadValue("bool".into()))?;
                if let Some(i) = self.instances.get_mut(&id)
                    && i.archivable != b
                {
                    i.archivable = b;
                    self.events.push(DomEvent::PropertyChanged {
                        id,
                        property: "Archivable",
                    });
                }
                return Ok(());
            }
            "RobloxLocked" => {
                let b = value
                    .as_bool()
                    .ok_or_else(|| DomError::BadValue("bool".into()))?;
                if let Some(i) = self.instances.get_mut(&id)
                    && i.roblox_locked != b
                {
                    i.roblox_locked = b;
                    self.events.push(DomEvent::PropertyChanged {
                        id,
                        property: "RobloxLocked",
                    });
                }
                return Ok(());
            }
            "ClassName" | "className" | "DataCost" => {
                return Err(DomError::ReadOnly(name.to_string()));
            }
            _ => {}
        }
        let class = inst.class;
        let layout = inst.layout;
        let desc = layout
            .descriptor(name)
            .ok_or_else(|| DomError::UnknownProperty(name.to_string(), class.to_string()))?;
        let mut value = coerce_to(&desc.ty, value)?;
        for b in behaviors_for(class) {
            match b.set(self, id, name, &value) {
                SetOutcome::Unhandled => {}
                SetOutcome::Handled => return Ok(()),
                SetOutcome::Replace(v) => {
                    value = v;
                    break;
                }
                SetOutcome::Error(e) => return Err(DomError::Message(e)),
            }
        }
        let canonical = layout.canonical(name);
        if layout.computed.contains(canonical) {
            return Err(DomError::ReadOnly(name.to_string()));
        }
        let slot = layout
            .slot(canonical)
            .ok_or_else(|| DomError::UnknownProperty(name.to_string(), class.to_string()))?;
        self.store(id, slot, value);
        Ok(())
    }

    pub fn set_unchecked(&mut self, id: InstanceId, name: &str, value: Variant) {
        let Some(inst) = self.instances.get(&id) else {
            return;
        };
        let layout = inst.layout;
        if let Some(slot) = layout.slot(layout.canonical(name)) {
            self.store(id, slot, value);
        }
    }

    pub(crate) fn store(&mut self, id: InstanceId, slot: usize, value: Variant) {
        let Some(inst) = self.instances.get_mut(&id) else {
            return;
        };
        if inst.values[slot] == value {
            return;
        }
        let prop: &'static str = inst.layout.slots[slot].name.as_str();
        if self.history.recording() {
            let old = std::mem::replace(&mut inst.values[slot], value.clone());
            self.history.record(crate::history::Change::Property {
                id,
                name: prop,
                old,
                new: value,
            });
        } else {
            inst.values[slot] = value;
        }
        let Some(inst) = self.instances.get(&id) else {
            return;
        };
        let class = inst.class;
        self.events
            .push(DomEvent::PropertyChanged { id, property: prop });
        for b in behaviors_for(class) {
            b.changed(self, id, prop);
        }
        if matches!(prop, "C0" | "C1") && crate::database().is_a(class, "JointInstance") {
            self.with_world_hooks(|h, dm| h.joint_changed(dm, id));
        }
    }

    pub fn store_silent(&mut self, id: InstanceId, name: &str, value: Variant) {
        let Some(inst) = self.instances.get_mut(&id) else {
            return;
        };
        let layout = inst.layout;
        if let Some(slot) = layout.slot(layout.canonical(name)) {
            inst.values[slot] = value;
        }
    }

    pub fn notify_changed(&mut self, id: InstanceId, property: &'static str) {
        self.events.push(DomEvent::PropertyChanged { id, property });
    }

    pub fn clone_instance(&mut self, id: InstanceId) -> Result<Option<InstanceId>, DomError> {
        let mut map: Vec<(InstanceId, InstanceId)> = Vec::new();
        let root_clone = self.clone_create(id, &mut map)?;
        let Some(root_clone) = root_clone else {
            return Ok(None);
        };
        let lookup: HashMap<InstanceId, InstanceId> = map.iter().copied().collect();
        for &(src, dst) in &map {
            self.clone_copy_properties(src, dst, &lookup);
        }
        self.clone_set_parents(id, root_clone, &lookup)?;
        Ok(Some(root_clone))
    }

    fn clone_create(
        &mut self,
        id: InstanceId,
        map: &mut Vec<(InstanceId, InstanceId)>,
    ) -> Result<Option<InstanceId>, DomError> {
        let inst = self.instances.get(&id).ok_or(DomError::Gone)?;
        let class = inst.class;
        let serializable = database()
            .class(class)
            .map(|c| c.functionality.is_serializable())
            .unwrap_or(false);
        if !serializable || !inst.archivable {
            return Ok(None);
        }
        let copy = self
            .create_internal(class)
            .map_err(|_| DomError::Message(format!("{} cannot be cloned", self.full_name(id))))?;
        map.push((id, copy));
        for c in self.children_of(id).to_vec() {
            self.clone_create(c, map)?;
        }
        Ok(Some(copy))
    }

    fn clone_copy_properties(
        &mut self,
        src: InstanceId,
        dst: InstanceId,
        lookup: &HashMap<InstanceId, InstanceId>,
    ) {
        let Some(s) = self.instances.get(&src) else {
            return;
        };
        let name = s.name.clone();
        let layout = s.layout;
        let values: Vec<(usize, Variant)> = layout
            .slots
            .iter()
            .enumerate()
            .filter(|(_, p)| p.always_clone() || (p.can_xml_read() && p.can_xml_write()))
            .map(|(i, _)| {
                let v = match &s.values[i] {
                    Variant::Ref(Some(r)) => Variant::Ref(Some(*lookup.get(r).unwrap_or(r))),
                    v => v.clone(),
                };
                (i, v)
            })
            .collect();
        if let Some(d) = self.instances.get_mut(&dst) {
            d.name = name;
            for (i, v) in values {
                d.values[i] = v;
            }
        }
        let class = self.class_of(dst).unwrap_or("Instance");
        for b in behaviors_for(class) {
            b.changed(self, dst, "");
        }
    }

    fn clone_set_parents(
        &mut self,
        src: InstanceId,
        root_clone: InstanceId,
        lookup: &HashMap<InstanceId, InstanceId>,
    ) -> Result<(), DomError> {
        let Some(&dst) = lookup.get(&src) else {
            return Ok(());
        };
        for c in self.children_of(src).to_vec() {
            self.clone_set_parents(c, root_clone, lookup)?;
        }
        if dst == root_clone {
            return Ok(());
        }
        let parent = self.parent_of(src).and_then(|p| lookup.get(&p).copied());
        if let Some(p) = parent {
            self.set_parent(dst, Some(p))?;
        }
        Ok(())
    }

    pub fn can_add_child(&self, parent: InstanceId, child: InstanceId) -> bool {
        if self.contains_or_is(child, Some(parent)) {
            return false;
        }
        if self.parent_of(child) == Some(parent) {
            return false;
        }
        let pclass = self.class_of(parent).unwrap_or("Instance");
        let cclass = self.class_of(child).unwrap_or("Instance");
        if behaviors_for(pclass)
            .iter()
            .any(|b| b.ask_forbid_child(self, parent, child))
        {
            return false;
        }
        if behaviors_for(cclass)
            .iter()
            .any(|b| b.ask_forbid_parent(self, child, parent))
        {
            return false;
        }
        if behaviors_for(pclass)
            .iter()
            .any(|b| b.ask_add_child(self, parent, child))
        {
            return true;
        }
        behaviors_for(cclass)
            .iter()
            .any(|b| b.ask_set_parent(self, child, parent))
    }

    pub fn move_child_index(&mut self, parent: InstanceId, child: InstanceId, index: usize) {
        if let Some(p) = self.instances.get_mut(&parent)
            && let Some(pos) = p.children.iter().position(|&c| c == child)
        {
            let c = p.children.remove(pos);
            let idx = index.min(p.children.len());
            p.children.insert(idx, c);
        }
    }
}

pub fn coerce_to(ty: &ValueType, value: Variant) -> Result<Variant, DomError> {
    match ty {
        ValueType::Value(t) => value
            .coerce(*t)
            .ok_or_else(|| DomError::BadValue(format!("{t:?}"))),
        ValueType::Enum(name) => match value {
            Variant::Enum(v) => Ok(Variant::Enum(v)),
            Variant::Int(v) if v >= 0 => Ok(Variant::Enum(v as u32)),
            Variant::Int64(v) if v >= 0 => Ok(Variant::Enum(v as u32)),
            Variant::Double(v) if v >= 0.0 => Ok(Variant::Enum(v as u32)),
            Variant::Float(v) if v >= 0.0 => Ok(Variant::Enum(v as u32)),
            Variant::String(s) => database()
                .enum_desc(name)
                .and_then(|e| e.from_name(&s))
                .map(Variant::Enum)
                .ok_or_else(|| DomError::BadValue(name.clone())),
            _ => Err(DomError::BadValue(name.clone())),
        },
        ValueType::Ref(_) => match value {
            Variant::Ref(r) => Ok(Variant::Ref(r)),
            Variant::Nil => Ok(Variant::Ref(None)),
            _ => Err(DomError::BadValue("Instance".into())),
        },
        _ => Ok(value),
    }
}
