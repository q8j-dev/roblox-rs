use super::{Registry, Ret, none};
use crate::convert::{arg_instance, arg_string};
use crate::runtime::{engine, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    r.add("ContentProvider", &["PreloadAsync"], preload_async);
    r.add(
        "ContentProvider",
        &["GetAssetFetchStatus"],
        asset_fetch_status,
    );
    r.add(
        "ContentProvider",
        &["GetAssetFetchStatusChangedSignal"],
        asset_fetch_status_signal,
    );
    r.add("ParticleEmitter", &["Emit"], emit_particles);
}

fn emit_particles(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let n = crate::convert::arg_number(args.first())
        .unwrap_or(16.0)
        .max(0.0) as u64;
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        *e.dm.particle_emits.entry(id).or_insert(0) += n;
    }
    crate::runtime::fire_lua(
        lua,
        id,
        "OnEmitRequested",
        mlua::MultiValue::from_vec(vec![Value::Integer(n as mlua::Integer)]),
    );
    none()
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
    rbx_base::vfs::read_to_string(path).ok()
}

enum CoreSource {
    Text(String),
    Bytecode(Arc<Vec<u8>>),
}

fn core_source(lua: &Lua, name: &str) -> Option<CoreSource> {
    let packaged = with_state(lua, |s| {
        s.core_package_scripts
            .as_ref()
            .map(|scripts| scripts.get(name).cloned())
    });
    match packaged {
        Some(found) => found.map(CoreSource::Bytecode),
        None => read_core_source(&scripts_dir(lua)?, name).map(CoreSource::Text),
    }
}

pub fn add_core_script(lua: &Lua, name: &str, parent: Option<InstanceId>) -> Result<InstanceId> {
    let source =
        core_source(lua, name).ok_or_else(|| rt(format!("Error loading core script {name}")))?;
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
    with_state(lua, |s| match source {
        CoreSource::Text(text) => {
            s.core_sources.insert(id.0, text);
        }
        CoreSource::Bytecode(code) => {
            s.core_bytecode.insert(id.0, code);
        }
    });
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.dm.set_parent_ignore_lock(id.0, Some(id.1))
            .map_err(|err| rt(err.to_string()))?;
    }
    crate::scripts::start_now(lua, id.0);
    Ok(id.0)
}

fn add_core_script_local(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let parent = arg_instance(lua, args.get(1));
    add_core_script(lua, &name, parent)?;
    none()
}

fn collect_modules(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    let Ok(mut entries) = rbx_base::vfs::read_dir(dir) else {
        return;
    };
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if rbx_base::vfs::is_dir(&path) {
            collect_modules(&path, &format!("{prefix}{name}/"), out);
        } else if let Some(stem) = name.strip_suffix(".lua")
            && let Ok(src) = rbx_base::vfs::read_to_string(&path)
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
    let gui = ensure_robloxgui(lua)?;
    let eng = engine(lua);
    let mut e = eng.lock();
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

fn package_bytecode(inst: &mut rbx_format::TreeInstance) -> Option<Arc<Vec<u8>>> {
    let at = inst.properties.iter().position(|(n, _)| n == "Source")?;
    let (_, value) = inst.properties.remove(at);
    let Variant::BinaryString(raw) = value else {
        return None;
    };
    match luau_bytecode::to_vm_bytecode(&raw) {
        Ok(code) => Some(Arc::new(code)),
        Err(err) => {
            tracing::warn!("core package module has unreadable bytecode: {err}");
            None
        }
    }
}

fn child_named(tree: &rbx_format::Tree, parent: usize, name: &str) -> Option<usize> {
    tree.instances[parent]
        .children
        .iter()
        .copied()
        .find(|&c| tree.instances[c].get("Name").and_then(|v| v.as_str()) == Some(name))
}

pub fn load_core_package(lua: &Lua, path: &Path) -> Result<()> {
    if with_state(lua, |s| s.core_package_scripts.is_some()) {
        return Ok(());
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("ClientAppSettings.json"))
        && let Err(err) = engine(lua).lock().flags.load_baseline_json(&text)
    {
        tracing::warn!("ClientAppSettings.json: {err}");
    }
    if let Some(dir) = path.parent() {
        let eng = engine(lua);
        let mut e = eng.lock();
        if !e.content_overlays.iter().any(|p| p == dir) {
            e.content_overlays.insert(0, dir.to_path_buf());
        }
    }
    {
        let roots = {
            let eng = engine(lua);
            let e = eng.lock();
            let mut roots = e.content_overlays.clone();
            roots.extend(e.content_root.clone());
            roots
        };
        rbx_text::set_loader(move |url: &str| {
            let rel = url.strip_prefix("rbxasset://").unwrap_or(url);
            roots.iter().find_map(|root| {
                let p = rel
                    .split(['/', '\\'])
                    .filter(|s| !s.is_empty() && *s != "..")
                    .fold(root.clone(), |p, s| p.join(s));
                rbx_base::vfs::read(&p).ok()
            })
        });
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("VERSION"))
        && let Some(version) = text.lines().nth(1)
    {
        engine(lua).lock().client_version = version.trim().to_string();
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("EngineFlagDefaults.json"))
        && let Err(err) = engine(lua).lock().flags.load_engine_defaults_json(&text)
    {
        tracing::warn!("EngineFlagDefaults.json: {err}");
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("IxpSettings.json"))
        && let Err(err) = engine(lua).lock().flags.load_ixp_values_json(&text)
    {
        tracing::warn!("IxpSettings.json: {err}");
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("AppPolicy.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
    {
        crate::api::ipc::seed_mem_item(lua, "GUAC:-1:app-policy", value.to_string());
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("OfflineFlags.json"))
        && let Err(err) = engine(lua).lock().flags.load_json(&text)
    {
        tracing::warn!("OfflineFlags.json: {err}");
    }
    if let Some(dir) = path.parent()
        && let Ok(text) = rbx_base::vfs::read_to_string(&dir.join("EngineFeatures.json"))
        && let Err(err) = engine(lua).lock().flags.load_engine_features_json(&text)
    {
        tracing::warn!("EngineFeatures.json: {err}");
    }
    let data = rbx_base::vfs::read(path).map_err(|err| {
        rt(format!(
            "Error loading core package {}: {err}",
            path.display()
        ))
    })?;
    let mut tree = rbx_format::read(&data).map_err(|err| rt(err.to_string()))?;
    let root = *tree
        .roots
        .first()
        .ok_or_else(|| rt("core package has no root"))?;
    let mut code: HashMap<usize, Arc<Vec<u8>>> = HashMap::new();
    for (i, inst) in tree.instances.iter_mut().enumerate() {
        if inst.class == "ModuleScript"
            && let Some(b) = package_bytecode(inst)
        {
            code.insert(i, b);
        }
    }
    let mut scripts: rustc_hash::FxHashMap<String, Arc<Vec<u8>>> = Default::default();
    let mut tables = Vec::new();
    if let Some(folder) = child_named(&tree, root, "CoreScripts") {
        let localization: Vec<usize> = tree.instances[folder]
            .children
            .iter()
            .copied()
            .filter(|&c| tree.instances[c].class == "LocalizationTable")
            .collect();
        if !localization.is_empty() {
            let mut sub = tree.clone();
            sub.roots = localization;
            for &r in &sub.roots {
                sub.instances[r].parent = None;
            }
            let loaded = engine(lua)
                .lock()
                .dm
                .load_tree(&sub, None, Default::default())
                .map_err(|err| rt(err.to_string()))?;
            tables = loaded.0;
        }
        for &c in &tree.instances[folder].children {
            if let (Some(b), Some(name)) = (
                code.get(&c),
                tree.instances[c].get("Name").and_then(|v| v.as_str()),
            ) {
                scripts.insert(name.to_string(), b.clone());
            }
        }
    }
    let mut bytecode: Vec<(InstanceId, Arc<Vec<u8>>)> = Vec::new();
    ensure_robloxgui(lua)?;
    if let Some(instances) = child_named(&tree, root, "DataModelInstances") {
        let services: Vec<usize> = tree.instances[instances].children.clone();
        for service_folder in services {
            let Some(service_name) = tree.instances[service_folder]
                .get("Name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            else {
                continue;
            };
            let service = engine(lua)
                .lock()
                .dm
                .get_service(&service_name)
                .map_err(|err| rt(err.to_string()))?;
            merge_patch(lua, &tree, service_folder, service, &code, &mut bytecode)?;
        }
    }
    with_state(lua, |s| {
        s.core_bytecode.extend(bytecode);
        s.localization.core_tables = tables;
        s.core_package_scripts = Some(scripts);
    });
    Ok(())
}

fn merge_patch(
    lua: &Lua,
    tree: &rbx_format::Tree,
    folder: usize,
    target: InstanceId,
    code: &HashMap<usize, Arc<Vec<u8>>>,
    bytecode: &mut Vec<(InstanceId, Arc<Vec<u8>>)>,
) -> Result<()> {
    let mut fresh = Vec::new();
    for &c in &tree.instances[folder].children {
        let inst = &tree.instances[c];
        let name = inst.get("Name").and_then(|v| v.as_str()).unwrap_or("");
        let existing = if inst.class == "Folder" {
            engine(lua).lock().dm.find_first_child(target, name, false)
        } else {
            None
        };
        match existing {
            Some(id) => merge_patch(lua, tree, c, id, code, bytecode)?,
            None => fresh.push(c),
        }
    }
    if fresh.is_empty() {
        return Ok(());
    }
    let mut sub = tree.clone();
    sub.roots = fresh;
    for &r in &sub.roots {
        sub.instances[r].parent = None;
    }
    let eng = engine(lua);
    let mut e = eng.lock();
    let (_, warnings, map) =
        e.dm.load_tree_mapped(&sub, Some(target), Default::default())
            .map_err(|err| rt(err.to_string()))?;
    for w in warnings.iter().take(20) {
        tracing::debug!("core package: {w}");
    }
    for (index, id) in map {
        if let Some(b) = code.get(&index) {
            bytecode.push((id, b.clone()));
        }
    }
    Ok(())
}

fn ensure_robloxgui(lua: &Lua) -> Result<InstanceId> {
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
            if e.core_package.is_some() {
                let _ = e.dm.set(g, "ZIndexBehavior", Variant::Enum(1));
            }
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
    Ok(gui)
}

fn fetch_success(lua: &Lua) -> Result<Value> {
    match crate::types::LEnumItem::by_name("AssetFetchStatus", "Success") {
        Some(i) => crate::types::enum_item_value(lua, i),
        None => Ok(Value::Nil),
    }
}

fn preload_async(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let callback = match args.get(1) {
        Some(Value::Function(f)) => Some(f.clone()),
        _ => None,
    };
    if let (Some(Value::Table(list)), Some(cb)) = (args.first(), callback) {
        for item in list.clone().sequence_values::<Value>() {
            let item = item?;
            let id = match &item {
                Value::String(_) => item.clone(),
                other => match crate::runtime::instance_from_value(lua, other) {
                    Some(_) => Value::String(lua.create_string("")?),
                    None => continue,
                },
            };
            crate::runtime::spawn_function(
                lua,
                cb.clone(),
                mlua::MultiValue::from_vec(vec![id, fetch_success(lua)?]),
                crate::runtime::current_info(lua),
            );
        }
    }
    none()
}

fn asset_fetch_status(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    super::ret(lua, fetch_success(lua)?)
}

fn asset_fetch_status_signal(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let signal = crate::signal::LSignal {
        target: id,
        event: crate::signal::filtered_event("AssetFetchStatusChanged", &url),
    };
    super::ret(lua, lua.create_userdata(signal)?)
}
