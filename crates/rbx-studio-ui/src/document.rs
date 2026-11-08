use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_script::{Engine, ScriptRuntime};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const UNDO_LIMIT: usize = 64;

pub struct Document {
    pub runtime: ScriptRuntime,
    pub path: Option<PathBuf>,
    pub content_root: PathBuf,
    pub dirty: bool,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
}

fn is_xml(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("rbxlx"))
}

fn runtime_for(dm: DataModel, content_root: &Path) -> Result<ScriptRuntime, String> {
    let mut engine = Engine::new(dm);
    engine.content_root = Some(content_root.to_path_buf());
    engine.host_selection = true;
    engine.process_after_load();
    ScriptRuntime::new(Arc::new(Mutex::new(engine))).map_err(|e| e.to_string())
}

fn load(data: &[u8]) -> Result<DataModel, String> {
    let mut dm = DataModel::new();
    dm.load_place(data).map_err(|e| e.to_string())?;
    dm.ensure_terrain();
    Ok(dm)
}

impl Document {
    pub fn new_baseplate(content_root: &Path) -> Result<Document, String> {
        let mut dm = DataModel::new();
        dm.ensure_terrain();
        let ws = dm.get_service("Workspace").map_err(|e| e.to_string())?;
        for s in [
            "Players",
            "Lighting",
            "ReplicatedStorage",
            "ServerScriptService",
            "ServerStorage",
            "StarterGui",
            "StarterPack",
            "StarterPlayer",
            "SoundService",
        ] {
            let _ = dm.get_service(s);
        }
        let base = dm.create("Part").map_err(|e| e.to_string())?;
        dm.set_name(base, "BasePlate");
        let _ = dm.set(base, "Anchored", Variant::Bool(true));
        let _ = dm.set(base, "Locked", Variant::Bool(true));
        let _ = dm.set(
            base,
            "Size",
            Variant::Vector3(Vec3::new(512.0, 20.0, 512.0)),
        );
        let _ = dm.set(
            base,
            "CFrame",
            Variant::CFrame(CFrame::from_translation(Vec3::new(0.0, -10.0, 0.0))),
        );
        dm.set_parent(base, Some(ws)).map_err(|e| e.to_string())?;
        Ok(Document {
            runtime: runtime_for(dm, content_root)?,
            path: None,
            content_root: content_root.to_path_buf(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn from_runtime(runtime: ScriptRuntime, content_root: &Path) -> Document {
        runtime.engine.lock().host_selection = true;
        Document {
            runtime,
            path: None,
            content_root: content_root.to_path_buf(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn open(path: &Path, content_root: &Path) -> Result<Document, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let dm = load(&data)?;
        Ok(Document {
            runtime: runtime_for(dm, content_root)?,
            path: Some(path.to_path_buf()),
            content_root: content_root.to_path_buf(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.runtime
            .engine
            .lock()
            .dm
            .save_place_binary()
            .map_err(|e| e.to_string())
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let data = {
            let e = self.runtime.engine.lock();
            if is_xml(path) {
                e.dm.save_place_xml().into_bytes()
            } else {
                e.dm.save_place_binary().map_err(|e| e.to_string())?
            }
        };
        std::fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = self.path.clone().ok_or("the place has no file yet")?;
        self.save_to(&path)
    }

    pub fn checkpoint(&mut self) {
        if let Ok(b) = self.bytes() {
            self.undo.push(b);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.dirty = true;
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), String> {
        let dm = load(data)?;
        self.runtime = runtime_for(dm, &self.content_root)?;
        self.dirty = true;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, String> {
        let Some(prev) = self.undo.pop() else {
            return Ok(false);
        };
        let now = self.bytes()?;
        self.redo.push(now);
        self.restore(&prev)?;
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, String> {
        let Some(next) = self.redo.pop() else {
            return Ok(false);
        };
        let now = self.bytes()?;
        self.undo.push(now);
        self.restore(&next)?;
        Ok(true)
    }

    pub fn selection(&self) -> Vec<InstanceId> {
        rbx_script::api::selection::current(&self.runtime.lua)
    }

    pub fn select(&self, items: Vec<InstanceId>) {
        rbx_script::api::selection::set(&self.runtime.lua, items);
    }

    pub fn set_property(
        &mut self,
        id: InstanceId,
        name: &str,
        value: Variant,
    ) -> Result<(), String> {
        self.checkpoint();
        self.runtime
            .engine
            .lock()
            .dm
            .set(id, name, value)
            .map_err(|e| e.to_string())
    }

    pub fn rename(&mut self, id: InstanceId, name: &str) {
        self.checkpoint();
        self.runtime.engine.lock().dm.set_name(id, name);
    }

    pub fn insert(&mut self, class: &str, parent: InstanceId) -> Result<InstanceId, String> {
        self.checkpoint();
        let mut e = self.runtime.engine.lock();
        let id = e.dm.create(class).map_err(|err| err.to_string())?;
        if e.dm.is_a(id, "BasePart") {
            let _ = e.dm.set(id, "Anchored", Variant::Bool(true));
            let _ = e.dm.set(
                id,
                "CFrame",
                Variant::CFrame(CFrame::from_translation(Vec3::new(0.0, 0.5, 0.0))),
            );
        }
        e.dm.set_parent(id, Some(parent))
            .map_err(|err| err.to_string())?;
        drop(e);
        self.select(vec![id]);
        Ok(id)
    }

    pub fn delete_selection(&mut self) {
        let items = self.selection();
        if items.is_empty() {
            return;
        }
        self.checkpoint();
        let mut e = self.runtime.engine.lock();
        for id in items {
            let root = e.dm.root();
            if e.dm.contains(id) && e.dm.parent_of(id) != Some(root) {
                let _ = e.dm.destroy(id);
            }
        }
        drop(e);
        self.select(Vec::new());
    }

    pub fn run_verb(&mut self, verb: &str) -> bool {
        if matches!(verb, "Cut" | "Paste" | "PasteInto" | "Delete" | "Duplicate") {
            self.checkpoint();
        }
        rbx_script::api::verbs::run(&self.runtime.lua, verb)
    }

    fn cframe_of(&self, id: InstanceId) -> Option<CFrame> {
        self.runtime
            .engine
            .lock()
            .dm
            .get(id, "CFrame")
            .ok()
            .and_then(|v| v.as_cframe())
    }

    fn parts_of(&self, ids: &[InstanceId]) -> Vec<InstanceId> {
        let e = self.runtime.engine.lock();
        let mut out = Vec::new();
        for &id in ids {
            if e.dm.is_a(id, "BasePart") {
                out.push(id);
            } else {
                out.extend(
                    e.dm.descendants(id)
                        .into_iter()
                        .filter(|d| e.dm.is_a(*d, "BasePart")),
                );
            }
        }
        out
    }

    pub fn translate(&mut self, ids: &[InstanceId], delta: Vec3) {
        let parts = self.parts_of(ids);
        if parts.is_empty() || delta == Vec3::ZERO {
            return;
        }
        let mut e = self.runtime.engine.lock();
        for p in parts {
            if let Some(cf) = e.dm.get(p, "CFrame").ok().and_then(|v| v.as_cframe()) {
                let moved = CFrame::new(cf.rotation, cf.translation + delta);
                let _ = e.dm.set(p, "CFrame", Variant::CFrame(moved));
            }
        }
        self.dirty = true;
    }

    pub fn rotate(&mut self, ids: &[InstanceId], axis: Vec3, angle: f32) {
        let parts = self.parts_of(ids);
        if parts.is_empty() {
            return;
        }
        let centers: Vec<Vec3> = parts
            .iter()
            .filter_map(|&p| self.cframe_of(p).map(|c| c.translation))
            .collect();
        let pivot =
            centers.iter().copied().fold(Vec3::ZERO, |a, b| a + b) / centers.len().max(1) as f32;
        let rot = Mat3::from_axis_angle(axis.normalize(), angle);
        let mut e = self.runtime.engine.lock();
        for p in parts {
            if let Some(cf) = e.dm.get(p, "CFrame").ok().and_then(|v| v.as_cframe()) {
                let pos = pivot + rot * (cf.translation - pivot);
                let turned = CFrame::new(rot * cf.rotation, pos);
                let _ = e.dm.set(p, "CFrame", Variant::CFrame(turned));
            }
        }
        self.dirty = true;
    }

    pub fn resize(&mut self, id: InstanceId, local_axis: Vec3, amount: f32) {
        let mut e = self.runtime.engine.lock();
        if !e.dm.is_a(id, "BasePart") {
            return;
        }
        let (Some(size), Some(cf)) = (
            e.dm.get(id, "Size").ok().and_then(|v| v.as_vec3()),
            e.dm.get(id, "CFrame").ok().and_then(|v| v.as_cframe()),
        ) else {
            return;
        };
        let along = local_axis.abs();
        let new_size = (size + along * amount).max(Vec3::splat(0.2));
        let grown = new_size - size;
        let shift = cf.rotation * (local_axis * (grown.dot(along) * 0.5));
        let _ = e.dm.set(id, "Size", Variant::Vector3(new_size));
        let _ = e.dm.set(
            id,
            "CFrame",
            Variant::CFrame(CFrame::new(cf.rotation, cf.translation + shift)),
        );
        self.dirty = true;
    }

    pub fn script_source(&self, id: InstanceId) -> Option<String> {
        match self.runtime.engine.lock().dm.get(id, "Source").ok()? {
            Variant::ProtectedString(s) | Variant::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn set_script_source(&mut self, id: InstanceId, source: &str) {
        self.checkpoint();
        let _ = self.runtime.engine.lock().dm.set(
            id,
            "Source",
            Variant::ProtectedString(source.to_string()),
        );
    }

    pub fn write_script_source(&mut self, id: InstanceId, source: &str) {
        let _ = self.runtime.engine.lock().dm.set(
            id,
            "Source",
            Variant::ProtectedString(source.to_string()),
        );
        self.dirty = true;
    }
}
