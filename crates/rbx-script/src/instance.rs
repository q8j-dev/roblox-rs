use crate::api;
use crate::convert::{lua_to_variant, lua_type_name, variant_to_lua};
use crate::runtime::{
    current_identity, engine, instance_from_value, pump, with_state, yield_sentinel,
};
use crate::security::permission_name;
use crate::signal::LSignal;
use crate::types::rt;
use mlua::{
    Function, Lua, MetaMethod, MultiValue, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};
use parking_lot::Mutex;
use rbx_dom::{InstanceId, database};
use std::sync::Arc;

pub struct LInstance {
    pub id: InstanceId,
    release: Arc<Mutex<Vec<InstanceId>>>,
}

impl Drop for LInstance {
    fn drop(&mut self) {
        self.release.lock().push(self.id);
    }
}

impl UserData for LInstance {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(mlua::MetaMethod::Type, "Instance");
        f.add_meta_field_with(MetaMethod::Index, |lua| {
            lua.named_registry_value::<Function>("rbx_instance_index")
        });
        f.add_meta_field_with(MetaMethod::NewIndex, |lua| {
            lua.named_registry_value::<Function>("rbx_instance_newindex")
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
            Ok(instance_from_value(lua, &other) == Some(this.id))
        });
        m.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            let eng = engine(lua);
            let e = eng.lock();
            Ok(e.dm.name_of(this.id).to_string())
        });
    }
}

pub fn push_instance(lua: &Lua, id: InstanceId) -> Result<Value> {
    let (cache, release) = with_state(lua, |s| {
        (
            s.instance_cache
                .as_ref()
                .map(|k| lua.registry_value::<Table>(k)),
            s.release.clone(),
        )
    });
    let cache = match cache {
        Some(c) => c?,
        None => return Err(rt("runtime not initialized")),
    };
    let key = id.raw() as f64;
    if let Value::UserData(ud) = cache.raw_get::<Value>(key)? {
        return Ok(Value::UserData(ud));
    }
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        if !e.dm.contains(id) {
            return Ok(Value::Nil);
        }
        e.dm.retain(id);
    }
    let ud = lua.create_userdata(LInstance { id, release })?;
    cache.raw_set(key, ud.clone())?;
    Ok(Value::UserData(ud))
}

pub fn push_instances(lua: &Lua, ids: &[InstanceId]) -> Result<Vec<Value>> {
    let (cache, release) = with_state(lua, |s| {
        (
            s.instance_cache
                .as_ref()
                .map(|k| lua.registry_value::<Table>(k)),
            s.release.clone(),
        )
    });
    let cache = match cache {
        Some(c) => c?,
        None => return Err(rt("runtime not initialized")),
    };
    let mut out = Vec::with_capacity(ids.len());
    let mut missing = Vec::new();
    for (i, &id) in ids.iter().enumerate() {
        match cache.raw_get::<Value>(id.raw() as f64)? {
            Value::UserData(ud) => out.push(Value::UserData(ud)),
            _ => {
                out.push(Value::Nil);
                missing.push(i);
            }
        }
    }
    if missing.is_empty() {
        return Ok(out);
    }
    let alive: Vec<bool> = {
        let eng = engine(lua);
        let mut e = eng.lock();
        missing
            .iter()
            .map(|&i| {
                let id = ids[i];
                let ok = e.dm.contains(id);
                if ok {
                    e.dm.retain(id);
                }
                ok
            })
            .collect()
    };
    for (&i, ok) in missing.iter().zip(alive) {
        if !ok {
            continue;
        }
        let id = ids[i];
        let ud = lua.create_userdata(LInstance {
            id,
            release: release.clone(),
        })?;
        cache.raw_set(id.raw() as f64, ud.clone())?;
        out[i] = Value::UserData(ud);
    }
    Ok(out)
}

fn key_name(key: &Value) -> Result<String> {
    match key {
        Value::String(s) => Ok(s.to_str()?.to_string()),
        Value::Number(n) => Ok(crate::format::lua_number_str(*n)),
        Value::Integer(i) => Ok(i.to_string()),
        other => Err(rt(format!(
            "{} is not a valid member",
            lua_type_name(other)
        ))),
    }
}

fn security_error(lua: &Lua, name: &str, p: rbx_reflection::Security) -> mlua::Error {
    rt(format!(
        "The current identity ({}) cannot {name} (lacking permission {})",
        current_identity(lua).level(),
        permission_name(p)
    ))
}

pub fn index(lua: &Lua, id: InstanceId, key: Value) -> Result<Value> {
    let name = key_name(&key)?;
    let eng = engine(lua);
    let (class, desc) = {
        let e = eng.lock();
        let class =
            e.dm.class_of(id)
                .ok_or_else(|| rt(format!("{name} is not a valid member")))?;
        (class, e.dm.property_descriptor(id, &name))
    };
    if let Some(desc) = desc {
        let visible = desc.is_scriptable()
            || matches!(
                name.as_str(),
                "Name" | "Parent" | "ClassName" | "className" | "Archivable" | "archivable"
            );
        if visible {
            if !current_identity(lua).allows(desc.security) {
                return Err(security_error(
                    lua,
                    &format!("access property {name}"),
                    desc.security,
                ));
            }
            if let Some(v) = api::get_property(lua, id, class, &name)? {
                return Ok(v);
            }
            let v = eng
                .lock()
                .dm
                .get(id, &name)
                .map_err(|e| rt(e.to_string()))?;
            return variant_to_lua(lua, &v, Some(&desc.ty));
        }
    }
    if let Some(f) = database().find_function(class, &name) {
        if f.kind == rbx_reflection::FunctionKind::Callback {
            return Err(rt(format!(
                "{name} is a callback member of {class}; you can only set the callback value, get is not available"
            )));
        }
        if !current_identity(lua).allows(f.security) {
            return Err(security_error(lua, &format!("call {name}"), f.security));
        }
        return Ok(Value::Function(method_function(lua, &f.name)?));
    }
    if let Some(ev) = database().find_event(class, &name) {
        if !current_identity(lua).allows(ev.security) {
            return Err(security_error(
                lua,
                &format!("access event {name}"),
                ev.security,
            ));
        }
        return Ok(Value::UserData(lua.create_userdata(LSignal {
            target: id,
            event: event_static(class, &name),
        })?));
    }
    let child = eng.lock().dm.find_first_child(id, &name, false);
    if let Some(c) = child {
        return push_instance(lua, c);
    }
    if let Some(upper) = camel_case_alias(&name)
        && class_has_member(&eng.lock().dm, id, class, &upper)
    {
        camel_case_violation(lua, id, &name, &upper);
        return index(lua, id, Value::String(lua.create_string(&upper)?));
    }
    Err(rt(format!("{name} is not a valid member of {class}")))
}

fn camel_case_alias(name: &str) -> Option<String> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    Some(first.to_ascii_uppercase().to_string() + chars.as_str())
}

fn class_has_member(dm: &rbx_dom::DataModel, id: InstanceId, class: &str, name: &str) -> bool {
    let db = database();
    dm.property_descriptor(id, name).is_some()
        || db.find_function(class, name).is_some()
        || db.find_event(class, name).is_some()
}

fn camel_case_violation(lua: &Lua, id: InstanceId, name: &str, upper: &str) {
    let script = crate::runtime::current_info(lua).script;
    let eng = engine(lua);
    let mut e = eng.lock();
    e.camel_case_violations += 1;
    if e.camel_case_violations > 100 {
        return;
    }
    let class = e.dm.class_of(id).unwrap_or_default();
    let message = match script {
        Some(s) => format!(
            "\"{class}.{name}\" should be \"{upper}\" in {}",
            e.dm.full_name(s)
        ),
        None => format!("\"{class}.{name}\" should be \"{upper}\""),
    };
    e.print(crate::runtime::MessageType::Warning, message);
}

pub fn event_static(class: &str, name: &str) -> &'static str {
    let db = database();
    match db.find_event(class, name) {
        Some(e) => match e
            .deprecated_by
            .as_deref()
            .filter(|t| !t.is_empty() && *t != name)
            .and_then(|t| db.find_event(class, t))
        {
            Some(c) => c.name.as_str(),
            None => e.name.as_str(),
        },
        None => "Changed",
    }
}

pub fn newindex(lua: &Lua, id: InstanceId, key: Value, value: Value) -> Result<()> {
    let name = key_name(&key)?;
    let eng = engine(lua);
    let (class, desc) = {
        let e = eng.lock();
        let class = e.dm.class_of(id).unwrap_or("Instance");
        (class, e.dm.property_descriptor(id, &name))
    };
    let Some(desc) = desc else {
        if let Some(f) = database().find_function(class, &name)
            && f.kind == rbx_reflection::FunctionKind::Callback
        {
            if !current_identity(lua).allows(f.security) {
                return Err(security_error(
                    lua,
                    &format!("set callback {name}"),
                    f.security,
                ));
            }
            return crate::api::bindables::set_callback(lua, id, &f.name, value);
        }
        return Err(rt(format!("{name} is not a valid member of {class}")));
    };
    let visible = desc.is_scriptable()
        || matches!(
            name.as_str(),
            "Name" | "Parent" | "Archivable" | "archivable"
        );
    if !visible {
        return Err(rt(format!("{name} is not a valid member of {class}")));
    }
    if desc.read_only || matches!(name.as_str(), "ClassName" | "className" | "DataCost") {
        return Err(rt("can't set value"));
    }
    if name == "Parent" && with_state(lua, |s| s.reparent_guard.contains(&id)) {
        let new_parent = instance_from_value(lua, &value);
        let message = {
            let e = eng.lock();
            let current = e.dm.parent_of(id);
            if current == new_parent {
                return Ok(());
            }
            let label = |p: Option<InstanceId>| {
                p.map(|p| e.dm.name_of(p).to_string())
                    .unwrap_or_else(|| "NULL".into())
            };
            let own = e.dm.name_of(id).to_string();
            format!(
                "Something unexpectedly tried to set the parent of {own} to {} while trying to set the parent of {own}. Current parent is {}.",
                label(new_parent),
                label(current)
            )
        };
        eng.lock()
            .print(crate::runtime::MessageType::Warning, message);
        return Ok(());
    }
    if !current_identity(lua).allows(desc.security) {
        return Err(security_error(
            lua,
            &format!("set property {name}"),
            desc.security,
        ));
    }
    let variant = lua_to_variant(lua, value, &desc.ty).map_err(|e| {
        rt(format!(
            "bad argument #3 to '{name}' ({})",
            crate::runtime::format_error(&e)
        ))
    })?;
    let handled = api::set_property(lua, id, class, &name, &variant)?;
    if !handled {
        let r = eng.lock().dm.set(id, &name, variant);
        r.map_err(|e| rt(e.to_string()))?;
    }
    pump(lua);
    Ok(())
}

pub fn method_function(lua: &Lua, name: &str) -> Result<Function> {
    let cache = with_state(lua, |s| {
        s.method_cache
            .as_ref()
            .map(|k| lua.registry_value::<Table>(k))
    });
    let cache = match cache {
        Some(c) => c?,
        None => return Err(rt("runtime not initialized")),
    };
    if let Value::Function(f) = cache.raw_get::<Value>(name)? {
        return Ok(f);
    }
    let owned = name.to_string();
    let native =
        lua.create_function(move |lua, args: MultiValue| call_method(lua, &owned, args))?;
    let wrap: Function = lua.named_registry_value("rbx_wrap")?;
    let f: Function = wrap.call(native)?;
    cache.raw_set(name, f.clone())?;
    Ok(f)
}

fn call_method(lua: &Lua, name: &str, args: MultiValue) -> Result<MultiValue> {
    let mut args = args.into_vec();
    let this = if args.is_empty() {
        None
    } else {
        instance_from_value(lua, &args[0])
    };
    let Some(id) = this else {
        return Err(rt(format!(
            "Expected ':' not '.' calling member function {name}"
        )));
    };
    args.remove(0);
    let class = engine(lua).lock().dm.class_of(id).unwrap_or("Instance");
    let result = api::call(lua, id, class, name, MultiValue::from_vec(args))?;
    pump(lua);
    match result {
        api::Ret::Values(v) => Ok(v),
        api::Ret::Yield => Ok(MultiValue::from_vec(vec![yield_sentinel(lua)?])),
    }
}
