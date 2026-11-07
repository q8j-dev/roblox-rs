use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, arg_string};
use crate::runtime::{MessageType, engine, expect_instance};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

fn debris_max_items(e: &crate::Engine, id: InstanceId) -> usize {
    e.dm.get(id, "MaxItems")
        .ok()
        .and_then(|v| v.as_i32())
        .unwrap_or(1000)
        .max(0) as usize
}

pub fn debris_cleanup(e: &mut crate::Engine, debris: InstanceId) {
    let max = debris_max_items(e, debris);
    while e.debris_queue.len() > max {
        let id = e.debris_queue.remove(0);
        if e.dm.contains(id)
            && let Err(err) = e.dm.destroy(id)
        {
            e.print(MessageType::Warning, err.to_string());
        }
    }
}

fn add_item(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let item = match args.first() {
        None | Some(Value::Nil) => return none(),
        Some(v) => expect_instance(lua, v, "Instance")?,
    };
    let lifetime = arg_number(args.get(1)).unwrap_or(10.0);
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.instance(item).is_some_and(|i| i.roblox_locked)
        && !crate::runtime::current_identity(lua).allows(rbx_reflection::Security::RobloxScript)
    {
        return Err(rt(
            "The current identity cannot access a RobloxLocked object",
        ));
    }
    let due = e.game_time + lifetime;
    e.debris_timers.push((due, item));
    e.debris_queue.push(item);
    debris_cleanup(&mut e, id);
    none()
}

fn set_legacy_max_items(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    engine(lua).lock().debris_legacy_max_items = arg_bool(args.first());
    none()
}

fn get_fflag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    match rbx_base::flag_value(&name) {
        Some(v) => ret(lua, v.eq_ignore_ascii_case("true")),
        None => Err(rt(format!("Flag {name} does not exist"))),
    }
}

fn get_fvariable(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    match rbx_base::flag_value(&name) {
        Some(v) => ret(lua, v),
        None => Err(rt(format!("Variable {name} does not exist"))),
    }
}

fn get_fvariables(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let t = lua.create_table()?;
    for (k, v) in rbx_base::flags::all_values() {
        t.set(k, v)?;
    }
    ret(lua, t)
}

pub fn register(r: &mut Registry) {
    r.add("Debris", &["AddItem", "addItem"], add_item);
    r.add("Debris", &["SetLegacyMaxItems"], set_legacy_max_items);
    r.add("GlobalSettings", &["GetFFlag"], get_fflag);
    r.add("GlobalSettings", &["GetFVariable"], get_fvariable);
    r.add("GlobalSettings", &["GetFVariables"], get_fvariables);
}
