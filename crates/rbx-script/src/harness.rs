use crate::net::{NetClient, NetServer};
use crate::runtime::{Engine, MessageType, ScriptRuntime, Shared};
use crate::security::Identity;
use parking_lot::Mutex;
use rbx_dom::{DataModel, Variant};
use rbx_net::memory_pair;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct PlaceRun {
    pub path: PathBuf,
    pub has_tests: bool,
    pub tests: u32,
    pub warns: u32,
    pub errors: u32,
    pub completed: bool,
    pub failures: Vec<String>,
    pub script_errors: Vec<String>,
    pub game_time: f64,
    pub load_warnings: Vec<String>,
    pub players: i32,
    pub output: Vec<String>,
    pub timed_out: bool,
}

impl PlaceRun {
    pub fn passed(&self) -> bool {
        if self.has_tests {
            self.completed && self.errors == 0
        } else {
            self.completed
        }
    }
}

pub const FRAME: f64 = 1.0 / 60.0;
pub const EMPTY_TEST_SECONDS: f64 = 5.0;
pub const SCRIPT_TIMEOUT: f64 = 120.0;
pub fn wall_limit() -> std::time::Duration {
    let secs = std::env::var("RBX_PLACE_WALL_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    std::time::Duration::from_secs(secs)
}

fn load_file(path: &Path) -> Result<(DataModel, Vec<String>), String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dm = DataModel::new();
    let load_warnings = dm
        .load_place(&data)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    dm.ensure_terrain();
    Ok((dm, load_warnings))
}

pub fn is_performance_test(dm: &DataModel) -> bool {
    dm.find_service("TestService").is_some_and(|t| {
        let flag = |name: &str| {
            dm.get(t, name)
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
        };
        !flag("Is30FpsThrottleEnabled") || !flag("IsPhysicsEnvironmentalThrottled")
    })
}

pub fn is_performance_place(path: &Path) -> bool {
    load_file(path).is_ok_and(|(dm, _)| is_performance_test(&dm))
}

pub fn run_place_file(path: &Path, content_root: Option<&Path>) -> Result<PlaceRun, String> {
    let (dm, load_warnings) = load_file(path)?;
    run_datamodel(dm, path, content_root, load_warnings)
}

fn test_service_value(engine: &Shared, name: &str) -> Option<Variant> {
    let e = engine.lock();
    e.dm.find_service("TestService")
        .and_then(|ts| e.dm.get(ts, name).ok())
}

fn has_test_scripts(engine: &Shared) -> bool {
    let e = engine.lock();
    e.dm.find_service("TestService")
        .map(|ts| e.dm.children_of(ts).iter().any(|&c| e.dm.is_a(c, "Script")))
        .unwrap_or(false)
}

fn prepare(
    dm: DataModel,
    path: &Path,
    content_root: Option<&Path>,
) -> Result<ScriptRuntime, String> {
    rbx_base::flags::set_flag("MaterialPropertiesEnabled", "false");
    let mut engine = Engine::new(dm);
    engine.content_root = content_root.map(Path::to_path_buf);
    if let Some(tests) = path
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "places"))
        .and_then(Path::parent)
    {
        engine.asset_dirs.push(tests.join("assets"));
    }
    engine.process_after_load();
    if !is_performance_test(&engine.dm) {
        engine.virtual_clock = Some(
            rbx_base::time::SystemTime::now()
                .duration_since(rbx_base::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
        );
    }
    let engine = Arc::new(Mutex::new(engine));
    let runtime = ScriptRuntime::new(engine).map_err(|e| e.to_string())?;
    crate::runtime::set_script_timeout(&runtime.lua, SCRIPT_TIMEOUT);
    if let Ok(math) = runtime.lua.globals().get::<mlua::Table>("math")
        && let Ok(seed) = math.get::<mlua::Function>("randomseed")
    {
        let _ = seed.call::<()>(1);
    }
    Ok(runtime)
}

fn arm(runtime: &ScriptRuntime, test_scripts: bool) {
    let auto_runs = test_service_value(&runtime.engine, "AutoRuns")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut e = runtime.engine.lock();
    if test_scripts && !auto_runs {
        e.running = false;
        e.stopped = true;
    } else {
        e.running = true;
    }
}

fn finish(run: &mut PlaceRun, engine: &Shared) {
    let e = engine.lock();
    run.game_time = e.game_time;
    run.output = e
        .output
        .iter()
        .map(|m| format!("{:?}: {}", m.kind, m.text))
        .collect();
    run.script_errors = e
        .output
        .iter()
        .filter(|m| m.kind == MessageType::Error)
        .map(|m| m.text.clone())
        .collect();
}

fn test_timeout(engine: &Shared) -> f64 {
    test_service_value(engine, "Timeout")
        .and_then(|v| v.as_f64())
        .unwrap_or(10.0)
}

pub fn run_datamodel(
    dm: DataModel,
    path: &Path,
    content_root: Option<&Path>,
    load_warnings: Vec<String>,
) -> Result<PlaceRun, String> {
    let runtime = prepare(dm, path, content_root)?;
    let engine = runtime.engine.clone();
    let test_scripts = has_test_scripts(&engine);
    let players = test_service_value(&engine, "NumberOfPlayers")
        .and_then(|v| v.as_i32())
        .unwrap_or(0);
    let mut run = PlaceRun {
        path: path.to_path_buf(),
        has_tests: test_scripts,
        load_warnings,
        players,
        ..PlaceRun::default()
    };
    if players > 0 && test_scripts {
        run_multiplayer(runtime, &mut run, path, content_root, players as usize)?;
        return Ok(run);
    }
    arm(&runtime, test_scripts);
    runtime.scan_scripts();
    if test_scripts {
        crate::api::test::start(&runtime.lua, Identity::LocalGui).map_err(|e| e.to_string())?;
        runtime.pump();
        let max_frames = ((test_timeout(&engine) + 2.0) / FRAME).ceil() as usize;
        let started = rbx_base::time::Instant::now();
        for _ in 0..max_frames {
            if engine.lock().test.done {
                break;
            }
            if started.elapsed() > wall_limit() {
                run.timed_out = true;
                break;
            }
            runtime.step(FRAME);
        }
        let e = engine.lock();
        run.completed = e.test.done;
        run.tests = e.test.tests;
        run.warns = e.test.warns;
        run.errors = e.test.errors;
        run.failures = e.test.failures.clone();
    } else {
        let frames = (EMPTY_TEST_SECONDS / FRAME).ceil() as usize;
        let started = rbx_base::time::Instant::now();
        for _ in 0..frames {
            if started.elapsed() > wall_limit() {
                run.timed_out = true;
                break;
            }
            runtime.step(FRAME);
        }
        run.completed = true;
    }
    finish(&mut run, &engine);
    Ok(run)
}

struct ClientNode {
    runtime: ScriptRuntime,
    net: NetClient,
    tested: bool,
}

fn run_multiplayer(
    server: ScriptRuntime,
    run: &mut PlaceRun,
    path: &Path,
    content_root: Option<&Path>,
    players: usize,
) -> Result<(), String> {
    let mut net = NetServer::start(&server);
    arm(&server, true);
    server.scan_scripts();
    crate::api::test::start(&server.lua, Identity::LocalGui).map_err(|e| e.to_string())?;
    server.pump();
    let client_file = path.with_extension("clienttest");
    let mut clients = Vec::with_capacity(players);
    for i in 0..players {
        let dm = if client_file.is_file() {
            load_file(&client_file)?.0
        } else {
            let mut dm = DataModel::new();
            for s in [
                "Workspace",
                "Players",
                "Lighting",
                "StarterGui",
                "StarterPack",
            ] {
                let _ = dm.get_service(s);
            }
            dm.ensure_terrain();
            dm
        };
        let runtime = prepare(dm, &client_file, content_root)?;
        let (server_end, client_end) = memory_pair();
        net.accept(Box::new(server_end));
        let client = NetClient::connect(&runtime, Box::new(client_end));
        arm(&runtime, has_test_scripts(&runtime.engine));
        let players_service = runtime
            .engine
            .lock()
            .dm
            .get_service("Players")
            .map_err(|e| e.to_string())?;
        crate::api::players::make_local_player(
            &runtime.lua,
            players_service,
            -(i as i32 + 1),
            false,
            Some(&format!("Player{}", i + 1)),
        )
        .map_err(|e| e.to_string())?;
        runtime.scan_scripts();
        let tested = has_test_scripts(&runtime.engine);
        if tested {
            crate::api::test::start(&runtime.lua, Identity::LocalGui).map_err(|e| e.to_string())?;
        }
        clients.push(ClientNode {
            runtime,
            net: client,
            tested,
        });
    }
    let max_frames = ((test_timeout(&server.engine) + 2.0) / FRAME).ceil() as usize;
    let trace_server = std::env::var("RBX_TRACE_SERVER").ok();
    let trace_client = std::env::var("RBX_TRACE_CLIENT").ok();
    let trace_every: usize = std::env::var("RBX_TRACE_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
        .max(1);
    let started = rbx_base::time::Instant::now();
    for frame in 0..max_frames {
        if server.engine.lock().test.done {
            break;
        }
        if started.elapsed() > wall_limit() {
            run.timed_out = true;
            break;
        }
        if frame % trace_every == 0 {
            if let Some(src) = &trace_server {
                let r = server.execute_named(src, "Trace", Identity::CmdLine, Default::default());
                eprintln!("trace t={:.1} server: {r:?}", frame as f64 * FRAME);
            }
            if let Some(src) = &trace_client {
                for (i, c) in clients.iter().enumerate() {
                    let r = c.runtime.execute_named(
                        src,
                        "Trace",
                        Identity::CmdLine,
                        Default::default(),
                    );
                    eprintln!(
                        "trace t={:.1} client {}: {r:?}",
                        frame as f64 * FRAME,
                        i + 1
                    );
                }
            }
        }
        server.step(FRAME);
        net.tick(&server);
        for c in &mut clients {
            c.net.tick(&c.runtime);
            c.runtime.step(FRAME);
            c.net.tick(&c.runtime);
        }
        net.tick(&server);
    }
    let e = server.engine.lock();
    run.completed = e.test.done;
    run.tests = e.test.tests;
    run.warns = e.test.warns;
    run.errors = e.test.errors;
    run.failures = e.test.failures.clone();
    drop(e);
    finish(run, &server.engine);
    for (i, c) in clients.iter().enumerate() {
        if !c.net.joined() {
            run.output.push(format!("client {} never joined", i + 1));
        }
        let e = c.runtime.engine.lock();
        if c.tested {
            run.output.extend(
                e.test
                    .failures
                    .iter()
                    .map(|f| format!("client {} check failed: {f}", i + 1)),
            );
        }
        run.output.extend(
            e.output
                .iter()
                .map(|m| format!("client {} {:?}: {}", i + 1, m.kind, m.text)),
        );
    }
    Ok(())
}
