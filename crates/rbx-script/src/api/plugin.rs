use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_instance, arg_string};
use crate::instance::push_instance;
use crate::runtime::{engine, fire_lua};
use crate::types::rt;
use mlua::{Lua, MultiValue, RegistryKey, Result, Value};
use rbx_dom::InstanceId;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct PluginButton {
    pub id: InstanceId,
    pub toolbar: String,
    pub text: String,
    pub tooltip: String,
    pub icon: Option<PathBuf>,
    pub active: bool,
    pub builtin: bool,
}

#[derive(Default)]
pub struct PluginState {
    pub manager: Option<InstanceId>,
    pub plugins: Vec<(InstanceId, Option<PathBuf>, bool)>,
    pub toolbars: Vec<(InstanceId, String)>,
    pub buttons: Vec<PluginButton>,
    pub active: Option<InstanceId>,
    pub mice: HashMap<InstanceId, InstanceId>,
    pub settings: HashMap<String, RegistryKey>,
    pub open_requests: Vec<InstanceId>,
    pub current_dir: Option<PathBuf>,
    pub loading_builtin: bool,
    pub model_plugins: HashMap<InstanceId, InstanceId>,
}

impl PluginState {
    pub fn active_mouse(&self) -> Option<InstanceId> {
        self.active.and_then(|p| self.mice.get(&p).copied())
    }

    fn dir_of(&self, plugin: InstanceId) -> Option<PathBuf> {
        self.plugins
            .iter()
            .find(|(p, _, _)| *p == plugin)
            .and_then(|(_, d, _)| d.clone())
    }
}

pub fn manager(lua: &Lua) -> Result<InstanceId> {
    let eng = engine(lua);
    let mut e = eng.lock();
    if let Some(m) = e.plugins.manager.filter(|m| e.dm.contains(*m)) {
        return Ok(m);
    }
    let m =
        e.dm.create_internal("PluginManager")
            .map_err(|err| rt(err.to_string()))?;
    e.plugins.manager = Some(m);
    Ok(m)
}

fn create_plugin(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let id =
            e.dm.create_internal("Plugin")
                .map_err(|err| rt(err.to_string()))?;
        let dir = e.plugins.current_dir.clone();
        let builtin = e.plugins.loading_builtin;
        e.plugins.plugins.push((id, dir, builtin));
        id
    };
    ret(lua, push_instance(lua, id)?)
}

pub fn create_model_plugin(lua: &Lua, roots: &[InstanceId]) -> Result<InstanceId> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let id =
        e.dm.create_internal("Plugin")
            .map_err(|err| rt(err.to_string()))?;
    let dir = e.plugins.current_dir.clone();
    let builtin = e.plugins.loading_builtin;
    e.plugins.plugins.push((id, dir, builtin));
    for r in roots {
        e.plugins.model_plugins.insert(*r, id);
    }
    Ok(id)
}

pub fn plugin_for_script(lua: &Lua, script: InstanceId) -> Option<InstanceId> {
    let eng = engine(lua);
    let e = eng.lock();
    if e.plugins.model_plugins.is_empty() {
        return None;
    }
    let mut cur = Some(script);
    while let Some(c) = cur {
        if let Some(p) = e.plugins.model_plugins.get(&c) {
            return Some(*p);
        }
        cur = e.dm.parent_of(c);
    }
    None
}

fn create_toolbar(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        match e.plugins.toolbars.iter().find(|(_, n)| *n == name) {
            Some((t, _)) => *t,
            None => {
                let t =
                    e.dm.create_internal("Toolbar")
                        .map_err(|err| rt(err.to_string()))?;
                e.dm.set_name(t, &name);
                e.plugins.toolbars.push((t, name));
                t
            }
        }
    };
    ret(lua, push_instance(lua, id)?)
}

fn create_button(lua: &Lua, toolbar: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let tooltip = arg_string(args.get(1)).unwrap_or_default();
    let icon = arg_string(args.get(2)).filter(|s| !s.is_empty());
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let toolbar_name = e
            .plugins
            .toolbars
            .iter()
            .find(|(t, _)| *t == toolbar)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let b =
            e.dm.create_internal("Button")
                .map_err(|err| rt(err.to_string()))?;
        e.dm.set_name(b, if text.is_empty() { &tooltip } else { &text });
        let dir = e.plugins.current_dir.clone();
        let content = e.content_root.clone();
        let icon = icon.map(|i| {
            if let Some(rest) = i.strip_prefix("rbxasset://") {
                content.clone().unwrap_or_default().join(rest)
            } else {
                match &dir {
                    Some(d) => d.join(i),
                    None => PathBuf::from(i),
                }
            }
        });
        let builtin = e.plugins.loading_builtin;
        e.plugins.buttons.push(PluginButton {
            id: b,
            toolbar: toolbar_name,
            text,
            tooltip,
            icon,
            active: false,
            builtin,
        });
        b
    };
    ret(lua, push_instance(lua, id)?)
}

fn set_active(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let on = arg_bool(args.first());
    let eng = engine(lua);
    let mut e = eng.lock();
    if let Some(b) = e.plugins.buttons.iter_mut().find(|b| b.id == id) {
        b.active = on;
    }
    none()
}

fn activate(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let previous = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let prev = e.plugins.active.filter(|p| *p != id);
        e.plugins.active = Some(id);
        prev
    };
    if let Some(p) = previous {
        fire_lua(lua, p, "Deactivation", MultiValue::new());
    }
    none()
}

fn get_mouse(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let mouse = {
        let eng = engine(lua);
        let mut e = eng.lock();
        match e
            .plugins
            .mice
            .get(&id)
            .copied()
            .filter(|m| e.dm.contains(*m))
        {
            Some(m) => m,
            None => {
                let m =
                    e.dm.create_internal("PluginMouse")
                        .map_err(|err| rt(err.to_string()))?;
                e.plugins.mice.insert(id, m);
                m
            }
        }
    };
    ret(lua, push_instance(lua, mouse)?)
}

fn get_setting(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).unwrap_or_default();
    let value = {
        let eng = engine(lua);
        let e = eng.lock();
        match e.plugins.settings.get(&key) {
            Some(k) => lua.registry_value::<Value>(k)?,
            None => Value::Nil,
        }
    };
    ret(lua, value)
}

fn set_setting(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).unwrap_or_default();
    let value = args.get(1).cloned().unwrap_or(Value::Nil);
    let stored = lua.create_registry_value(value)?;
    let old = engine(lua).lock().plugins.settings.insert(key, stored);
    if let Some(old) = old {
        lua.remove_registry_value(old)?;
    }
    none()
}

fn open_script(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(script) = arg_instance(lua, args.first()) else {
        return Err(rt("OpenScript expects a script"));
    };
    engine(lua).lock().plugins.open_requests.push(script);
    none()
}

fn studio_user_id(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0i64)
}

fn join_mode(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0i64)
}

fn csg_args(lua: &Lua, args: &[Value]) -> Vec<InstanceId> {
    match args.first() {
        Some(Value::Table(t)) => t
            .sequence_values::<Value>()
            .filter_map(|v| v.ok())
            .filter_map(|v| crate::runtime::instance_from_value(lua, &v))
            .collect(),
        other => arg_instance(lua, other).into_iter().collect(),
    }
}

fn instance_table(lua: &Lua, ids: &[InstanceId]) -> Result<Value> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, v) in crate::instance::push_instances(lua, ids)?
        .into_iter()
        .enumerate()
    {
        t.raw_set(i + 1, v)?;
    }
    Ok(Value::Table(t))
}

fn union(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let parts = csg_args(lua, args);
    let r = crate::csg::union(&mut engine(lua).lock().dm, &parts);
    let op = r.map_err(rt)?;
    ret(lua, push_instance(lua, op)?)
}

fn negate(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let parts = csg_args(lua, args);
    let r = crate::csg::negate(&mut engine(lua).lock().dm, &parts);
    let ops = r.map_err(rt)?;
    ret(lua, instance_table(lua, &ops)?)
}

fn separate(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let parts = csg_args(lua, args);
    let r = crate::csg::separate(&mut engine(lua).lock().dm, &parts);
    let restored = r.map_err(rt)?;
    ret(lua, instance_table(lua, &restored)?)
}

fn ignore(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

pub fn plugin_dir(lua: &Lua, plugin: InstanceId) -> Option<PathBuf> {
    engine(lua).lock().plugins.dir_of(plugin)
}

pub fn register(r: &mut Registry) {
    r.add("PluginManager", &["CreatePlugin"], create_plugin);
    r.add("PluginManager", &["ExportPlace", "ExportSelection"], ignore);
    r.add("Plugin", &["CreateToolbar"], create_toolbar);
    r.add("Plugin", &["Activate"], activate);
    r.add("Plugin", &["GetMouse"], get_mouse);
    r.add("Plugin", &["GetSetting"], get_setting);
    r.add("Plugin", &["SetSetting"], set_setting);
    r.add("Plugin", &["OpenScript"], open_script);
    r.add("Plugin", &["OpenWikiPage", "SaveSelectedToRoblox"], ignore);
    r.add("Plugin", &["GetStudioUserId"], studio_user_id);
    r.add("Plugin", &["GetJoinMode"], join_mode);
    r.add("Plugin", &["Union"], union);
    r.add("Plugin", &["Negate"], negate);
    r.add("Plugin", &["Separate"], separate);
    r.add("Toolbar", &["CreateButton"], create_button);
    r.add("Button", &["SetActive"], set_active);
}
