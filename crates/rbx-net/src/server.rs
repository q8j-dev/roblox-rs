use crate::core::{Core, NIL, View};
use crate::filter::{Filter, Verdict};
use crate::scope::CONTAINERS;
use crate::wire::{
    Message, NetId, NewInstance, PROTOCOL_VERSION, PeerId, PhysicsState, SERVER_PEER,
};
use rbx_dom::{
    DataModel, DomEvent, EventCursor, InstanceId, RemoteDelivery, RemoteKind, RemoteTarget,
    RemoteValue, Variant, database,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Peer {
    pub view: View,
    pub hello: bool,
    pub joined: bool,
    pub kicked: Option<String>,
    pub rejected: u64,
}

impl Peer {
    pub fn player(&self) -> Option<InstanceId> {
        self.view.viewer
    }
}

pub struct ServerReplicator {
    pub core: Core,
    cursor: EventCursor,
    peers: BTreeMap<PeerId, Peer>,
    next_peer: PeerId,
    outbox: Vec<(PeerId, Message)>,
    physics_in: Vec<(PeerId, Vec<PhysicsState>)>,
}

impl ServerReplicator {
    pub fn new(dm: &mut DataModel) -> ServerReplicator {
        let mut core = Core::default();
        core.ids.me = SERVER_PEER;
        ServerReplicator {
            core,
            cursor: dm.subscribe(),
            peers: BTreeMap::new(),
            next_peer: 1,
            outbox: Vec::new(),
            physics_in: Vec::new(),
        }
    }

    pub fn filtering(dm: &DataModel) -> bool {
        dm.find_service("Workspace")
            .is_some_and(|ws| dm.get_bool(ws, "FilteringEnabled"))
    }

    pub fn connect(&mut self) -> PeerId {
        let peer = self.next_peer;
        self.next_peer = self.next_peer.wrapping_add(1).max(1);
        self.peers.insert(peer, Peer::default());
        peer
    }

    pub fn disconnect(&mut self, dm: &mut DataModel, peer: PeerId) {
        self.flush(dm);
        if let Some(p) = self.peers.remove(&peer)
            && let Some(player) = p.player()
        {
            if let Some(ch) = dm.get_ref(player, "Character") {
                let _ = dm.destroy(ch);
            }
            dm.lock_parent(player, false);
            let _ = dm.destroy(player);
        }
        self.flush(dm);
    }

    pub fn peers(&self) -> impl Iterator<Item = (PeerId, &Peer)> {
        self.peers.iter().map(|(k, v)| (*k, v))
    }

    pub fn peer(&self, peer: PeerId) -> Option<&Peer> {
        self.peers.get(&peer)
    }

    pub fn peer_of_player(&self, player: InstanceId) -> Option<PeerId> {
        self.peers
            .iter()
            .find(|(_, p)| p.player() == Some(player))
            .map(|(k, _)| *k)
    }

    pub fn take_kicked(&mut self) -> Vec<(PeerId, String)> {
        let mut out = Vec::new();
        for (id, p) in self.peers.iter_mut() {
            if let Some(reason) = p.kicked.take() {
                out.push((*id, reason));
            }
        }
        out
    }

    pub fn send(&mut self, peer: PeerId, msg: Message) {
        self.outbox.push((peer, msg));
    }

    pub fn knows(&self, peer: PeerId, id: InstanceId) -> bool {
        self.peers.get(&peer).is_some_and(|p| p.view.knows(id))
    }

    pub fn take_physics(&mut self) -> Vec<(PeerId, Vec<PhysicsState>)> {
        std::mem::take(&mut self.physics_in)
    }

    pub fn take_outgoing(&mut self) -> Vec<(PeerId, Message)> {
        std::mem::take(&mut self.outbox)
    }

    fn send_join(&mut self, dm: &DataModel, peer: PeerId) {
        let filtering = Self::filtering(dm);
        let Some(p) = self.peers.get_mut(&peer) else {
            return;
        };
        let mut out = vec![Message::Welcome { peer, filtering }];
        p.view.begin();
        for c in CONTAINERS {
            if let Some(id) = dm.find_service(c.class)
                && dm.parent_of(id) == Some(dm.root())
            {
                self.core.sync(dm, &mut p.view, id, &mut out);
            }
        }
        out.push(Message::JoinComplete);
        p.joined = true;
        self.outbox.extend(out.into_iter().map(|m| (peer, m)));
    }

    pub fn flush(&mut self, dm: &mut DataModel) {
        self.flush_except(dm, None);
    }

    fn flush_except(&mut self, dm: &mut DataModel, except: Option<PeerId>) {
        let events = dm.drain_events(self.cursor);
        let remote = dm.drain_remote();
        let dm: &DataModel = dm;
        for (&peer, p) in self.peers.iter_mut() {
            if !p.joined || Some(peer) == except {
                continue;
            }
            p.view.begin();
            let mut out = Vec::new();
            for ev in &events {
                match *ev {
                    DomEvent::ChildAdded { child, .. } | DomEvent::ChildRemoved { child, .. } => {
                        self.core.sync(dm, &mut p.view, child, &mut out)
                    }
                    DomEvent::PropertyChanged { id, property } => {
                        self.core
                            .property_changed(dm, &mut p.view, id, property, &mut out)
                    }
                    DomEvent::Destroyed { id } if p.view.knows(id) => {
                        p.view.forget_tree(dm, id);
                        out.push(Message::Delete {
                            id: self.core.ids.net(id),
                        });
                    }
                    _ => {}
                }
            }
            self.outbox.extend(out.into_iter().map(|m| (peer, m)));
        }
        for call in remote {
            let Some(player_peer) = (match call.target {
                RemoteTarget::Server => continue,
                RemoteTarget::Player(pl) => self.peer_of_player(pl).map(Some),
                RemoteTarget::AllPlayers => Some(None),
            }) else {
                continue;
            };
            let ids = &self.core.ids;
            let args: Vec<RemoteValue> = call
                .args
                .into_iter()
                .map(|a| a.map_refs(&mut |r| r.and_then(|i| InstanceId::from_raw(ids.net(i).0))))
                .collect();
            for (&peer, p) in &self.peers {
                if !p.joined || player_peer.is_some_and(|only| only != peer) {
                    continue;
                }
                if !p.view.knows(call.instance) {
                    continue;
                }
                self.outbox.push((
                    peer,
                    Message::Remote {
                        id: self.core.ids.net(call.instance),
                        kind: call.kind,
                        args: args.clone(),
                    },
                ));
            }
        }
        self.core.collect(dm);
    }

    fn resolve_known(&self, dm: &DataModel, peer: PeerId, net: NetId) -> Option<InstanceId> {
        let id = self.core.ids.local(dm, net)?;
        self.peers.get(&peer)?.view.knows(id).then_some(id)
    }

    fn verdict(&mut self, peer: PeerId, v: Verdict, what: &str) -> bool {
        let Some(p) = self.peers.get_mut(&peer) else {
            return false;
        };
        match v {
            Verdict::Accept => true,
            Verdict::Reject => {
                p.rejected += 1;
                tracing::debug!("filtered {what} from peer {peer}");
                false
            }
            Verdict::Disconnect => {
                p.kicked = Some(format!("illegal {what}"));
                false
            }
        }
    }

    pub fn receive(&mut self, dm: &mut DataModel, peer: PeerId, msg: Message) {
        let Some(p) = self.peers.get(&peer) else {
            return;
        };
        if p.kicked.is_some() {
            return;
        }
        if !p.hello {
            match msg {
                Message::Hello { protocol } if protocol == PROTOCOL_VERSION => {
                    self.flush(dm);
                    if let Some(p) = self.peers.get_mut(&peer) {
                        p.hello = true;
                    }
                    self.send_join(dm, peer);
                }
                Message::Hello { protocol } => {
                    self.outbox.push((
                        peer,
                        Message::Reject {
                            reason: format!(
                                "protocol {protocol} is not supported, server speaks {PROTOCOL_VERSION}"
                            ),
                        },
                    ));
                    if let Some(p) = self.peers.get_mut(&peer) {
                        p.kicked = Some("protocol mismatch".into());
                    }
                }
                _ => {
                    if let Some(p) = self.peers.get_mut(&peer) {
                        p.kicked = Some("expected hello".into());
                    }
                }
            }
            return;
        }
        match msg {
            Message::Ping { nonce } => {
                self.outbox.push((peer, Message::Pong { nonce }));
                return;
            }
            Message::Physics(states) => {
                self.physics_in.push((peer, states));
                return;
            }
            _ => {}
        }
        self.flush(dm);
        let strict = Self::filtering(dm);
        match msg {
            Message::New(items) => self.receive_new(dm, peer, strict, items),
            Message::Property { id, name, value } => {
                let Some(target) = self.resolve_known(dm, peer, id) else {
                    return;
                };
                let Some(desc) = dm.instance(target).and_then(|i| {
                    if name == "Name" {
                        database().find_property(i.class, "Name")
                    } else {
                        i.layout.descriptor(&name)
                    }
                }) else {
                    return;
                };
                let player = self.peers.get(&peer).and_then(Peer::player);
                let v = Filter { dm, player, strict }.property(target, desc);
                if self.verdict(peer, v, "property") {
                    self.core.apply_property(dm, target, &name, value);
                }
            }
            Message::Parent { id, parent } => {
                let Some(target) = self.resolve_known(dm, peer, id) else {
                    return;
                };
                let new_parent = if parent == NIL {
                    None
                } else {
                    match self.resolve_known(dm, peer, parent) {
                        Some(p) => Some(p),
                        None => return,
                    }
                };
                let player = self.peers.get(&peer).and_then(Peer::player);
                let v = Filter { dm, player, strict }.parent(target, new_parent);
                if self.verdict(peer, v, "parent")
                    && self.core.set_parent(dm, target, new_parent)
                    && let Some(p) = self.peers.get_mut(&peer)
                {
                    p.view.known.insert(target, new_parent);
                }
            }
            Message::Delete { id } => {
                let Some(target) = self.resolve_known(dm, peer, id) else {
                    return;
                };
                let player = self.peers.get(&peer).and_then(Peer::player);
                let v = Filter { dm, player, strict }.delete(target);
                if self.verdict(peer, v, "delete") {
                    if let Some(p) = self.peers.get_mut(&peer) {
                        p.view.forget_tree(dm, target);
                    }
                    self.core.delete(dm, target);
                }
            }
            Message::Event { id, name, args } => {
                let Some(target) = self.resolve_known(dm, peer, id) else {
                    return;
                };
                let Some(class) = dm.class_of(target) else {
                    return;
                };
                let Some(desc) = database().find_event(class, &name) else {
                    return;
                };
                let player = self.peers.get(&peer).and_then(Peer::player);
                let v = Filter { dm, player, strict }.event(target, desc);
                if !self.verdict(peer, v, "event") {
                    return;
                }
                let decoded = args
                    .into_iter()
                    .map(|a| self.decode_arg(dm, peer, a))
                    .collect();
                dm.fire_signal(target, desc.name.as_str(), decoded);
            }
            Message::Remote { id, kind, args } => {
                let Some(target) = self.resolve_known(dm, peer, id) else {
                    return;
                };
                let class = dm.class_of(target).unwrap_or("Instance");
                let expected = match kind {
                    RemoteKind::Event => "RemoteEvent",
                    _ => "RemoteFunction",
                };
                if !database().is_a(class, expected) {
                    return;
                }
                let player = self.peers.get(&peer).and_then(Peer::player);
                if kind == RemoteKind::Event
                    && let Some(desc) = database().find_event(class, "OnServerEvent")
                {
                    let v = Filter { dm, player, strict }.event(target, desc);
                    if !self.verdict(peer, v, "event") {
                        return;
                    }
                }
                let args = args
                    .into_iter()
                    .map(|a| {
                        a.map_refs(&mut |r| {
                            r.and_then(|raw| self.resolve_known(dm, peer, NetId(raw.raw())))
                        })
                    })
                    .collect();
                dm.deliver_remote(RemoteDelivery {
                    instance: target,
                    player,
                    kind,
                    args,
                });
            }
            _ => {}
        }
        self.flush_except(dm, Some(peer));
    }

    fn decode_arg(&self, dm: &DataModel, peer: PeerId, v: Variant) -> Variant {
        match v {
            Variant::Ref(Some(raw)) => Variant::Ref(self.resolve_known(dm, peer, NetId(raw.raw()))),
            other => other,
        }
    }

    fn receive_new(
        &mut self,
        dm: &mut DataModel,
        peer: PeerId,
        strict: bool,
        items: Vec<NewInstance>,
    ) {
        let mut created: Vec<Created> = Vec::new();
        for item in items {
            if item.id.peer() != peer || self.core.ids.local(dm, item.id).is_some() {
                continue;
            }
            let parent = created
                .iter()
                .find(|(id, _, _)| self.core.ids.net(*id) == item.parent)
                .map(|(id, _, _)| *id)
                .or_else(|| self.resolve_known(dm, peer, item.parent));
            let Some(parent) = parent else {
                continue;
            };
            let Some(class) = database().class(&item.class) else {
                continue;
            };
            let player = self.peers.get(&peer).and_then(Peer::player);
            let v = Filter { dm, player, strict }.new_instance(&class.name, parent);
            if !self.verdict(peer, v, "instance") {
                continue;
            }
            if let Some(id) = self.core.create(dm, &item, parent) {
                if class.name == "Player"
                    && let Some(p) = self.peers.get_mut(&peer)
                {
                    p.view.viewer = Some(id);
                }
                created.push((id, parent, item.properties));
            }
        }
        for (id, _, props) in &mut created {
            let props = std::mem::take(props);
            self.core.apply_properties(dm, *id, props);
        }
        for (id, parent, _) in &created {
            self.core.set_parent(dm, *id, Some(*parent));
            if let Some(p) = self.peers.get_mut(&peer) {
                p.view.known.insert(*id, Some(*parent));
            }
        }
        if let Some(p) = self.peers.get_mut(&peer)
            && let Some(player) = p.player()
            && created.iter().any(|(id, _, _)| *id == player)
        {
            let player_children: Vec<InstanceId> = dm.descendants(player);
            let mut out = Vec::new();
            p.view.begin();
            for c in player_children {
                self.core.sync(dm, &mut p.view, c, &mut out);
            }
            self.outbox.extend(out.into_iter().map(|m| (peer, m)));
        }
    }
}

type Created = (InstanceId, InstanceId, Vec<(String, Variant)>);
