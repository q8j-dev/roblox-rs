pub mod actions;
pub mod animation;
pub mod bindables;
pub mod camera;
pub mod core;
pub mod flags;
pub mod gui;
pub mod http;
pub mod humanoid;
pub mod input;
pub mod instance;
pub mod ipc;
pub mod ixp;
pub mod localization;
pub mod misc;
pub mod network;
pub mod physics;
pub mod players;
pub mod plugin;
pub mod remote;
pub mod selection;
pub mod services;
pub mod sound;
pub mod stats;
pub mod terrain;
pub mod test;
pub mod tween;
pub mod user_input;
pub mod verbs;

use crate::types::rt;
use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant, database};
use rustc_hash::FxHashMap as HashMap;
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
        methods: HashMap::default(),
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
    gui::register(&mut r);
    core::register(&mut r);
    flags::register(&mut r);
    ipc::register(&mut r);
    ixp::register(&mut r);
    localization::register(&mut r);
    input::register(&mut r);
    actions::register(&mut r);
    tween::register(&mut r);
    camera::register(&mut r);
    user_input::register(&mut r);
    sound::register(&mut r);
    selection::register(&mut r);
    plugin::register(&mut r);
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
    if class == "Player" && name == "ChatMode" {
        let guest = {
            let eng = crate::runtime::engine(lua);
            let e = eng.lock();
            matches!(e.dm.get(id, "UserId"), Ok(Variant::Int(u)) if u < 0)
                || matches!(e.dm.get(id, "UserId"), Ok(Variant::Int64(u)) if u < 0)
        };
        let v = crate::convert::variant_to_lua(
            lua,
            &Variant::Enum(if guest { 0 } else { 1 }),
            Some(&rbx_reflection::ValueType::Enum("ChatMode".into())),
        )?;
        return Ok(Some(v));
    }
    if class == "Sound"
        && matches!(
            name,
            "IsPlaying" | "isPlaying" | "IsPaused" | "TimeLength" | "TimePosition"
        )
    {
        return sound::get_sound_property(lua, id, name);
    }
    if class == "UserInputService" && matches!(name, "KeyboardEnabled" | "MouseEnabled") {
        let desktop = !cfg!(any(target_os = "ios", target_os = "android"));
        return Ok(Some(Value::Boolean(desktop)));
    }
    if class == "UserInputService" && name == "TouchEnabled" {
        let touch = crate::runtime::engine(lua)
            .lock()
            .input
            .pointer
            .touch_enabled;
        return Ok(Some(Value::Boolean(touch)));
    }
    if matches!(class, "Mouse" | "PlayerMouse") {
        return user_input::get_mouse_property(lua, id, name);
    }
    if matches!(
        name,
        "AbsolutePosition"
            | "AbsoluteSize"
            | "AbsoluteWindowSize"
            | "TextBounds"
            | "TextFits"
            | "ViewportSize"
    ) {
        return gui::get_gui_property(lua, id, class, name);
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
    if class == "Sound" && name == "TimePosition" {
        if let Some(v) = value.as_f64() {
            sound::set_time_position(lua, id, v);
        }
        return Ok(true);
    }
    Ok(false)
}
