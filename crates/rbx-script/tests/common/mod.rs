#![allow(dead_code)]

use mlua::{MultiValue, Value};
use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId};
use rbx_script::{Engine, Identity, MessageType, ScriptRuntime, Shared};
use std::sync::Arc;

pub struct Fixture {
    pub rt: ScriptRuntime,
}

impl Fixture {
    pub fn new() -> Fixture {
        let mut dm = DataModel::new();
        for s in [
            "Workspace",
            "Lighting",
            "Players",
            "ReplicatedStorage",
            "ServerScriptService",
            "ServerStorage",
            "RunService",
            "TestService",
        ] {
            let _ = dm.get_service(s);
        }
        let engine = Arc::new(Mutex::new(Engine::new(dm)));
        let rt = match ScriptRuntime::new(engine) {
            Ok(r) => r,
            Err(e) => panic!("runtime: {e}"),
        };
        Fixture { rt }
    }

    pub fn engine(&self) -> Shared {
        self.rt.engine.clone()
    }

    pub fn execute_as(&self, src: &str, identity: Identity) -> Result<Vec<Value>, String> {
        self.rt
            .execute_in_new_thread(src, identity, MultiValue::new())
            .map(|v| v.into_vec())
    }

    pub fn execute(&self, src: &str) -> Result<Vec<Value>, String> {
        self.execute_as(src, Identity::CmdLine)
    }

    pub fn bool(&self, src: &str) -> bool {
        match self.execute(src) {
            Ok(v) => matches!(v.first(), Some(Value::Boolean(true))),
            Err(e) => panic!("{src}: {e}"),
        }
    }

    pub fn number(&self, src: &str) -> f64 {
        match self.execute(src) {
            Ok(v) => match v.first() {
                Some(Value::Number(n)) => *n,
                Some(Value::Integer(n)) => *n as f64,
                other => panic!("{src}: expected a number, got {other:?}"),
            },
            Err(e) => panic!("{src}: {e}"),
        }
    }

    pub fn string(&self, src: &str) -> String {
        match self.execute(src) {
            Ok(v) => match v.first() {
                Some(Value::String(s)) => s.to_string_lossy().to_string(),
                other => panic!("{src}: expected a string, got {other:?}"),
            },
            Err(e) => panic!("{src}: {e}"),
        }
    }

    pub fn instance(&self, src: &str) -> InstanceId {
        let v = self.execute(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let first = v.first().cloned().unwrap_or(Value::Nil);
        rbx_script::instance_from_value(&self.rt.lua, &first)
            .unwrap_or_else(|| panic!("{src}: not an instance"))
    }

    pub fn run(&self) {
        self.rt.engine.lock().running = true;
    }

    pub fn step_seconds(&self, seconds: f64) {
        let frames = (seconds * 60.0).round() as usize;
        for _ in 0..frames {
            self.rt.step(1.0 / 60.0);
        }
    }

    pub fn workspace(&self) -> InstanceId {
        self.rt
            .engine
            .lock()
            .dm
            .find_service("Workspace")
            .expect("workspace")
    }

    pub fn add_script(&self, class: &str, source: &str, parent: InstanceId) -> InstanceId {
        let id = {
            let mut e = self.rt.engine.lock();
            let s = e.dm.create(class).expect("script");
            let _ = e.dm.set(
                s,
                "Source",
                rbx_dom::Variant::ProtectedString(source.to_string()),
            );
            e.dm.set_parent(s, Some(parent)).expect("parent");
            s
        };
        self.rt.scan_scripts();
        id
    }

    pub fn name_of(&self, id: InstanceId) -> String {
        self.rt.engine.lock().dm.name_of(id).to_string()
    }

    pub fn errors(&self) -> Vec<String> {
        self.rt
            .engine
            .lock()
            .output
            .iter()
            .filter(|m| m.kind == MessageType::Error)
            .map(|m| m.text.clone())
            .collect()
    }

    pub fn output(&self) -> Vec<String> {
        self.rt
            .engine
            .lock()
            .output
            .iter()
            .map(|m| m.text.clone())
            .collect()
    }
}
