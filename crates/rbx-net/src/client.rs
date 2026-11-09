use crate::core::{Core, NIL, View};
use crate::wire::{Message, NetId, NewInstance, PROTOCOL_VERSION, PeerId, PhysicsState};
use rbx_dom::{
    DataModel, DomEvent, EventCursor, InstanceId, RemoteDelivery, RemoteKind, RemoteTarget, Variant,
};
use rustc_hash::FxHashSet as HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientState {
    Connecting,
    Joining,
    Joined,
    Rejected(String),
}

pub struct ClientReplicator {
    pub core: Core,
    cursor: EventCursor,
    view: View,
    pub state: ClientState,
    pub peer: Option<PeerId>,
    pub filtering: bool,
    outbox: Vec<Message>,
    physics_in: Vec<PhysicsState>,
    owned: HashSet<InstanceId>,
}

impl ClientReplicator {
    pub fn new(dm: &mut DataModel) -> ClientReplicator {
        ClientReplicator {
            core: Core::default(),
            cursor: dm.subscribe(),
            view: View::default(),
            state: ClientState::Connecting,
            peer: None,
            filtering: false,
            outbox: vec![Message::Hello {
                protocol: PROTOCOL_VERSION,
            }],
            physics_in: Vec::new(),
            owned: HashSet::default(),
        }
    }

    pub fn send(&mut self, msg: Message) {
        self.outbox.push(msg);
    }

    pub fn take_physics(&mut self) -> Vec<PhysicsState> {
        std::mem::take(&mut self.physics_in)
    }

    pub fn owned(&self) -> &HashSet<InstanceId> {
        &self.owned
    }

    pub fn take_outgoing(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    pub fn joined(&self) -> bool {
        self.state == ClientState::Joined
    }

    pub fn net_id(&self, id: InstanceId) -> NetId {
        self.core.ids.net(id)
    }

    pub fn flush(&mut self, dm: &mut DataModel) {
        let events = dm.drain_events(self.cursor);
        let remote = dm.drain_remote();
        if !self.joined() {
            return;
        }
        self.view.viewer = dm.local_player;
        self.view.begin();
        let dm: &DataModel = dm;
        let mut out = Vec::new();
        for ev in &events {
            match *ev {
                DomEvent::ChildAdded { child, .. } | DomEvent::ChildRemoved { child, .. } => {
                    self.core.sync(dm, &mut self.view, child, &mut out)
                }
                DomEvent::PropertyChanged { id, property } => {
                    self.core
                        .property_changed(dm, &mut self.view, id, property, &mut out)
                }
                DomEvent::Destroyed { id } if self.view.knows(id) => {
                    self.view.forget_tree(dm, id);
                    out.push(Message::Delete {
                        id: self.core.ids.net(id),
                    });
                }
                _ => {}
            }
        }
        for call in remote {
            if call.target != RemoteTarget::Server || !self.view.knows(call.instance) {
                continue;
            }
            let ids = &self.core.ids;
            out.push(Message::Remote {
                id: ids.net(call.instance),
                kind: call.kind,
                args: call
                    .args
                    .into_iter()
                    .map(|a| {
                        a.map_refs(&mut |r| r.and_then(|i| InstanceId::from_raw(ids.net(i).0)))
                    })
                    .collect(),
            });
        }
        self.outbox.extend(out);
    }

    fn discard_events(&mut self, dm: &mut DataModel) {
        dm.drain_events(self.cursor);
        self.core.collect(dm);
    }

    fn sync_local(&mut self, dm: &mut DataModel) {
        self.view.viewer = dm.local_player;
        self.view.begin();
        let dm: &DataModel = dm;
        let mut out = Vec::new();
        if let Some(player) = dm.local_player.filter(|p| dm.contains(*p)) {
            self.core.sync(dm, &mut self.view, player, &mut out);
        }
        self.outbox.extend(out);
    }

    pub fn receive(&mut self, dm: &mut DataModel, msg: Message) {
        if matches!(self.state, ClientState::Rejected(_)) {
            return;
        }
        match msg {
            Message::Physics(states) => {
                self.physics_in.extend(states);
                return;
            }
            Message::Ownership { id, owner } => {
                if let Some(local) = self.core.ids.local(dm, id) {
                    if Some(owner) == self.peer {
                        self.owned.insert(local);
                    } else {
                        self.owned.remove(&local);
                    }
                }
                return;
            }
            _ => {}
        }
        self.flush(dm);
        match msg {
            Message::Welcome { peer, filtering } => {
                self.peer = Some(peer);
                self.core.ids.me = peer;
                self.filtering = filtering;
                self.state = ClientState::Joining;
            }
            Message::Reject { reason } => {
                self.state = ClientState::Rejected(reason);
            }
            Message::Service {
                id,
                class,
                properties,
            } => {
                if let Some(local) = self.core.bind_service(dm, id, &class) {
                    self.core.apply_properties(dm, local, properties);
                    self.view.known.insert(local, Some(dm.root()));
                }
            }
            Message::New(items) => self.receive_new(dm, items),
            Message::Property { id, name, value } => {
                if let Some(target) = self.core.ids.local(dm, id) {
                    self.core.apply_property(dm, target, &name, value);
                }
            }
            Message::Parent { id, parent } => {
                let Some(target) = self.core.ids.local(dm, id) else {
                    return;
                };
                let new_parent = if parent == NIL {
                    None
                } else {
                    match self.core.ids.local(dm, parent) {
                        Some(p) => Some(p),
                        None => return,
                    }
                };
                if self.core.set_parent(dm, target, new_parent) {
                    self.view.known.insert(target, new_parent);
                }
            }
            Message::Delete { id } => {
                if let Some(target) = self.core.ids.local(dm, id) {
                    self.view.forget_tree(dm, target);
                    self.core.delete(dm, target);
                }
            }
            Message::Event { id, name, args } => {
                let Some(target) = self.core.ids.local(dm, id) else {
                    return;
                };
                let Some(event) = Core::event_name(dm, target, &name) else {
                    return;
                };
                let args = args
                    .into_iter()
                    .map(|a| match a {
                        Variant::Ref(Some(raw)) => {
                            Variant::Ref(self.core.ids.local(dm, NetId(raw.raw())))
                        }
                        other => other,
                    })
                    .collect();
                dm.fire_signal(target, event, args);
            }
            Message::Remote { id, kind, args } => {
                let Some(target) = self.core.ids.local(dm, id) else {
                    return;
                };
                let expected = match kind {
                    RemoteKind::Event => "RemoteEvent",
                    _ => "RemoteFunction",
                };
                if !dm.is_a(target, expected) {
                    return;
                }
                let ids = &self.core.ids;
                let dmr: &DataModel = dm;
                let args = args
                    .into_iter()
                    .map(|a| {
                        a.map_refs(&mut |r| r.and_then(|raw| ids.local(dmr, NetId(raw.raw()))))
                    })
                    .collect();
                dm.deliver_remote(RemoteDelivery {
                    instance: target,
                    player: None,
                    kind,
                    args,
                });
            }
            Message::Ping { nonce } => self.outbox.push(Message::Pong { nonce }),
            Message::JoinComplete => {
                self.discard_events(dm);
                self.state = ClientState::Joined;
                self.sync_local(dm);
                return;
            }
            _ => {}
        }
        self.discard_events(dm);
    }

    fn receive_new(&mut self, dm: &mut DataModel, items: Vec<NewInstance>) {
        let mut created: Vec<Created> = Vec::new();
        for item in items {
            let parent = created
                .iter()
                .find(|(id, _, _)| self.core.ids.net(*id) == item.parent)
                .map(|(id, _, _)| *id)
                .or_else(|| self.core.ids.local(dm, item.parent));
            let Some(parent) = parent else {
                continue;
            };
            if let Some(id) = self.core.create(dm, &item, parent) {
                created.push((id, parent, item.properties));
            }
        }
        for (id, _, props) in &mut created {
            let props = std::mem::take(props);
            self.core.apply_properties(dm, *id, props);
        }
        for (id, parent, _) in &created {
            self.core.set_parent(dm, *id, Some(*parent));
            self.view.known.insert(*id, Some(*parent));
        }
    }
}

type Created = (InstanceId, InstanceId, Vec<(String, Variant)>);
