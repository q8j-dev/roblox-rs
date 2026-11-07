pub mod animation;
pub mod bindables;
pub mod http;
pub mod humanoid;
pub mod instance;
pub mod misc;
pub mod network;
pub mod physics;
pub mod players;
pub mod remote;
pub mod services;
pub mod stats;
pub mod terrain;
pub mod test;

use crate::types::rt;
use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant, database};
use std::collections::HashMap;
use std::sync::LazyLock;

pub enum Ret {
    Values(MultiValue),
    Yield,
}

pub type Native = fn(&Lua, InstanceId, &[Value]) -> Result<Ret>;

pub fn ret(lua: &Lua, v: impl IntoLuaMulti) -> Result<Ret> {
    Ok(Ret::Values(v.into_lua_multi(lua)?))
}

pub fn none() -> Result<Ret> {
    Ok(Ret::Values(MultiValue::new()))
}

pub struct Registry {
    methods: HashMap<(&'static str, &'static str), Native>,
}

impl Registry {
    pub fn add(&mut self, class: &'static str, names: &[&'static str], f: Native) {
        for n in names {
            self.methods.insert((class, n), f);
        }
    }
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let mut r = Registry {
        methods: HashMap::new(),
    };
    instance::register(&mut r);
    services::register(&mut r);
    test::register(&mut r);
    bindables::register(&mut r);
    physics::register(&mut r);
    misc::register(&mut r);
    humanoid::register(&mut r);
    players::register(&mut r);
    animation::register(&mut r);
    terrain::register(&mut r);
    remote::register(&mut r);
    network::register(&mut r);
    stats::register(&mut r);
    http::register(&mut r);
    r
});

pub fn has_native(class: &str, name: &str) -> bool {
    database()
        .superclasses(class)
        .any(|c| REGISTRY.methods.contains_key(&(c.name.as_str(), name)))
}

pub fn call(lua: &Lua, id: InstanceId, class: &str, name: &str, args: MultiValue) -> Result<Ret> {
    let args = args.into_vec();
    for c in database().superclasses(class) {
        if let Some(f) = REGISTRY.methods.get(&(c.name.as_str(), name)) {
            return f(lua, id, &args);
        }
    }
    if let Some(f) = database().find_function(class, name)
        && let Some(target) = f
            .deprecated_by
            .as_deref()
            .filter(|t| !t.is_empty() && *t != name)
    {
        return call(lua, id, class, target, MultiValue::from_vec(args));
    }
    Err(rt(format!(
        "{name} is not available on {class} in this build"
    )))
}

pub fn get_property(lua: &Lua, id: InstanceId, class: &str, name: &str) -> Result<Option<Value>> {
    if class == "AnimationTrack" {
        return animation::get_track_property(lua, id, name);
    }
    if class == "VehicleSeat" && name == "AreHingesDetected" {
        return physics::hinges_detected(lua, id).map(Some);
    }
    Ok(None)
}

pub fn set_property(
    lua: &Lua,
    id: InstanceId,
    class: &str,
    name: &str,
    value: &Variant,
) -> Result<bool> {
    if class == "AnimationTrack" {
        return animation::set_track_property(lua, id, name, value);
    }
    Ok(false)
}
