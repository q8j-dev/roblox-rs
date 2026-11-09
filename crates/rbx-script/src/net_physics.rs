use crate::runtime::Engine;
use rbx_dom::{DataModel, InstanceId};
use rbx_math::{Vec2, Vec3};
use rbx_net::wire::SERVER_PEER;
use rbx_net::{ClientReplicator, Message, PeerId, PhysicsState, ServerReplicator};
use rbx_physics::world::{NetworkRoot, OwnershipGroup};
use rustc_hash::FxHashMap as HashMap;

pub const SEND_INTERVAL: u32 = 2;
const CHUNK: usize = 64;
const SERVER_SLOP: f32 = 1.0;
const DEFAULT_RADIUS: f32 = 80.0;

struct ClientPoint {
    player: InstanceId,
    point: Vec2,
    radius: f32,
    character: Option<InstanceId>,
}

impl ClientPoint {
    fn relative_error(&self, p: Vec2) -> f32 {
        (p - self.point).length_squared() / (self.radius * self.radius).max(1e-6)
    }

    fn in_range(&self, p: Vec2) -> bool {
        let r = self.radius * SERVER_SLOP;
        (p - self.point).length_squared() < r * r
    }

    fn owns_group(&self, dm: &DataModel, group: &OwnershipGroup) -> bool {
        self.character
            .is_some_and(|ch| group.parts.iter().any(|&p| dm.is_descendant_of(p, ch)))
    }
}

fn closer_to_other(p: Vec2, owner: &ClientPoint, other: &ClientPoint) -> bool {
    if owner.point == other.point {
        return false;
    }
    let owner_d = (p - owner.point).length_squared();
    let other_d = (p - other.point).length_squared();
    if other_d < owner_d {
        let delta = owner.radius * (SERVER_SLOP - 1.0);
        return owner_d - other_d > delta * delta;
    }
    false
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

fn client_points(dm: &DataModel, rep: &ServerReplicator) -> Vec<ClientPoint> {
    let mut out = Vec::new();
    for (_, p) in rep.peers() {
        let Some(player) = p.player() else {
            continue;
        };
        let character = dm.get_ref(player, "Character");
        let head = character.and_then(|ch| {
            dm.find_first_child(ch, "Head", false)
                .or_else(|| dm.find_first_child(ch, "HumanoidRootPart", false))
                .or_else(|| dm.find_first_child(ch, "Torso", false))
        });
        let Some(head) = head.filter(|h| dm.is_a(*h, "BasePart")) else {
            continue;
        };
        let pos = dm
            .get_raw(head, "CFrame")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
            .unwrap_or_default();
        let radius = match dm.get_f32(player, "SimulationRadius") {
            r if r > 0.0 => r,
            _ => DEFAULT_RADIUS,
        };
        out.push(ClientPoint {
            player,
            point: xz(pos),
            radius,
            character,
        });
    }
    out
}

#[derive(Default)]
pub struct ServerPhysics {
    frame: u32,
    sent_owner: HashMap<InstanceId, PeerId>,
    awake: HashMap<InstanceId, bool>,
}

fn state(rep_ids: &rbx_net::IdMap, r: &NetworkRoot) -> PhysicsState {
    PhysicsState {
        id: rep_ids.net(r.id),
        cframe: r.cframe,
        velocity: r.velocity,
        rot_velocity: r.rot_velocity,
    }
}

fn chunks(states: Vec<PhysicsState>) -> impl Iterator<Item = Message> {
    let mut states = states;
    let mut out = Vec::new();
    while !states.is_empty() {
        let rest = states.split_off(states.len().min(CHUNK));
        out.push(Message::Physics(states));
        states = rest;
    }
    out.into_iter()
}

impl ServerPhysics {
    pub fn apply_incoming(&mut self, e: &mut Engine, rep: &mut ServerReplicator) {
        let incoming = rep.take_physics();
        if incoming.is_empty() {
            return;
        }
        let world = e.world.clone();
        let mut w = world.lock();
        for (peer, states) in incoming {
            let Some(player) = rep.peer(peer).and_then(|p| p.player()) else {
                continue;
            };
            for st in states {
                let Some(local) = rep.core.ids.local(&e.dm, st.id) else {
                    continue;
                };
                if !rep.knows(peer, local) {
                    continue;
                }
                if e.ownership.get(&local).and_then(|o| o.0) != Some(player) {
                    continue;
                }
                w.apply_network_state(&mut e.dm, local, st.cframe, st.velocity, st.rot_velocity);
            }
        }
    }

    pub fn tick(&mut self, e: &mut Engine, rep: &mut ServerReplicator) {
        self.frame = self.frame.wrapping_add(1);
        let (roots, groups) = {
            let world = e.world.clone();
            let mut w = world.lock();
            w.sync(&mut e.dm);
            w.rebuild_assemblies();
            (w.network_roots(), w.ownership_groups())
        };
        let clients = client_points(&e.dm, rep);
        self.update_owners(e, rep, &groups, &clients);
        if self.frame.is_multiple_of(SEND_INTERVAL) {
            self.send_states(e, rep, &roots);
        }
    }

    fn update_owners(
        &mut self,
        e: &mut Engine,
        rep: &mut ServerReplicator,
        groups: &[OwnershipGroup],
        clients: &[ClientPoint],
    ) {
        let by_player =
            |p: Option<InstanceId>| p.and_then(|p| clients.iter().find(|c| c.player == p));
        let moving: rustc_hash::FxHashSet<InstanceId> = groups
            .iter()
            .flat_map(|g| g.parts.iter().copied())
            .collect();
        e.ownership.retain(|part, _| moving.contains(part));
        for g in groups {
            let entry = e.reconcile_ownership(&g.parts);
            let owner = match entry {
                (owner, false) => {
                    let present = owner.is_none_or(|p| {
                        e.dm.parent_of(p)
                            .is_some_and(|pp| e.dm.class_of(pp) == Some("Players"))
                    });
                    if present {
                        owner
                    } else {
                        for part in &g.parts {
                            e.ownership.insert(*part, (None, true));
                        }
                        None
                    }
                }
                (current_owner, true) => {
                    let pos =
                        e.dm.get_raw(g.root, "CFrame")
                            .and_then(|v| v.as_cframe())
                            .map(|c| c.translation)
                            .unwrap_or_default();
                    let p = xz(pos);
                    let character_owner = clients.iter().find(|c| c.owns_group(&e.dm, g));
                    let new = if let Some(c) = character_owner {
                        Some(c.player)
                    } else {
                        let current = by_player(current_owner);
                        let closest = clients.iter().min_by(|a, b| {
                            a.relative_error(p)
                                .partial_cmp(&b.relative_error(p))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let current_ok = current.is_some_and(|c| c.in_range(p));
                        let closest_ok = closest.is_some_and(|c| {
                            current.is_none_or(|cur| cur.player != c.player) && c.in_range(p)
                        });
                        match (current, closest) {
                            (Some(cur), Some(near)) if current_ok => {
                                if closest_ok && closer_to_other(p, cur, near) {
                                    Some(near.player)
                                } else {
                                    Some(cur.player)
                                }
                            }
                            (_, Some(near)) if closest_ok => Some(near.player),
                            _ => None,
                        }
                    };
                    for part in &g.parts {
                        e.ownership.insert(*part, (new, true));
                    }
                    new
                }
            };
            let peer = owner
                .and_then(|p| rep.peer_of_player(p))
                .unwrap_or(SERVER_PEER);
            for &root in &g.assemblies {
                let previous = self.sent_owner.insert(root, peer);
                if previous == Some(peer) {
                    continue;
                }
                let net = rep.core.ids.net(root);
                let targets: Vec<PeerId> = rep
                    .peers()
                    .map(|(p, _)| p)
                    .filter(|p| *p == peer || previous == Some(*p))
                    .collect();
                for target in targets {
                    if rep.knows(target, root) {
                        rep.send(
                            target,
                            Message::Ownership {
                                id: net,
                                owner: peer,
                            },
                        );
                    }
                }
            }
        }
    }

    fn send_states(&mut self, e: &mut Engine, rep: &mut ServerReplicator, roots: &[NetworkRoot]) {
        let mut moving = Vec::new();
        for r in roots {
            let was_awake = self.awake.insert(r.id, !r.sleeping).unwrap_or(false);
            if !r.sleeping || was_awake {
                moving.push(r);
            }
        }
        let peers: Vec<(PeerId, Option<InstanceId>)> =
            rep.peers().map(|(id, p)| (id, p.player())).collect();
        for (peer, player) in peers {
            if !rep.peer(peer).is_some_and(|p| p.joined) {
                continue;
            }
            let states: Vec<PhysicsState> = moving
                .iter()
                .filter(|r| rep.knows(peer, r.id))
                .filter(|r| player.is_none() || e.ownership.get(&r.id).and_then(|o| o.0) != player)
                .map(|r| state(&rep.core.ids, r))
                .collect();
            for msg in chunks(states) {
                rep.send(peer, msg);
            }
        }
    }
}

#[derive(Default)]
pub struct ClientPhysics {
    frame: u32,
}

impl ClientPhysics {
    pub fn apply_incoming(&mut self, e: &mut Engine, rep: &mut ClientReplicator) {
        let incoming = rep.take_physics();
        if incoming.is_empty() {
            return;
        }
        let world = e.world.clone();
        let mut w = world.lock();
        w.sync(&mut e.dm);
        for st in incoming {
            let Some(local) = rep.core.ids.local(&e.dm, st.id) else {
                continue;
            };
            if rep.owned().contains(&local) {
                continue;
            }
            w.apply_network_state(&mut e.dm, local, st.cframe, st.velocity, st.rot_velocity);
        }
    }

    pub fn tick(&mut self, e: &mut Engine, rep: &mut ClientReplicator) {
        self.frame = self.frame.wrapping_add(1);
        if !self.frame.is_multiple_of(SEND_INTERVAL) || rep.owned().is_empty() || !rep.joined() {
            return;
        }
        let roots = {
            let world = e.world.clone();
            let mut w = world.lock();
            w.sync(&mut e.dm);
            w.rebuild_assemblies();
            w.network_roots()
        };
        let states: Vec<PhysicsState> = roots
            .iter()
            .filter(|r| !r.sleeping && rep.owned().contains(&r.id))
            .map(|r| state(&rep.core.ids, r))
            .collect();
        for msg in chunks(states) {
            rep.send(msg);
        }
    }
}
