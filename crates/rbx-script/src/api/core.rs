use super::{Registry, Ret, none};
use crate::convert::{arg_instance, arg_string};
use crate::runtime::{engine, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};
use std::path::{Path, PathBuf};

const COLLECTION_CLASSES: &[&str] = &[
    "Configuration",
    "CustomEventReceiver",
    "Dialog",
    "VehicleSeat",
];

pub fn register(r: &mut Registry) {
    r.add(
        "ScriptContext",
        &["AddCoreScriptLocal"],
        add_core_script_local,
    );
    r.add("CollectionService", &["GetCollection"], get_collection);
    r.add("GuiButton", &["SetVerb"], set_verb);
    r.add("RenderSettings", &["GetMaxQualityLevel"], max_quality_level);
    r.add("Teams", &["GetTeams"], get_teams);
    r.add("ContentProvider", &["Preload"], preload);
}

fn get_teams(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let teams: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.children_of(id)
            .iter()
            .copied()
            .filter(|c| e.dm.is_a(*c, "Team"))
            .collect()
    };
    let t = lua.create_table()?;
    for (i, team) in teams.into_iter().enumerate() {
        t.raw_set(i + 1, crate::instance::push_instance(lua, team)?)?;
    }
    super::ret(lua, t)
}

fn preload(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let eng = engine(lua);
    let e = eng.lock();
    if crate::content::fetch(&e, &url).is_none() {
        tracing::debug!("preload could not find {url}");
    }
    none()
}

fn max_quality_level(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    super::ret(lua, 22i64)
}

fn get_collection(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = arg_string(args.first()).unwrap_or_default();
    let items: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        if COLLECTION_CLASSES.contains(&class.as_str()) {
            let root = e.dm.root();
            e.dm.descendants(root)
                .into_iter()
                .filter(|d| e.dm.class_of(*d) == Some(class.as_str()))
                .collect()
        } else {
            Vec::new()
        }
    };
    let t = lua.create_table()?;
    for (i, id) in items.into_iter().enumerate() {
        t.raw_set(i + 1, crate::instance::push_instance(lua, id)?)?;
    }
    super::ret(lua, t)
}

pub fn collection_changed(
    lua: &Lua,
    ancestor: InstanceId,
    descendant: InstanceId,
    event: &'static str,
) {
    let target = {
        let eng = engine(lua);
        let e = eng.lock();
        if ancestor != e.dm.root() {
            return;
        }
        let class = e.dm.class_of(descendant).unwrap_or("");
        if !COLLECTION_CLASSES.contains(&class) {
            return;
        }
        e.dm.find_service("CollectionService")
    };
    if let Some(service) = target
        && let Ok(v) = crate::instance::push_instance(lua, descendant)
    {
        crate::runtime::fire_lua(lua, service, event, mlua::MultiValue::from_vec(vec![v]));
    }
}

fn set_verb(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let verb = arg_string(args.first()).unwrap_or_default();
    engine(lua).lock().gui.button_verbs.insert(id, verb);
    none()
}

fn scripts_dir(lua: &Lua) -> Option<PathBuf> {
    engine(lua)
        .lock()
        .content_root
        .as_ref()
        .map(|c| c.join("scripts"))
}

fn read_core_source(dir: &Path, name: &str) -> Option<String> {
    if name.contains("..") {
        return None;
    }
    let path = name
        .split('/')
        .fold(dir.to_path_buf(), |p, part| p.join(part))
        .with_extension("lua");
    std::fs::read_to_string(path).ok()
}

pub fn add_core_script(lua: &Lua, name: &str, parent: Option<InstanceId>) -> Result<InstanceId> {
    let dir = scripts_dir(lua).ok_or_else(|| rt(format!("Error loading core script {name}")))?;
    let source = read_core_source(&dir, name)
        .ok_or_else(|| rt(format!("Error loading core script {name}")))?;
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let parent = match parent {
            Some(p) => p,
            None => {
                e.dm.get_service("ScriptContext")
                    .map_err(|err| rt(err.to_string()))?
            }
        };
        let id =
            e.dm.create_internal("CoreScript")
                .map_err(|err| rt(err.to_string()))?;
        e.dm.set_name(id, name);
        (id, parent)
    };
    with_state(lua, |s| {
        s.core_sources.insert(id.0, source);
    });
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.dm.set_parent_ignore_lock(id.0, Some(id.1))
            .map_err(|err| rt(err.to_string()))?;
    }
    crate::scripts::reevaluate(lua, id.0);
    Ok(id.0)
}

fn add_core_script_local(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let parent = arg_instance(lua, args.get(1));
    add_core_script(lua, &name, parent)?;
    none()
}

fn collect_modules(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_modules(&path, &format!("{prefix}{name}/"), out);
        } else if let Some(stem) = name.strip_suffix(".lua")
            && let Ok(src) = std::fs::read_to_string(&path)
        {
            out.push((format!("{prefix}{stem}"), src));
        }
    }
}

pub fn load_core_modules(lua: &Lua) -> Result<()> {
    let Some(dir) = scripts_dir(lua) else {
        return Ok(());
    };
    let mut modules = Vec::new();
    collect_modules(&dir.join("Modules"), "", &mut modules);
    let eng = engine(lua);
    let mut e = eng.lock();
    let core =
        e.dm.get_service("CoreGui")
            .map_err(|err| rt(err.to_string()))?;
    let gui = match e.dm.find_first_child(core, "RobloxGui", false) {
        Some(g) => g,
        None => {
            let g =
                e.dm.create_internal("ScreenGui")
                    .map_err(|err| rt(err.to_string()))?;
            e.dm.set_name(g, "RobloxGui");
            let _ = e.dm.set_parent_ignore_lock(g, Some(core));
            g
        }
    };
    if e.dm.find_first_child(gui, "ControlFrame", false).is_none() {
        let frame = |e: &mut crate::runtime::Engine,
                     name: &str,
                     size: rbx_math::UDim2,
                     pos: rbx_math::UDim2,
                     parent: InstanceId|
         -> Result<InstanceId> {
            let f =
                e.dm.create_internal("Frame")
                    .map_err(|err| rt(err.to_string()))?;
            e.dm.set_name(f, name);
            let _ = e.dm.set(f, "BackgroundTransparency", Variant::Float(1.0));
            let _ = e.dm.set(f, "Size", Variant::UDim2(size));
            let _ = e.dm.set(f, "Position", Variant::UDim2(pos));
            let _ = e.dm.set_parent_ignore_lock(f, Some(parent));
            Ok(f)
        };
        use rbx_math::UDim2;
        let controls = frame(
            &mut e,
            "ControlFrame",
            UDim2::new(1.0, 0, 1.0, 0),
            UDim2::default(),
            gui,
        )?;
        frame(
            &mut e,
            "BottomLeftControl",
            UDim2::new(0.0, 130, 0.0, 46),
            UDim2::new(0.0, 0, 1.0, -46),
            controls,
        )?;
        frame(
            &mut e,
            "BottomRightControl",
            UDim2::new(0.0, 180, 0.0, 41),
            UDim2::new(1.0, -180, 1.0, -41),
            controls,
        )?;
        frame(
            &mut e,
            "TopLeftControl",
            UDim2::new(0.05, 0, 0.05, 0),
            UDim2::default(),
            controls,
        )?;
    }
    let folder = match e.dm.find_first_child(gui, "Modules", false) {
        Some(f) => f,
        None => {
            let f =
                e.dm.create_internal("Folder")
                    .map_err(|err| rt(err.to_string()))?;
            e.dm.set_name(f, "Modules");
            let _ = e.dm.set_parent_ignore_lock(f, Some(gui));
            f
        }
    };
    for (path, source) in modules {
        let mut parent = folder;
        let parts: Vec<&str> = path.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            parent = match e.dm.find_first_child(parent, part, false) {
                Some(p) => p,
                None => {
                    let f =
                        e.dm.create_internal("Folder")
                            .map_err(|err| rt(err.to_string()))?;
                    e.dm.set_name(f, part);
                    let _ = e.dm.set_parent_ignore_lock(f, Some(parent));
                    f
                }
            };
        }
        let m =
            e.dm.create_internal("ModuleScript")
                .map_err(|err| rt(err.to_string()))?;
        e.dm.set_name(m, parts[parts.len() - 1]);
        e.dm.set(m, "Source", Variant::String(source))
            .map_err(|err| rt(err.to_string()))?;
        let _ = e.dm.set_parent_ignore_lock(m, Some(parent));
    }
    Ok(())
}
