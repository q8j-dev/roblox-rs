use super::{Registry, Ret, ret};
use crate::convert::arg_string;
use crate::instance::push_instance;
use crate::runtime::{engine, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, database, is_service_class};

pub fn settings_root(lua: &Lua, class: &str) -> Result<InstanceId> {
    let key = format!("rbx_settings_{class}");
    if let Ok(Value::Number(n)) = lua.named_registry_value::<Value>(&key)
        && let Some(id) = InstanceId::from_raw(n as u64)
        && engine(lua).lock().dm.contains(id)
    {
        return Ok(id);
    }
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let id = e.dm.create_internal(class).map_err(|e| rt(e.to_string()))?;
        e.dm.retain(id);
        if class == "GlobalSettings" {
            for (item, name) in [
                ("LuaSettings", "Lua"),
                ("PhysicsSettings", "Physics"),
                ("RenderSettings", "Rendering"),
                ("NetworkSettings", "Network"),
                ("DebugSettings", "Diagnostics"),
                ("GameSettings", "Game Options"),
                ("TaskScheduler", "Task Scheduler"),
            ] {
                if let Ok(child) = e.dm.create_internal(item) {
                    e.dm.set_name(child, name);
                    let _ = e.dm.set_parent(child, Some(id));
                }
            }
        }
        id
    };
    lua.set_named_registry_value(&key, id.raw() as f64)?;
    Ok(id)
}

fn get_service(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = arg_string(args.first()).unwrap_or_default();
    let eng = engine(lua);
    let found = {
        let mut e = eng.lock();
        let exists = database().class(&class).is_some();
        if !exists {
            return Err(rt(format!("'{class}' is not a valid Service name")));
        }
        let child =
            e.dm.children_of(id)
                .iter()
                .copied()
                .find(|&c| e.dm.class_of(c) == Some(class.as_str()));
        match child {
            Some(c) => Some(c),
            None => {
                if !is_service_class(&class) {
                    return Err(rt(format!("'{class}' is not a valid Service name")));
                }
                let c =
                    e.dm.create_internal(&class)
                        .map_err(|e| rt(e.to_string()))?;
                e.dm.set_parent(c, Some(id))
                    .map_err(|e| rt(e.to_string()))?;
                e.dm.lock_parent(c, true);
                Some(c)
            }
        }
    };
    match found {
        Some(c) => ret(lua, push_instance(lua, c)?),
        None => ret(lua, Value::Nil),
    }
}

fn find_service(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = arg_string(args.first()).unwrap_or_default();
    let found = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.children_of(id)
            .iter()
            .copied()
            .find(|&c| e.dm.class_of(c) == Some(class.as_str()))
    };
    match found {
        Some(c) => ret(lua, push_instance(lua, c)?),
        None => ret(lua, Value::Nil),
    }
}

fn set_timeout(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let seconds = crate::convert::arg_number(args.first()).unwrap_or(0.0);
    crate::runtime::set_script_timeout(lua, seconds);
    super::none()
}

fn run_state(lua: &Lua, running: bool) -> Result<Ret> {
    let was_stopped = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let was = e.stopped;
        e.running = running;
        if running {
            e.stopped = false;
        }
        was
    };
    if running && was_stopped {
        crate::scripts::scan_all(lua);
    }
    Ok(Ret::Values(mlua::MultiValue::new()))
}

fn stop_runtime(lua: &Lua) -> Result<Ret> {
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.running = false;
        e.stopped = true;
    }
    let running: Vec<InstanceId> = with_state(lua, |s| {
        s.running_scripts
            .iter()
            .filter(|(id, on)| **on && !s.test_scripts.contains(id))
            .map(|(id, _)| *id)
            .collect()
    });
    for id in running {
        let core = engine(lua).lock().dm.class_of(id) == Some("CoreScript");
        if !core {
            crate::scripts::stop_script(lua, id);
        }
    }
    Ok(Ret::Values(mlua::MultiValue::new()))
}

fn mode_flags(lua: &Lua) -> (bool, bool) {
    let e = engine(lua);
    let e = e.lock();
    let server = e.dm.find_service("NetworkServer").is_some();
    let client = e.dm.find_service("NetworkClient").is_some();
    match e.mode {
        crate::runtime::RunMode::Server => (true, false),
        crate::runtime::RunMode::Client => (false, true),
        crate::runtime::RunMode::Standalone => (server || !client, client || !server),
    }
}

fn bind_render_step(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let priority = crate::convert::arg_number(args.get(1))
        .ok_or_else(|| rt("Argument 2 missing or nil"))? as i32;
    let f = match args.get(2) {
        Some(Value::Function(f)) => f.clone(),
        _ => return Err(rt("Argument 3 missing or nil")),
    };
    let key = lua.create_registry_value(f)?;
    let e = engine(lua);
    let mut e = e.lock();
    e.render_bindings.retain(|b| b.0 != name);
    e.render_bindings.push((name, priority, key));
    e.render_bindings.sort_by_key(|b| b.1);
    Ok(Ret::Values(mlua::MultiValue::new()))
}

fn unbind_render_step(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let e = engine(lua);
    let mut e = e.lock();
    e.render_bindings.retain(|b| b.0 != name);
    Ok(Ret::Values(mlua::MultiValue::new()))
}

fn history_step(lua: &Lua, id: InstanceId, undo: bool) -> Result<Ret> {
    let name = {
        let e = engine(lua);
        let mut e = e.lock();
        if undo {
            e.dm.history_undo()
        } else {
            e.dm.history_redo()
        }
    };
    if let Some(n) = name {
        let s = lua.create_string(&n)?;
        crate::runtime::fire_lua(
            lua,
            id,
            if undo { "OnUndo" } else { "OnRedo" },
            mlua::MultiValue::from_vec(vec![Value::String(s)]),
        );
    }
    Ok(Ret::Values(mlua::MultiValue::new()))
}

fn history_can(lua: &Lua, undo: bool) -> Result<Ret> {
    let name = {
        let e = engine(lua);
        let e = e.lock();
        if undo {
            e.dm.history_can_undo()
        } else {
            e.dm.history_can_redo()
        }
    };
    match name {
        Some(n) => ret(lua, (true, n)),
        None => ret(lua, false),
    }
}

pub fn register(r: &mut Registry) {
    r.add("ChangeHistoryService", &["SetEnabled"], |lua, _, args| {
        let on = crate::convert::arg_bool(args.first());
        engine(lua).lock().dm.history_set_enabled(on);
        Ok(Ret::Values(mlua::MultiValue::new()))
    });
    r.add("ChangeHistoryService", &["SetWaypoint"], |lua, _, args| {
        let name = arg_string(args.first()).unwrap_or_default();
        engine(lua).lock().dm.history_waypoint(&name);
        Ok(Ret::Values(mlua::MultiValue::new()))
    });
    r.add("ChangeHistoryService", &["ResetWaypoints"], |lua, _, _| {
        engine(lua).lock().dm.history_reset();
        Ok(Ret::Values(mlua::MultiValue::new()))
    });
    r.add("ChangeHistoryService", &["Undo"], |lua, id, _| {
        history_step(lua, id, true)
    });
    r.add("ChangeHistoryService", &["Redo"], |lua, id, _| {
        history_step(lua, id, false)
    });
    r.add("ChangeHistoryService", &["GetCanUndo"], |lua, _, _| {
        history_can(lua, true)
    });
    r.add("ChangeHistoryService", &["GetCanRedo"], |lua, _, _| {
        history_can(lua, false)
    });
    r.add("RunService", &["Run"], |lua, _, _| run_state(lua, true));
    r.add("RunService", &["Pause"], |lua, _, _| run_state(lua, false));
    r.add("RunService", &["Stop", "Reset"], |lua, _, _| {
        stop_runtime(lua)
    });
    r.add("RunService", &["IsRunning"], |lua, _, _| {
        let running = engine(lua).lock().running;
        ret(lua, running)
    });
    r.add("RunService", &["IsServer"], |lua, _, _| {
        ret(lua, mode_flags(lua).0)
    });
    r.add("RunService", &["IsClient"], |lua, _, _| {
        ret(lua, mode_flags(lua).1)
    });
    r.add("RunService", &["IsStudio", "IsRunMode"], |lua, _, _| {
        ret(lua, false)
    });
    r.add("RunService", &["BindToRenderStep"], bind_render_step);
    r.add("RunService", &["UnbindFromRenderStep"], unbind_render_step);
    r.add("ScriptContext", &["SetTimeout"], set_timeout);
    r.add(
        "ServiceProvider",
        &["GetService", "getService", "service"],
        get_service,
    );
    r.add("ServiceProvider", &["FindService"], find_service);
    r.add(
        "GlobalSettings",
        &["GetService", "getService", "service"],
        get_service,
    );
    r.add(
        "UserSettings",
        &["GetService", "getService", "service"],
        get_service,
    );
}
