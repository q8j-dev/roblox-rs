use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_net::quic::{Certificate, QuicConnection, QuicServer, connect};
use rbx_net::{ClientReplicator, Connection, PeerId, ServerReplicator};
use std::time::{Duration, Instant};

struct Client {
    dm: DataModel,
    rep: ClientReplicator,
    conn: QuicConnection,
}

struct Server {
    dm: DataModel,
    rep: ServerReplicator,
    quic: QuicServer,
    conns: Vec<(PeerId, QuicConnection)>,
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

impl Server {
    fn pump(&mut self) {
        while let Some(conn) = self.quic.accept() {
            let peer = self.rep.connect();
            self.conns.push((peer, conn));
        }
        for (peer, conn) in &mut self.conns {
            while let Some(msg) = conn.recv() {
                self.rep.receive(&mut self.dm, *peer, msg.unwrap());
            }
        }
        self.rep.flush(&mut self.dm);
        for (peer, msg) in self.rep.take_outgoing() {
            if let Some((_, c)) = self.conns.iter_mut().find(|(p, _)| *p == peer) {
                c.send(&msg);
            }
        }
    }
}

impl Client {
    fn pump(&mut self) {
        while let Some(msg) = self.conn.recv() {
            self.rep.receive(&mut self.dm, msg.unwrap());
        }
        self.rep.flush(&mut self.dm);
        for msg in self.rep.take_outgoing() {
            self.conn.send(&msg);
        }
    }
}

fn pump_until(
    server: &mut Server,
    clients: &mut [Client],
    cond: impl Fn(&Server, &[Client]) -> bool,
) {
    let start = Instant::now();
    while !cond(server, clients) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for replication"
        );
        server.pump();
        for c in clients.iter_mut() {
            c.pump();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn client_dm() -> DataModel {
    let mut dm = DataModel::new();
    dm.backend_processing = false;
    for s in ["Workspace", "Players"] {
        dm.get_service(s).unwrap();
    }
    dm
}

#[test]
fn two_clients_replicate_both_ways_over_quic() {
    let mut dm = DataModel::new();
    dm.frontend_processing = false;
    let ws = dm.get_service("Workspace").unwrap();
    dm.get_service("Players").unwrap();
    let rep = ServerReplicator::new(&mut dm);
    let cert = Certificate::self_signed(&["localhost"]).unwrap();
    let quic = QuicServer::bind("127.0.0.1:0".parse().unwrap(), cert).unwrap();
    let addr = quic.local_addr().unwrap();
    let hash = quic.cert_hash();
    let mut server = Server {
        dm,
        rep,
        quic,
        conns: Vec::new(),
    };

    let mut clients = Vec::new();
    for name in ["Alice", "Bob"] {
        let mut dm = client_dm();
        let rep = ClientReplicator::new(&mut dm);
        let conn = connect(addr, hash).unwrap();
        let mut c = Client { dm, rep, conn };
        let players = c.dm.find_service("Players").unwrap();
        let player = c.dm.create_internal("Player").unwrap();
        c.dm.set_name(player, name);
        c.dm.set_parent(player, Some(players)).unwrap();
        c.dm.local_player = Some(player);
        clients.push(c);
    }
    pump_until(&mut server, &mut clients, |s, cs| {
        cs.iter().all(|c| c.rep.joined())
            && find(&s.dm, &["Players", "Alice"]).is_some()
            && find(&s.dm, &["Players", "Bob"]).is_some()
            && find(&cs[0].dm, &["Players", "Bob"]).is_some()
            && find(&cs[1].dm, &["Players", "Alice"]).is_some()
    });

    let part = server.dm.create("Part").unwrap();
    server.dm.set_name(part, "FromServer");
    server.dm.set_parent(part, Some(ws)).unwrap();
    pump_until(&mut server, &mut clients, |_, cs| {
        cs.iter()
            .all(|c| find(&c.dm, &["Workspace", "FromServer"]).is_some())
    });

    let c = &mut clients[0];
    let cws = c.dm.find_service("Workspace").unwrap();
    let mine = c.dm.create("Part").unwrap();
    c.dm.set_name(mine, "FromAlice");
    c.dm.set(mine, "Transparency", Variant::Float(0.5)).unwrap();
    c.dm.set_parent(mine, Some(cws)).unwrap();
    pump_until(&mut server, &mut clients, |s, cs| {
        find(&s.dm, &["Workspace", "FromAlice"]).is_some()
            && find(&cs[1].dm, &["Workspace", "FromAlice"])
                .is_some_and(|p| cs[1].dm.get_f32(p, "Transparency") == 0.5)
    });

    let c = &mut clients[1];
    let theirs = find(&c.dm, &["Workspace", "FromAlice"]).unwrap();
    c.dm.set(theirs, "Transparency", Variant::Float(0.25))
        .unwrap();
    pump_until(&mut server, &mut clients, |_, cs| {
        find(&cs[0].dm, &["Workspace", "FromAlice"])
            .is_some_and(|p| cs[0].dm.get_f32(p, "Transparency") == 0.25)
    });
}

#[test]
fn wrong_certificate_hash_is_rejected() {
    let cert = Certificate::self_signed(&["localhost"]).unwrap();
    let quic = QuicServer::bind("127.0.0.1:0".parse().unwrap(), cert).unwrap();
    let addr = quic.local_addr().unwrap();
    let mut bad = quic.cert_hash();
    bad[0] ^= 0xFF;
    assert!(connect(addr, bad).is_err());
}
