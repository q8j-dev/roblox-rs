use super::{Registry, Ret, ret};
use crate::convert::arg_string;
use crate::instance::push_instance;
use crate::runtime::engine;
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

pub fn register(r: &mut Registry) {
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
