use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_string};
use crate::instance::push_instance;
use crate::runtime::{ThreadInfo, current_info, engine, spawn_function, with_state};
use crate::types::rt;
use mlua::{Function, Lua, MultiValue, RegistryKey, Result, Value};
use rbx_dom::InstanceId;
use std::collections::HashMap;

struct Subscriber {
    connection: InstanceId,
    func: RegistryKey,
    once: bool,
    info: ThreadInfo,
}

#[derive(Default)]
pub struct IpcState {
    bus: HashMap<String, Vec<Subscriber>>,
    bus_last: HashMap<String, RegistryKey>,
    mem_items: HashMap<String, String>,
    mem_binds: HashMap<String, Vec<Subscriber>>,
    local_items: HashMap<String, String>,
    local_loaded: bool,
}

pub fn register(r: &mut Registry) {
    r.add("MessageBusService", &["GetMessageId"], message_id);
    r.add(
        "MessageBusService",
        &["GetProtocolMethodRequestMessageId"],
        request_id,
    );
    r.add(
        "MessageBusService",
        &["GetProtocolMethodResponseMessageId"],
        response_id,
    );
    r.add("MessageBusService", &["Publish"], publish);
    r.add("MessageBusService", &["Subscribe"], subscribe);
    r.add(
        "MessageBusService",
        &["SubscribeToProtocolMethodRequest"],
        subscribe_request,
    );
    r.add(
        "MessageBusService",
        &["SubscribeToProtocolMethodResponse"],
        subscribe_response,
    );
    r.add(
        "MessageBusService",
        &["PublishProtocolMethodRequest"],
        publish_request,
    );
    r.add(
        "MessageBusService",
        &["PublishProtocolMethodResponse"],
        publish_response,
    );
    r.add("MessageBusService", &["MakeRequest"], make_request);
    r.add("MessageBusService", &["GetLast"], get_last);
    r.add("MessageBusService", &["Call"], no_handler);
    r.add("MemStorageService", &["Call"], no_handler);
    r.add("MessageBusConnection", &["Disconnect"], disconnect);
    r.add("MemStorageConnection", &["Disconnect"], disconnect);
    r.add("MemStorageService", &["GetItem"], mem_get);
    r.add("MemStorageService", &["SetItem"], mem_set);
    r.add("MemStorageService", &["HasItem"], mem_has);
    r.add("MemStorageService", &["RemoveItem"], mem_remove);
    r.add("MemStorageService", &["Fire"], mem_fire);
    r.add("MemStorageService", &["BindAndFire"], mem_bind_and_fire);
    r.add("LocalStorageService", &["GetItem"], local_get);
    r.add("LocalStorageService", &["SetItem"], local_set);
    r.add("LocalStorageService", &["Flush"], local_flush);
    r.add("LocalStorageService", &["WhenLoaded"], local_when_loaded);
}

fn string_arg(args: &[Value], i: usize) -> Result<String> {
    arg_string(args.get(i)).ok_or_else(|| rt(format!("Argument {} missing or nil", i + 1)))
}

fn function_arg(args: &[Value], i: usize) -> Result<Function> {
    match args.get(i) {
        Some(Value::Function(f)) => Ok(f.clone()),
        _ => Err(rt(format!("Argument {} missing or nil", i + 1))),
    }
}

fn message_id(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    ret(
        lua,
        format!("{}.{}", string_arg(args, 0)?, string_arg(args, 1)?),
    )
}

fn request_id(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    ret(
        lua,
        format!("{}.{}.request", string_arg(args, 0)?, string_arg(args, 1)?),
    )
}

fn response_id(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    ret(
        lua,
        format!("{}.{}.response", string_arg(args, 0)?, string_arg(args, 1)?),
    )
}

fn new_connection(lua: &Lua, class: &str) -> Result<InstanceId> {
    engine(lua)
        .lock()
        .dm
        .create_internal(class)
        .map_err(|e| rt(e.to_string()))
}

fn deliver(lua: &Lua, mid: &str, payload: Value) -> Result<()> {
    let key = lua.create_registry_value(payload.clone())?;
    let targets: Vec<(Function, ThreadInfo)> = with_state(lua, |s| {
        if let Some(old) = s.ipc.bus_last.insert(mid.to_string(), key) {
            let _ = lua.remove_registry_value(old);
        }
        let mut out = Vec::new();
        if let Some(list) = s.ipc.bus.get_mut(mid) {
            for sub in list.iter() {
                if let Ok(f) = lua.registry_value::<Function>(&sub.func) {
                    out.push((f, sub.info.clone()));
                }
            }
            list.retain(|sub| !sub.once);
        }
        out
    });
    for (f, info) in targets {
        spawn_function(lua, f, MultiValue::from_vec(vec![payload.clone()]), info);
    }
    Ok(())
}

fn add_subscriber(lua: &Lua, mid: String, func: Function, once: bool, sticky: bool) -> Result<Ret> {
    let connection = new_connection(lua, "MessageBusConnection")?;
    let info = current_info(lua);
    let key = lua.create_registry_value(func.clone())?;
    let last = with_state(lua, |s| {
        s.ipc.bus.entry(mid.clone()).or_default().push(Subscriber {
            connection,
            func: key,
            once,
            info: info.clone(),
        });
        s.ipc
            .bus_last
            .get(&mid)
            .and_then(|k| lua.registry_value::<Value>(k).ok())
    });
    if sticky && let Some(v) = last {
        if once {
            with_state(lua, |s| {
                if let Some(list) = s.ipc.bus.get_mut(&mid) {
                    list.retain(|sub| sub.connection != connection);
                }
            });
        }
        spawn_function(lua, func, MultiValue::from_vec(vec![v]), info);
    }
    ret(lua, push_instance(lua, connection)?)
}

fn publish(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = string_arg(args, 0)?;
    deliver(lua, &mid, args.get(1).cloned().unwrap_or(Value::Nil))?;
    none()
}

fn subscribe(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = string_arg(args, 0)?;
    let func = function_arg(args, 1)?;
    add_subscriber(lua, mid, func, arg_bool(args.get(2)), arg_bool(args.get(3)))
}

fn subscribe_request(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = format!("{}.{}.request", string_arg(args, 0)?, string_arg(args, 1)?);
    let func = function_arg(args, 2)?;
    add_subscriber(lua, mid, func, arg_bool(args.get(3)), arg_bool(args.get(4)))
}

fn subscribe_response(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = format!("{}.{}.response", string_arg(args, 0)?, string_arg(args, 1)?);
    let func = function_arg(args, 2)?;
    add_subscriber(lua, mid, func, arg_bool(args.get(3)), arg_bool(args.get(4)))
}

fn publish_request(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = format!("{}.{}.request", string_arg(args, 0)?, string_arg(args, 1)?);
    deliver(lua, &mid, args.get(2).cloned().unwrap_or(Value::Nil))?;
    none()
}

fn publish_response(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = format!("{}.{}.response", string_arg(args, 0)?, string_arg(args, 1)?);
    deliver(lua, &mid, args.get(2).cloned().unwrap_or(Value::Nil))?;
    none()
}

fn make_request(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let protocol = string_arg(args, 0)?;
    let method = string_arg(args, 1)?;
    let message = args.get(2).cloned().unwrap_or(Value::Nil);
    let callback = function_arg(args, 3)?;
    let mid = format!("{protocol}.{method}.response");
    add_subscriber(lua, mid, callback, true, false)?;
    publish_request(
        lua,
        id,
        &[
            Value::String(lua.create_string(&protocol)?),
            Value::String(lua.create_string(&method)?),
            message,
        ],
    )
}

fn get_last(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mid = string_arg(args, 0)?;
    let v = with_state(lua, |s| {
        s.ipc
            .bus_last
            .get(&mid)
            .and_then(|k| lua.registry_value::<Value>(k).ok())
    });
    ret(lua, v.unwrap_or(Value::Nil))
}

fn disconnect(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    with_state(lua, |s| {
        for list in s.ipc.bus.values_mut().chain(s.ipc.mem_binds.values_mut()) {
            list.retain(|sub| sub.connection != id);
        }
    });
    none()
}

fn mem_get(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = string_arg(args, 0)?;
    let v = with_state(lua, |s| s.ipc.mem_items.get(&key).cloned());
    ret(
        lua,
        v.or_else(|| arg_string(args.get(1))).unwrap_or_default(),
    )
}

fn fire_mem(lua: &Lua, key: &str, value: &str) -> Result<()> {
    let targets: Vec<(Function, ThreadInfo)> = with_state(lua, |s| {
        s.ipc
            .mem_binds
            .get(key)
            .map(|list| {
                list.iter()
                    .filter_map(|sub| {
                        lua.registry_value::<Function>(&sub.func)
                            .ok()
                            .map(|f| (f, sub.info.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    for (f, info) in targets {
        let arg = Value::String(lua.create_string(value)?);
        spawn_function(lua, f, MultiValue::from_vec(vec![arg]), info);
    }
    Ok(())
}

fn mem_set(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = string_arg(args, 0)?;
    let value = arg_string(args.get(1)).unwrap_or_default();
    with_state(lua, |s| s.ipc.mem_items.insert(key.clone(), value.clone()));
    fire_mem(lua, &key, &value)?;
    none()
}

fn mem_has(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = string_arg(args, 0)?;
    let has = with_state(lua, |s| s.ipc.mem_items.contains_key(&key));
    ret(lua, has)
}

fn mem_remove(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = string_arg(args, 0)?;
    let had = with_state(lua, |s| s.ipc.mem_items.remove(&key).is_some());
    ret(lua, had)
}

fn mem_fire(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = string_arg(args, 0)?;
    let value = arg_string(args.get(1)).unwrap_or_default();
    fire_mem(lua, &key, &value)?;
    none()
}

fn bind(lua: &Lua, args: &[Value]) -> Result<(InstanceId, String, Function)> {
    let key = string_arg(args, 0)?;
    let func = function_arg(args, 1)?;
    let connection = new_connection(lua, "MemStorageConnection")?;
    let reg = lua.create_registry_value(func.clone())?;
    let info = current_info(lua);
    with_state(lua, |s| {
        s.ipc
            .mem_binds
            .entry(key.clone())
            .or_default()
            .push(Subscriber {
                connection,
                func: reg,
                once: false,
                info,
            })
    });
    Ok((connection, key, func))
}

fn mem_bind_and_fire(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let (connection, key, func) = bind(lua, args)?;
    let current = with_state(lua, |s| s.ipc.mem_items.get(&key).cloned());
    if let Some(v) = current {
        let arg = Value::String(lua.create_string(&v)?);
        spawn_function(
            lua,
            func,
            MultiValue::from_vec(vec![arg]),
            current_info(lua),
        );
    }
    ret(lua, push_instance(lua, connection)?)
}

pub fn seed_mem_item(lua: &Lua, key: &str, value: String) {
    with_state(lua, |s| {
        s.ipc.mem_items.entry(key.to_string()).or_insert(value);
    });
}

fn storage_path(lua: &Lua) -> Option<std::path::PathBuf> {
    engine(lua).lock().app_storage_path.clone()
}

fn load_local(lua: &Lua) {
    if with_state(lua, |s| std::mem::replace(&mut s.ipc.local_loaded, true)) {
        return;
    }
    let Some(path) = storage_path(lua) else {
        return;
    };
    let Ok(text) = rbx_base::vfs::read_to_string(&path) else {
        return;
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return;
    };
    with_state(lua, |s| {
        for (k, v) in map {
            let v = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            s.ipc.local_items.entry(k).or_insert(v);
        }
    });
}

fn save_local(lua: &Lua) {
    let Some(path) = storage_path(lua) else {
        return;
    };
    let map: serde_json::Map<String, serde_json::Value> = with_state(lua, |s| {
        s.ipc
            .local_items
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect()
    });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::Value::Object(map).to_string());
}

fn local_get(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_local(lua);
    let key = string_arg(args, 0)?;
    let v = with_state(lua, |s| s.ipc.local_items.get(&key).cloned());
    ret(lua, v.unwrap_or_default())
}

fn local_set(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_local(lua);
    let key = string_arg(args, 0)?;
    let value = arg_string(args.get(1)).unwrap_or_default();
    with_state(lua, |s| {
        s.ipc.local_items.insert(key.clone(), value.clone())
    });
    engine(lua).lock().dm.fire_signal(
        id,
        "ItemWasSet",
        vec![
            rbx_dom::Variant::String(key),
            rbx_dom::Variant::String(value),
        ],
    );
    none()
}

fn local_flush(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    load_local(lua);
    save_local(lua);
    none()
}

fn local_when_loaded(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let func = function_arg(args, 0)?;
    spawn_function(lua, func, MultiValue::new(), current_info(lua));
    none()
}

fn no_handler(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}
