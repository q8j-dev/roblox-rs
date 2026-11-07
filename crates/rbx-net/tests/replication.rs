use rbx_dom::{DataModel, InstanceId, RemoteCall, RemoteTarget, Variant};
use rbx_math::Vec3;
use rbx_net::{
    ClientReplicator, Connection, MemoryConnection, PeerId, ServerReplicator, memory_pair,
};

struct Client {
    dm: DataModel,
    rep: ClientReplicator,
    peer: PeerId,
    server_end: MemoryConnection,
    client_end: MemoryConnection,
}

struct Net {
    dm: DataModel,
    server: ServerReplicator,
    clients: Vec<Client>,
}

fn place() -> DataModel {
    let mut dm = DataModel::new();
    for s in [
        "Workspace",
        "Players",
        "Lighting",
        "ReplicatedStorage",
        "ServerStorage",
    ] {
        dm.get_service(s).unwrap();
    }
    let ws = dm.find_service("Workspace").unwrap();
    let part = dm.create("Part").unwrap();
    dm.set_name(part, "Baseplate");
    dm.set(part, "Anchored", Variant::Bool(true)).unwrap();
    dm.set_parent(part, Some(ws)).unwrap();
    let ss = dm.find_service("ServerStorage").unwrap();
    let secret = dm.create("Folder").unwrap();
    dm.set_name(secret, "Secret");
    dm.set_parent(secret, Some(ss)).unwrap();
    dm
}

impl Net {
    fn new() -> Net {
        let mut dm = place();
        dm.frontend_processing = false;
        let server = ServerReplicator::new(&mut dm);
        Net {
            dm,
            server,
            clients: Vec::new(),
        }
    }

    fn join(&mut self, name: &str) -> usize {
        let mut dm = DataModel::new();
        dm.backend_processing = false;
        for s in ["Workspace", "Players", "Lighting", "ReplicatedStorage"] {
            dm.get_service(s).unwrap();
        }
        let rep = ClientReplicator::new(&mut dm);
        let (server_end, client_end) = memory_pair();
        let peer = self.server.connect();
        self.clients.push(Client {
            dm,
            rep,
            peer,
            server_end,
            client_end,
        });
        let idx = self.clients.len() - 1;
        self.pump();
        let c = &mut self.clients[idx];
        let players = c.dm.find_service("Players").unwrap();
        let player = c.dm.create_internal("Player").unwrap();
        c.dm.set_name(player, name);
        c.dm.set_parent(player, Some(players)).unwrap();
        c.dm.local_player = Some(player);
        self.pump();
        idx
    }

    fn pump(&mut self) {
        for _ in 0..6 {
            self.server.flush(&mut self.dm);
            for (peer, msg) in self.server.take_outgoing() {
                if let Some(c) = self.clients.iter_mut().find(|c| c.peer == peer) {
                    c.server_end.send(&msg);
                }
            }
            for c in &mut self.clients {
                while let Some(msg) = c.client_end.recv() {
                    c.rep.receive(&mut c.dm, msg.unwrap());
                }
                c.rep.flush(&mut c.dm);
                for msg in c.rep.take_outgoing() {
                    c.client_end.send(&msg);
                }
            }
            for i in 0..self.clients.len() {
                let peer = self.clients[i].peer;
                while let Some(msg) = self.clients[i].server_end.recv() {
                    self.server.receive(&mut self.dm, peer, msg.unwrap());
                }
            }
        }
    }
}

fn find(dm: &DataModel, path: &[&str]) -> Option<InstanceId> {
    let mut cur = dm.root();
    for name in path {
        cur = dm
            .children_of(cur)
            .iter()
            .copied()
            .find(|&c| dm.name_of(c) == *name || dm.class_of(c) == Some(name))?;
    }
    Some(cur)
}

#[test]
fn join_replicates_world_but_not_server_storage() {
    let mut net = Net::new();
    let a = net.join("Alice");
    let c = &net.clients[a];
    assert!(c.rep.joined());
    let part = find(&c.dm, &["Workspace", "Baseplate"]).expect("baseplate replicated");
    assert!(c.dm.get_bool(part, "Anchored"));
    assert!(find(&c.dm, &["ServerStorage"]).is_none());
    assert!(find(&net.dm, &["Players", "Alice"]).is_some());
}

#[test]
fn changes_replicate_both_ways_between_two_clients() {
    let mut net = Net::new();
    let a = net.join("Alice");
    let b = net.join("Bob");
    assert!(find(&net.clients[a].dm, &["Players", "Bob"]).is_some());
    assert!(find(&net.clients[b].dm, &["Players", "Alice"]).is_some());

    let ws = net.dm.find_service("Workspace").unwrap();
    let block = net.dm.create("Part").unwrap();
    net.dm.set_name(block, "ServerBlock");
    net.dm
        .set(block, "Size", Variant::Vector3(Vec3::new(3.0, 6.0, 7.0)))
        .unwrap();
    net.dm.set_parent(block, Some(ws)).unwrap();
    net.pump();
    for c in &net.clients {
        let p = find(&c.dm, &["Workspace", "ServerBlock"]).expect("server part reached client");
        assert_eq!(
            c.dm.get(p, "Size").unwrap(),
            Variant::Vector3(Vec3::new(3.0, 6.0, 7.0))
        );
    }

    net.dm.set_name(block, "Renamed");
    net.dm
        .set(block, "Transparency", Variant::Float(0.5))
        .unwrap();
    net.pump();
    for c in &net.clients {
        let p = find(&c.dm, &["Workspace", "Renamed"]).expect("rename replicated");
        assert_eq!(c.dm.get_f32(p, "Transparency"), 0.5);
    }

    let c = &mut net.clients[a];
    let cws = c.dm.find_service("Workspace").unwrap();
    let model = c.dm.create("Model").unwrap();
    c.dm.set_name(model, "AliceModel");
    let inner = c.dm.create("Part").unwrap();
    c.dm.set_name(inner, "Inner");
    c.dm.set(inner, "Reflectance", Variant::Float(0.25))
        .unwrap();
    c.dm.set_parent(inner, Some(model)).unwrap();
    c.dm.set_parent(model, Some(cws)).unwrap();
    net.pump();
    let sp =
        find(&net.dm, &["Workspace", "AliceModel", "Inner"]).expect("client part reached server");
    assert_eq!(net.dm.get_f32(sp, "Reflectance"), 0.25);
    let bp = find(&net.clients[b].dm, &["Workspace", "AliceModel", "Inner"])
        .expect("client part reached the other client");
    assert_eq!(net.clients[b].dm.get_f32(bp, "Reflectance"), 0.25);

    let c = &mut net.clients[b];
    let theirs = find(&c.dm, &["Workspace", "AliceModel", "Inner"]).unwrap();
    c.dm.set(theirs, "Reflectance", Variant::Float(0.75))
        .unwrap();
    net.pump();
    assert_eq!(net.dm.get_f32(sp, "Reflectance"), 0.75);
    let ap = find(&net.clients[a].dm, &["Workspace", "AliceModel", "Inner"]).unwrap();
    assert_eq!(net.clients[a].dm.get_f32(ap, "Reflectance"), 0.75);

    let c = &mut net.clients[a];
    let m = find(&c.dm, &["Workspace", "AliceModel"]).unwrap();
    c.dm.destroy(m).unwrap();
    net.pump();
    assert!(find(&net.dm, &["Workspace", "AliceModel"]).is_none());
    assert!(find(&net.clients[b].dm, &["Workspace", "AliceModel"]).is_none());

    net.dm.destroy(block).unwrap();
    net.pump();
    for c in &net.clients {
        assert!(find(&c.dm, &["Workspace", "Renamed"]).is_none());
    }
}

#[test]
fn reparenting_out_of_scope_deletes_on_clients() {
    let mut net = Net::new();
    let a = net.join("Alice");
    let part = find(&net.dm, &["Workspace", "Baseplate"]).unwrap();
    let ss = net.dm.find_service("ServerStorage").unwrap();
    net.dm.set_parent(part, Some(ss)).unwrap();
    net.pump();
    assert!(find(&net.clients[a].dm, &["Workspace", "Baseplate"]).is_none());
    let ws = net.dm.find_service("Workspace").unwrap();
    net.dm.set_parent(part, Some(ws)).unwrap();
    net.pump();
    assert!(find(&net.clients[a].dm, &["Workspace", "Baseplate"]).is_some());
}

#[test]
fn filtering_enabled_rejects_client_changes() {
    let mut net = Net::new();
    let ws = net.dm.find_service("Workspace").unwrap();
    net.dm
        .set(ws, "FilteringEnabled", Variant::Bool(true))
        .unwrap();
    let a = net.join("Alice");
    assert!(net.clients[a].rep.filtering);
    assert!(find(&net.dm, &["Players", "Alice"]).is_some());
    let c = &mut net.clients[a];
    let cws = c.dm.find_service("Workspace").unwrap();
    let p = c.dm.create("Part").unwrap();
    c.dm.set_name(p, "Hacked");
    c.dm.set_parent(p, Some(cws)).unwrap();
    let base = find(&c.dm, &["Workspace", "Baseplate"]).unwrap();
    c.dm.set(base, "Transparency", Variant::Float(1.0)).unwrap();
    net.pump();
    assert!(find(&net.dm, &["Workspace", "Hacked"]).is_none());
    let sb = find(&net.dm, &["Workspace", "Baseplate"]).unwrap();
    assert_eq!(net.dm.get_f32(sb, "Transparency"), 0.0);
    let peer = net.clients[a].peer;
    assert!(net.server.peer(peer).unwrap().rejected >= 2);
}

#[test]
fn references_resolve_across_peers() {
    let mut net = Net::new();
    let a = net.join("Alice");
    let ws = net.dm.find_service("Workspace").unwrap();
    let value = net.dm.create("ObjectValue").unwrap();
    net.dm.set_name(value, "Pointer");
    let target = net.dm.create("Part").unwrap();
    net.dm.set_name(target, "Target");
    net.dm
        .set(value, "Value", Variant::Ref(Some(target)))
        .unwrap();
    net.dm.set_parent(value, Some(ws)).unwrap();
    net.pump();
    net.dm.set_parent(target, Some(ws)).unwrap();
    net.pump();
    let c = &net.clients[a];
    let v = find(&c.dm, &["Workspace", "Pointer"]).unwrap();
    let t = find(&c.dm, &["Workspace", "Target"]).unwrap();
    assert_eq!(c.dm.get_ref(v, "Value"), Some(t));
}

#[test]
fn remote_events_cross_the_wire() {
    let mut net = Net::new();
    let a = net.join("Alice");
    let b = net.join("Bob");
    let rs = net.dm.find_service("ReplicatedStorage").unwrap();
    let ev = net.dm.create("RemoteEvent").unwrap();
    net.dm.set_name(ev, "Chat");
    net.dm.set_parent(ev, Some(rs)).unwrap();
    net.pump();
    net.dm.drain_signals();

    let c = &mut net.clients[a];
    let cev = find(&c.dm, &["ReplicatedStorage", "Chat"]).unwrap();
    c.dm.queue_remote(RemoteCall {
        instance: cev,
        target: RemoteTarget::Server,
        args: vec![Variant::String("hi".into())],
    });
    net.pump();
    let fired = net.dm.drain_signals();
    let alice = find(&net.dm, &["Players", "Alice"]).unwrap();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].0, ev);
    assert_eq!(fired[0].1, "OnServerEvent");
    assert_eq!(
        fired[0].2,
        vec![Variant::Ref(Some(alice)), Variant::String("hi".into())]
    );

    for c in &mut net.clients {
        c.dm.drain_signals();
    }
    net.dm.queue_remote(RemoteCall {
        instance: ev,
        target: RemoteTarget::AllPlayers,
        args: vec![Variant::Int(7)],
    });
    net.pump();
    for c in [a, b] {
        let fired = net.clients[c].dm.drain_signals();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].1, "OnClientEvent");
        assert_eq!(fired[0].2, vec![Variant::Int(7)]);
    }
}
