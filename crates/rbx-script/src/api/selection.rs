use super::{Registry, Ret, none, ret};
use crate::instance::push_instance;
use crate::runtime::{engine, fire_lua};
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{DataModel, InstanceId};

pub fn current(lua: &Lua) -> Vec<InstanceId> {
    let eng = engine(lua);
    let e = eng.lock();
    e.selection
        .iter()
        .copied()
        .filter(|id| e.dm.contains(*id))
        .collect()
}

pub fn set(lua: &Lua, items: Vec<InstanceId>) {
    let mut unique = Vec::new();
    for id in items {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    let (changed, service) = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let changed = e.selection != unique;
        e.selection = unique;
        (changed, e.dm.find_service("Selection"))
    };
    if changed && let Some(s) = service {
        fire_lua(lua, s, "SelectionChanged", MultiValue::new());
    }
}

pub fn is_selectable_3d(dm: &DataModel, id: InstanceId) -> bool {
    (dm.is_a(id, "BasePart") && !dm.is_a(id, "Terrain")) || dm.is_a(id, "Model")
}

pub fn top_selectable_3d(dm: &DataModel, part: InstanceId) -> Option<InstanceId> {
    if !is_selectable_3d(dm, part) {
        return None;
    }
    let ws = dm.find_service("Workspace");
    let mut answer = part;
    let mut cur = dm.parent_of(part);
    while let Some(p) = cur {
        if Some(p) == ws {
            break;
        }
        if is_selectable_3d(dm, p) {
            answer = p;
        }
        cur = dm.parent_of(p);
    }
    Some(answer)
}

fn get(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let items = current(lua);
    let t = lua.create_table_with_capacity(items.len(), 0)?;
    for (i, id) in items.into_iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, id)?)?;
    }
    ret(lua, t)
}

fn set_native(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mut items = Vec::new();
    if let Some(Value::Table(t)) = args.first() {
        for v in t.sequence_values::<Value>() {
            if let Some(id) = crate::runtime::instance_from_value(lua, &v?) {
                items.push(id);
            }
        }
    }
    set(lua, items);
    none()
}

pub fn register(r: &mut Registry) {
    r.add("Selection", &["Get"], get);
    r.add("Selection", &["Set"], set_native);
}
