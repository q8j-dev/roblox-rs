use super::{Registry, Ret, none, ret};
use crate::convert::{arg_string, infer_variant};
use crate::runtime::{engine, instance_from_value, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};
use std::collections::HashMap;

#[derive(Default)]
pub struct StyleState {
    pub rule_properties: HashMap<InstanceId, Vec<(String, Variant)>>,
    pub derives: HashMap<InstanceId, Vec<InstanceId>>,
    pub dirty: bool,
}

pub fn register(r: &mut Registry) {
    r.add("StyleRule", &["SetProperties"], set_properties);
    r.add("StyleRule", &["SetProperty"], set_property);
    r.add(
        "StyleRule",
        &["GetProperties", "GetPropertiesResolved"],
        get_properties,
    );
    r.add(
        "StyleRule",
        &["GetProperty", "GetPropertyResolved"],
        get_property,
    );
    r.add("StyleSheet", &["SetDerives"], set_derives);
    r.add("StyleSheet", &["GetDerives"], get_derives);
    r.add("StyleBase", &["GetStyleRules"], get_style_rules);
}

fn style_value(lua: &Lua, v: &Value) -> Result<Variant> {
    infer_variant(v)
        .ok_or_else(|| {
            rt(format!(
                "{} is not a supported style property value",
                crate::convert::lua_type_name(v)
            ))
        })
        .or_else(|e| match v {
            Value::Nil => Ok(Variant::Nil),
            _ => {
                let _ = lua;
                Err(e)
            }
        })
}

fn set_properties(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(Value::Table(t)) = args.first() else {
        return Err(rt("Argument 1 missing or nil"));
    };
    let mut props = Vec::new();
    for pair in t.clone().pairs::<String, Value>() {
        let (k, v) = pair?;
        props.push((k, style_value(lua, &v)?));
    }
    props.sort_by(|a, b| a.0.cmp(&b.0));
    with_state(lua, |s| {
        s.styles.rule_properties.insert(id, props);
        s.styles.dirty = true;
    });
    none()
}

fn set_property(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let value = style_value(lua, args.get(1).unwrap_or(&Value::Nil))?;
    with_state(lua, |s| {
        let props = s.styles.rule_properties.entry(id).or_default();
        props.retain(|(k, _)| *k != name);
        if !matches!(value, Variant::Nil) {
            props.push((name, value));
        }
        s.styles.dirty = true;
    });
    none()
}

fn get_properties(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let props = with_state(lua, |s| s.styles.rule_properties.get(&id).cloned()).unwrap_or_default();
    let t = lua.create_table()?;
    for (k, v) in props {
        t.set(k, crate::convert::variant_to_lua(lua, &v, None)?)?;
    }
    ret(lua, t)
}

fn get_property(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let v = with_state(lua, |s| {
        s.styles
            .rule_properties
            .get(&id)
            .and_then(|p| p.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone()))
    });
    match v {
        Some(v) => ret(lua, crate::convert::variant_to_lua(lua, &v, None)?),
        None => ret(lua, Value::Nil),
    }
}

fn set_derives(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mut list = Vec::new();
    if let Some(Value::Table(t)) = args.first() {
        for v in t.clone().sequence_values::<Value>() {
            if let Some(i) = instance_from_value(lua, &v?) {
                list.push(i);
            }
        }
    }
    with_state(lua, |s| {
        s.styles.derives.insert(id, list);
        s.styles.dirty = true;
    });
    none()
}

fn get_derives(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let list = with_state(lua, |s| s.styles.derives.get(&id).cloned()).unwrap_or_default();
    let t = lua.create_sequence_from(crate::instance::push_instances(lua, &list)?)?;
    ret(lua, t)
}

fn get_style_rules(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let rules: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.children_of(id)
            .iter()
            .copied()
            .filter(|c| e.dm.is_a(*c, "StyleRule"))
            .collect()
    };
    ret(
        lua,
        lua.create_sequence_from(crate::instance::push_instances(lua, &rules)?)?,
    )
}
