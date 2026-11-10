use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_string};
use crate::instance::push_instance;
use crate::runtime::{engine, expect_instance, with_state, yield_sentinel};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

fn children_table(lua: &Lua, ids: &[InstanceId]) -> Result<mlua::Table> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, v) in crate::instance::push_instances(lua, ids)?
        .into_iter()
        .enumerate()
    {
        t.raw_set(i + 1, v)?;
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
    if crate::runtime::has_listeners(lua, id, "Destroying") {
        crate::runtime::fire_lua(lua, id, "Destroying", mlua::MultiValue::new());
    }
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

fn ancestors(lua: &Lua, id: InstanceId) -> Vec<InstanceId> {
    let eng = engine(lua);
    let e = eng.lock();
    let mut out = Vec::new();
    let mut cur = e.dm.parent_of(id);
    while let Some(p) = cur {
        out.push(p);
        cur = e.dm.parent_of(p);
    }
    out
}

fn found(lua: &Lua, id: Option<InstanceId>) -> Result<Ret> {
    match id {
        Some(c) => ret(lua, push_instance(lua, c)?),
        None => ret(lua, Value::Nil),
    }
}

fn required_string(args: &[Value]) -> Result<String> {
    arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))
}

fn find_first_ancestor(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let eng = engine(lua);
    let hit = ancestors(lua, id)
        .into_iter()
        .find(|a| eng.lock().dm.name_of(*a) == name);
    found(lua, hit)
}

fn find_first_ancestor_of_class(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = required_string(args)?;
    let eng = engine(lua);
    let hit = ancestors(lua, id)
        .into_iter()
        .find(|a| eng.lock().dm.class_of(*a) == Some(class.as_str()));
    found(lua, hit)
}

fn find_first_ancestor_which_is_a(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = required_string(args)?;
    let eng = engine(lua);
    let hit = ancestors(lua, id)
        .into_iter()
        .find(|a| eng.lock().dm.is_a(*a, &class));
    found(lua, hit)
}

fn find_first_child_of_class(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = required_string(args)?;
    let hit = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.children_of(id)
            .iter()
            .copied()
            .find(|c| e.dm.class_of(*c) == Some(class.as_str()))
    };
    found(lua, hit)
}

fn find_first_child_which_is_a(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let class = required_string(args)?;
    let recursive = arg_bool(args.get(1));
    let hit = {
        let eng = engine(lua);
        let e = eng.lock();
        if recursive {
            e.dm.descendants(id)
                .into_iter()
                .find(|c| e.dm.is_a(*c, &class))
        } else {
            e.dm.children_of(id)
                .iter()
                .copied()
                .find(|c| e.dm.is_a(*c, &class))
        }
    };
    found(lua, hit)
}

fn get_descendants(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let ids = engine(lua).lock().dm.descendants(id);
    ret(lua, children_table(lua, &ids)?)
}

fn attribute_value(lua: &Lua, v: &rbx_dom::Variant) -> Result<Value> {
    match v {
        rbx_dom::Variant::Double(n) => Ok(Value::Number(*n)),
        other => crate::convert::variant_to_lua(lua, other, None),
    }
}

fn get_attribute(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let v = engine(lua).lock().dm.attribute(id, &name).cloned();
    match v {
        Some(v) => ret(lua, attribute_value(lua, &v)?),
        None => ret(lua, Value::Nil),
    }
}

fn get_attributes(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let all = engine(lua).lock().dm.attributes(id).to_vec();
    let t = lua.create_table()?;
    for (k, v) in all {
        t.raw_set(k, attribute_value(lua, &v)?)?;
    }
    ret(lua, t)
}

fn set_attribute(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let value = args.get(1).cloned().unwrap_or(Value::Nil);
    let v = crate::convert::infer_variant(&value).ok_or_else(|| {
        rt(format!(
            "{} is not a supported attribute type",
            crate::convert::lua_type_name(&value)
        ))
    })?;
    engine(lua).lock().dm.set_attribute(id, &name, v);
    none()
}

fn get_attribute_changed_signal(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let signal = crate::signal::LSignal {
        target: id,
        event: crate::signal::filtered_event("AttributeChanged", &name),
    };
    ret(lua, lua.create_userdata(signal)?)
}

fn get_property_changed_signal(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let valid = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.property_descriptor(id, &name).is_some()
            || matches!(
                name.as_str(),
                "Name" | "Parent" | "Archivable" | "ClassName"
            )
    };
    if !valid {
        return Err(rt(format!("{name} is not a valid property name.")));
    }
    let signal = crate::signal::LSignal {
        target: id,
        event: crate::signal::filtered_event("Changed", &name),
    };
    ret(lua, lua.create_userdata(signal)?)
}

fn add_tag(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    engine(lua).lock().dm.add_tag(id, &tag);
    none()
}

fn remove_tag(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    engine(lua).lock().dm.remove_tag(id, &tag);
    none()
}

fn has_tag(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    let has = engine(lua).lock().dm.tags(id).iter().any(|t| *t == tag);
    ret(lua, has)
}

fn get_tags(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let tags = engine(lua).lock().dm.tags(id).to_vec();
    ret(lua, lua.create_sequence_from(tags)?)
}

pub fn register(r: &mut Registry) {
    r.add("Instance", &["FindFirstAncestor"], find_first_ancestor);
    r.add(
        "Instance",
        &["FindFirstAncestorOfClass"],
        find_first_ancestor_of_class,
    );
    r.add(
        "Instance",
        &["FindFirstAncestorWhichIsA"],
        find_first_ancestor_which_is_a,
    );
    r.add(
        "Instance",
        &["FindFirstChildOfClass"],
        find_first_child_of_class,
    );
    r.add(
        "Instance",
        &["FindFirstChildWhichIsA"],
        find_first_child_which_is_a,
    );
    r.add("Instance", &["GetDescendants"], get_descendants);
    r.add("Instance", &["GetAttribute"], get_attribute);
    r.add("Instance", &["GetAttributes"], get_attributes);
    r.add("Instance", &["SetAttribute"], set_attribute);
    r.add(
        "Instance",
        &["GetAttributeChangedSignal"],
        get_attribute_changed_signal,
    );
    r.add(
        "Instance",
        &["GetPropertyChangedSignal"],
        get_property_changed_signal,
    );
    r.add("Instance", &["AddTag"], add_tag);
    r.add("CollectionService", &["AddTag"], cs_add_tag);
    r.add("CollectionService", &["RemoveTag"], cs_remove_tag);
    r.add("CollectionService", &["HasTag"], cs_has_tag);
    r.add("CollectionService", &["GetTags"], cs_get_tags);
    r.add("CollectionService", &["GetTagged"], cs_get_tagged);
    r.add("CollectionService", &["GetAllTags"], cs_all_tags);
    r.add(
        "CollectionService",
        &["GetInstanceAddedSignal"],
        cs_added_signal,
    );
    r.add(
        "CollectionService",
        &["GetInstanceRemovedSignal"],
        cs_removed_signal,
    );
    r.add("Instance", &["ResetPropertyToDefault"], reset_property);
    r.add("Instance", &["RemoveTag"], remove_tag);
    r.add("Instance", &["HasTag"], has_tag);
    r.add("Instance", &["GetTags"], get_tags);
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

fn reset_property(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = required_string(args)?;
    let eng = engine(lua);
    let mut e = eng.lock();
    let class = e.dm.class_of(id).unwrap_or("Instance");
    let desc =
        e.dm.property_descriptor(id, &name)
            .ok_or_else(|| rt(format!("{name} is not a valid property name.")))?;
    let v = rbx_dom::default_value(class, &desc);
    e.dm.set(id, &name, v).map_err(|err| rt(err.to_string()))?;
    none()
}

fn cs_target(lua: &Lua, args: &[Value]) -> Result<(InstanceId, String)> {
    let target = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v))
        .ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let tag = arg_string(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))?;
    Ok((target, tag))
}

fn cs_fire(lua: &Lua, service: InstanceId, base: &str, tag: &str, target: InstanceId) {
    let event = crate::signal::filtered_event(base, tag);
    if crate::runtime::has_listeners(lua, service, event)
        && let Ok(v) = push_instance(lua, target)
    {
        crate::runtime::fire_lua(lua, service, event, mlua::MultiValue::from_vec(vec![v]));
    }
}

fn cs_add_tag(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let (target, tag) = cs_target(lua, args)?;
    let added = engine(lua).lock().dm.add_tag(target, &tag);
    if added {
        cs_fire(lua, id, "InstanceAdded", &tag, target);
    }
    none()
}

fn cs_remove_tag(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let (target, tag) = cs_target(lua, args)?;
    let removed = engine(lua).lock().dm.remove_tag(target, &tag);
    if removed {
        cs_fire(lua, id, "InstanceRemoved", &tag, target);
    }
    none()
}

fn cs_has_tag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let (target, tag) = cs_target(lua, args)?;
    let has = engine(lua).lock().dm.tags(target).iter().any(|t| *t == tag);
    ret(lua, has)
}

fn cs_get_tags(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let target = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v))
        .ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let tags = engine(lua).lock().dm.tags(target).to_vec();
    ret(lua, lua.create_sequence_from(tags)?)
}

fn cs_get_tagged(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    let ids: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        let root = e.dm.root();
        e.dm.descendants(root)
            .into_iter()
            .filter(|d| e.dm.tags(*d).iter().any(|t| *t == tag))
            .collect()
    };
    ret(lua, children_table(lua, &ids)?)
}

fn cs_all_tags(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let mut tags: Vec<String> = {
        let eng = engine(lua);
        let e = eng.lock();
        let root = e.dm.root();
        e.dm.descendants(root)
            .into_iter()
            .flat_map(|d| e.dm.tags(d).to_vec())
            .collect()
    };
    tags.sort();
    tags.dedup();
    ret(lua, lua.create_sequence_from(tags)?)
}

fn cs_added_signal(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    let signal = crate::signal::LSignal {
        target: id,
        event: crate::signal::filtered_event("InstanceAdded", &tag),
    };
    ret(lua, lua.create_userdata(signal)?)
}

fn cs_removed_signal(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tag = required_string(args)?;
    let signal = crate::signal::LSignal {
        target: id,
        event: crate::signal::filtered_event("InstanceRemoved", &tag),
    };
    ret(lua, lua.create_userdata(signal)?)
}
