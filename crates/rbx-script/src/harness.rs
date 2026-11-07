use crate::runtime::{Engine, MessageType, ScriptRuntime};
use crate::security::Identity;
use parking_lot::Mutex;
use rbx_dom::DataModel;
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
pub const SCRIPT_TIMEOUT: f64 = 10.0;

pub fn run_place_file(path: &Path, content_root: Option<&Path>) -> Result<PlaceRun, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dm = DataModel::new();
    let load_warnings = dm
        .load_place(&data)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    run_datamodel(dm, path, content_root, load_warnings)
}

pub fn run_datamodel(
    dm: DataModel,
    path: &Path,
    content_root: Option<&Path>,
    load_warnings: Vec<String>,
) -> Result<PlaceRun, String> {
    let mut engine = Engine::new(dm);
    engine.content_root = content_root.map(Path::to_path_buf);
    let engine = Arc::new(Mutex::new(engine));
    let runtime = ScriptRuntime::new(engine.clone()).map_err(|e| e.to_string())?;
    crate::runtime::set_script_timeout(&runtime.lua, SCRIPT_TIMEOUT);
    let test_scripts = {
        let e = engine.lock();
        e.dm.find_service("TestService")
            .map(|ts| e.dm.children_of(ts).iter().any(|&c| e.dm.is_a(c, "Script")))
            .unwrap_or(false)
    };
    let players = {
        let e = engine.lock();
        e.dm.find_service("TestService")
            .and_then(|ts| e.dm.get(ts, "NumberOfPlayers").ok())
            .and_then(|v| v.as_i32())
            .unwrap_or(0)
    };
    engine.lock().running = true;
    runtime.scan_scripts();
    let mut run = PlaceRun {
        path: path.to_path_buf(),
        has_tests: test_scripts,
        load_warnings,
        players,
        ..PlaceRun::default()
    };
    if test_scripts {
        crate::api::test::start(&runtime.lua, Identity::LocalGui).map_err(|e| e.to_string())?;
        runtime.pump();
        let timeout = {
            let e = engine.lock();
            e.dm.find_service("TestService")
                .and_then(|ts| e.dm.get(ts, "Timeout").ok())
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0)
        };
        let max_frames = ((timeout + 2.0) / FRAME).ceil() as usize;
        for _ in 0..max_frames {
            if engine.lock().test.done {
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
        for _ in 0..frames {
            runtime.step(FRAME);
        }
        run.completed = true;
    }
    let e = engine.lock();
    run.game_time = e.game_time;
    run.script_errors = e
        .output
        .iter()
        .filter(|m| m.kind == MessageType::Error)
        .map(|m| m.text.clone())
        .collect();
    Ok(run)
}
