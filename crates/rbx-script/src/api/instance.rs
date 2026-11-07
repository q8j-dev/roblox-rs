use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_string};
use crate::instance::push_instance;
use crate::runtime::{engine, expect_instance, with_state, yield_sentinel};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

fn children_table(lua: &Lua, ids: &[InstanceId]) -> Result<mlua::Table> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, &c) in ids.iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, c)?)?;
    }
    Ok(t)
}

fn find_first_child(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let recursive = arg_bool(args.get(1));
    let found = engine(lua).lock().dm.find_first_child(id, &name, recursive);
    match found {
        Some(c) => ret(lua, push_instance(lua, c)?),
        None => ret(lua, Value::Nil),
    }
}

fn get_children(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let ids = engine(lua).lock().dm.children_of(id).to_vec();
    ret(lua, children_table(lua, &ids)?)
}

fn clone(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().dm.clone_instance(id);
    match r.map_err(|e| rt(e.to_string()))? {
        Some(c) => ret(lua, push_instance(lua, c)?),
        None => ret(lua, Value::Nil),
    }
}

fn destroy(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().dm.destroy(id);
    r.map_err(|e| rt(e.to_string()))?;
    none()
}

fn remove(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().dm.remove(id);
    r.map_err(|e| rt(e.to_string()))?;
    none()
}

fn clear_all_children(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().dm.destroy_all_children(id);
    r.map_err(|e| rt(e.to_string()))?;
    none()
}

fn is_a(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let r = engine(lua).lock().dm.is_a(id, &name);
    ret(lua, r)
}

fn is_ancestor_of(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let other = match args.first() {
        Some(Value::Nil) | None => return ret(lua, false),
        Some(v) => expect_instance(lua, v, "Object")?,
    };
    let r = engine(lua).lock().dm.is_ancestor_of(id, Some(other));
    ret(lua, r)
}

fn is_descendant_of(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let other = match args.first() {
        Some(Value::Nil) | None => return ret(lua, false),
        Some(v) => expect_instance(lua, v, "Object")?,
    };
    let r = engine(lua).lock().dm.is_descendant_of(id, other);
    ret(lua, r)
}

fn get_full_name(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().dm.full_name(id);
    ret(lua, r)
}

fn get_debug_id(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let scope = crate::convert::arg_number(args.first())
        .unwrap_or(4.0)
        .max(0.0) as usize;
    let hex = format!("{:08X}", id.raw());
    let tail = &hex[hex.len().saturating_sub(scope)..];
    ret(lua, format!("{}_{}", tail, id.raw()))
}

fn wait_for_child(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    if name.is_empty() {
        return Err(rt("WaitForChild called with an empty child name."));
    }
    let found = engine(lua).lock().dm.find_first_child(id, &name, false);
    if let Some(c) = found {
        return ret(lua, push_instance(lua, c)?);
    }
    let thread = lua.current_thread();
    with_state(lua, |s| {
        s.child_waiters.push(crate::runtime::ChildWaiter {
            parent: id,
            name,
            thread,
        })
    });
    let _ = yield_sentinel(lua)?;
    Ok(Ret::Yield)
}

pub fn register(r: &mut Registry) {
    r.add(
        "Instance",
        &["FindFirstChild", "findFirstChild"],
        find_first_child,
    );
    r.add(
        "Instance",
        &["GetChildren", "getChildren", "children"],
        get_children,
    );
    r.add("Instance", &["Clone", "clone"], clone);
    r.add("Instance", &["Destroy", "destroy"], destroy);
    r.add("Instance", &["Remove", "remove"], remove);
    r.add("Instance", &["ClearAllChildren"], clear_all_children);
    r.add("Instance", &["IsA", "isA"], is_a);
    r.add("Instance", &["IsAncestorOf"], is_ancestor_of);
    r.add(
        "Instance",
        &["IsDescendantOf", "isDescendantOf"],
        is_descendant_of,
    );
    r.add("Instance", &["GetFullName"], get_full_name);
    r.add("Instance", &["GetDebugId"], get_debug_id);
    r.add("Instance", &["WaitForChild"], wait_for_child);
}
