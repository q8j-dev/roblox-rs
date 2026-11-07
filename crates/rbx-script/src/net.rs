use crate::net_physics::{ClientPhysics, ServerPhysics};
use crate::runtime::{MessageType, RunMode, ScriptRuntime};
use rbx_dom::InstanceId;
use rbx_net::{ClientReplicator, ClientState, Connection, PeerId, ServerReplicator};
use std::collections::HashSet;

pub struct NetServer {
    pub rep: ServerReplicator,
    conns: Vec<(PeerId, Box<dyn Connection>)>,
    installed: HashSet<InstanceId>,
    physics: ServerPhysics,
}

impl NetServer {
    pub fn start(runtime: &ScriptRuntime) -> NetServer {
        let mut e = runtime.engine.lock();
        e.mode = RunMode::Server;
        let _ = e.dm.get_service("NetworkServer");
        e.dm.frontend_processing = false;
        let rep = ServerReplicator::new(&mut e.dm);
        NetServer {
            rep,
            conns: Vec::new(),
            installed: HashSet::new(),
            physics: ServerPhysics::default(),
        }
    }

    pub fn accept(&mut self, conn: Box<dyn Connection>) -> PeerId {
        let peer = self.rep.connect();
        self.conns.push((peer, conn));
        peer
    }

    pub fn peer_count(&self) -> usize {
        self.conns.len()
    }

    pub fn close_all(&mut self, runtime: &ScriptRuntime) {
        let peers: Vec<PeerId> = self.conns.iter().map(|(p, _)| *p).collect();
        for peer in peers {
            let mut e = runtime.engine.lock();
            self.rep.disconnect(&mut e.dm, peer);
        }
        for (_, c) in &mut self.conns {
            c.close();
        }
        self.conns.clear();
    }

    pub fn tick(&mut self, runtime: &ScriptRuntime) {
        let mut closed = Vec::new();
        for (peer, conn) in &mut self.conns {
            while let Some(msg) = conn.recv() {
                match msg {
                    Ok(msg) => {
                        let mut e = runtime.engine.lock();
                        self.rep.receive(&mut e.dm, *peer, msg);
                    }
                    Err(err) => {
                        tracing::warn!("peer {peer} sent a bad packet: {err}");
                        closed.push(*peer);
                        break;
                    }
                }
            }
            if conn.is_closed() {
                closed.push(*peer);
            }
        }
        for (peer, reason) in self.rep.take_kicked() {
            runtime.engine.lock().print(
                MessageType::Warning,
                format!("Disconnecting peer {peer}: {reason}"),
            );
            closed.push(peer);
        }
        closed.sort_unstable();
        closed.dedup();
        for peer in closed {
            {
                let mut e = runtime.engine.lock();
                self.rep.disconnect(&mut e.dm, peer);
            }
            if let Some(i) = self.conns.iter().position(|(p, _)| *p == peer) {
                let (_, mut conn) = self.conns.remove(i);
                for (to, msg) in self.rep.take_outgoing() {
                    if to == peer {
                        conn.send(&msg);
                    } else if let Some((_, c)) = self.conns.iter_mut().find(|(p, _)| *p == to) {
                        c.send(&msg);
                    }
                }
                conn.close();
            }
        }
        {
            let mut e = runtime.engine.lock();
            self.physics.apply_incoming(&mut e, &mut self.rep);
        }
        runtime.pump();
        self.install_players(runtime);
        runtime.engine.lock().client_count = self.conns.len();
        {
            let mut e = runtime.engine.lock();
            self.rep.flush(&mut e.dm);
            self.physics.tick(&mut e, &mut self.rep);
        }
        for (to, msg) in self.rep.take_outgoing() {
            if let Some((_, c)) = self.conns.iter_mut().find(|(p, _)| *p == to) {
                c.send(&msg);
            }
        }
    }

    fn install_players(&mut self, runtime: &ScriptRuntime) {
        let fresh: Vec<InstanceId> = self
            .rep
            .peers()
            .filter_map(|(_, p)| p.player())
            .filter(|p| !self.installed.contains(p))
            .collect();
        for player in fresh {
            self.installed.insert(player);
            let auto = {
                let e = runtime.engine.lock();
                e.dm.find_service("Players")
                    .is_none_or(|ps| e.dm.get_bool(ps, "CharacterAutoLoads"))
            };
            if auto
                && let Err(err) =
                    crate::api::players::load_character(&runtime.lua, player, true, "")
            {
                runtime
                    .engine
                    .lock()
                    .print(MessageType::Error, err.to_string());
            }
        }
        let e = runtime.engine.lock();
        self.installed.retain(|p| e.dm.parent_of(*p).is_some());
    }
}

pub struct NetClient {
    pub rep: ClientReplicator,
    conn: Box<dyn Connection>,
    physics: ClientPhysics,
}

impl NetClient {
    pub fn connect(runtime: &ScriptRuntime, conn: Box<dyn Connection>) -> NetClient {
        let mut e = runtime.engine.lock();
        e.mode = RunMode::Client;
        let _ = e.dm.get_service("NetworkClient");
        e.dm.backend_processing = false;
        let rep = ClientReplicator::new(&mut e.dm);
        NetClient {
            rep,
            conn,
            physics: ClientPhysics::default(),
        }
    }

    pub fn joined(&self) -> bool {
        self.rep.joined()
    }

    pub fn rejected(&self) -> Option<&str> {
        match &self.rep.state {
            ClientState::Rejected(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.conn.is_closed()
    }

    pub fn tick(&mut self, runtime: &ScriptRuntime) {
        while let Some(msg) = self.conn.recv() {
            match msg {
                Ok(msg) => {
                    let mut e = runtime.engine.lock();
                    self.rep.receive(&mut e.dm, msg);
                }
                Err(err) => {
                    tracing::warn!("server sent a bad packet: {err}");
                    self.conn.close();
                    break;
                }
            }
        }
        {
            let mut e = runtime.engine.lock();
            self.physics.apply_incoming(&mut e, &mut self.rep);
        }
        runtime.pump();
        {
            let mut e = runtime.engine.lock();
            self.rep.flush(&mut e.dm);
            self.physics.tick(&mut e, &mut self.rep);
        }
        for msg in self.rep.take_outgoing() {
            self.conn.send(&msg);
        }
    }
}

pub struct ClientSession {
    pub runtime: ScriptRuntime,
    pub net: NetClient,
}

impl ClientSession {
    pub fn new(
        dm: rbx_dom::DataModel,
        conn: Box<dyn Connection>,
        player_name: &str,
        user_id: i32,
    ) -> Result<ClientSession, String> {
        let mut engine = crate::runtime::Engine::new(dm);
        engine.running = true;
        let runtime = ScriptRuntime::new(std::sync::Arc::new(parking_lot::Mutex::new(engine)))
            .map_err(|e| e.to_string())?;
        let net = NetClient::connect(&runtime, conn);
        let players = runtime
            .engine
            .lock()
            .dm
            .get_service("Players")
            .map_err(|e| e.to_string())?;
        crate::api::players::make_local_player(
            &runtime.lua,
            players,
            user_id,
            false,
            Some(player_name),
        )
        .map_err(|e| e.to_string())?;
        runtime.scan_scripts();
        Ok(ClientSession { runtime, net })
    }

    pub fn step(&mut self, dt: f64) {
        self.net.tick(&self.runtime);
        self.runtime.step(dt);
        self.net.tick(&self.runtime);
    }

    pub fn joined(&self) -> bool {
        self.net.joined()
    }
}
