use super::{Registry, Ret, none, ret};
use crate::convert::{arg_string, enum_value, variant_to_lua};
use crate::runtime::engine;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_reflection::ValueType;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct InputState {
    pub keys_down: BTreeSet<u32>,
    pub last_input_type: Option<u32>,
    pub focused_text_box: Option<InstanceId>,
    pub tutorials: BTreeSet<String>,
    pub pointer: super::user_input::PointerState,
}

pub fn register(r: &mut Registry) {
    r.add("UserInputService", &["GetPlatform"], get_platform);
    r.add("UserInputService", &["IsKeyDown"], is_key_down);
    r.add("UserInputService", &["GetKeysPressed"], get_keys_pressed);
    r.add(
        "UserInputService",
        &["GetLastInputType"],
        get_last_input_type,
    );
    r.add(
        "UserInputService",
        &["GetFocusedTextBox"],
        get_focused_text_box,
    );
    r.add(
        "UserInputService",
        &[
            "GetConnectedGamepads",
            "GetNavigationGamepads",
            "GetGamepadState",
            "GetSupportedGamepadKeyCodes",
        ],
        empty_table,
    );
    r.add(
        "UserInputService",
        &[
            "GetGamepadConnected",
            "IsNavigationGamepad",
            "GamepadSupports",
        ],
        no,
    );
    r.add(
        "UserInputService",
        &["SetNavigationGamepad", "RecenterUserHeadCFrame"],
        ignore,
    );
    r.add("UserInputService", &["GetUserCFrame"], get_user_cframe);
    r.add("UserInputService", &["GetDeviceType"], device_type);
    r.add("UserInputService", &["GetDeviceLevel"], device_level);
    r.add("UserInputService", &["GetMouseLocation"], mouse_location);
    r.add("UserInputService", &["GetMouseDelta"], mouse_delta);
    r.add("UserInputService", &["IsGamepadButtonDown"], no);
    r.add(
        "UserInputService",
        &["IsMouseButtonPressed"],
        mouse_button_pressed,
    );
    r.add("UserInputService", &["GetMouseButtonsPressed"], empty_table);
    r.add(
        "UserInputService",
        &["GetStringForKeyCode"],
        string_for_key_code,
    );
    r.add(
        "UserInputService",
        &["GetImageForKeyCode"],
        image_for_key_code,
    );
    r.add(
        "UserInputService",
        &[
            "GetDeviceAcceleration",
            "GetDeviceGravity",
            "GetDeviceRotation",
        ],
        device_motion,
    );
    r.add("UserGameSettings", &["InStudioMode"], no);
    r.add("UserGameSettings", &["InFullScreen"], in_full_screen);
    r.add(
        "UserGameSettings",
        &["GetTutorialState"],
        get_tutorial_state,
    );
    r.add(
        "UserGameSettings",
        &["SetTutorialState"],
        set_tutorial_state,
    );
    r.add(
        "UserSettings",
        &["IsUserFeatureEnabled"],
        is_user_feature_enabled,
    );
    r.add("UserSettings", &["Reset"], ignore);
}

fn enum_item(lua: &Lua, name: &str, value: u32) -> Result<Value> {
    variant_to_lua(
        lua,
        &Variant::Enum(value),
        Some(&ValueType::Enum(name.into())),
    )
}

pub fn platform() -> u32 {
    if cfg!(target_arch = "wasm32") {
        19
    } else if cfg!(windows) {
        0
    } else if cfg!(target_os = "macos") {
        1
    } else if cfg!(target_os = "ios") {
        2
    } else if cfg!(target_os = "android") {
        3
    } else if cfg!(target_os = "linux") {
        13
    } else {
        19
    }
}

fn get_platform(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, enum_item(lua, "Platform", platform())?)
}

fn is_key_down(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = match args.first() {
        Some(v) => enum_value(lua, v, "KeyCode")?,
        None => return ret(lua, false),
    };
    let down = engine(lua).lock().input.keys_down.contains(&key);
    ret(lua, down)
}

fn get_keys_pressed(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let keys: Vec<u32> = engine(lua).lock().input.keys_down.iter().copied().collect();
    let out = lua.create_table()?;
    for (i, key) in keys.into_iter().enumerate() {
        let obj = {
            let eng = engine(lua);
            let mut e = eng.lock();
            let Ok(obj) = e.dm.create_internal("InputObject") else {
                continue;
            };
            let _ = e.dm.set(obj, "KeyCode", Variant::Enum(key));
            let _ = e.dm.set(obj, "UserInputType", Variant::Enum(8));
            let _ = e.dm.set(obj, "UserInputState", Variant::Enum(0));
            obj
        };
        out.raw_set(i + 1, crate::instance::push_instance(lua, obj)?)?;
    }
    ret(lua, out)
}

fn get_last_input_type(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let last = engine(lua).lock().input.last_input_type.unwrap_or(21);
    ret(lua, enum_item(lua, "UserInputType", last)?)
}

fn get_focused_text_box(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let focused = engine(lua).lock().input.focused_text_box;
    match focused {
        Some(id) => ret(lua, crate::instance::push_instance(lua, id)?),
        None => ret(lua, Value::Nil),
    }
}

fn empty_table(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn in_full_screen(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let on = engine(lua).lock().fullscreen;
    ret(lua, on)
}

fn no(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn ignore(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn get_user_cframe(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(
        lua,
        variant_to_lua(lua, &Variant::CFrame(rbx_math::CFrame::IDENTITY), None)?,
    )
}

fn device_motion(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let obj = {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.dm.create_internal("InputObject").ok()
    };
    match obj {
        Some(o) => ret(lua, crate::instance::push_instance(lua, o)?),
        None => ret(lua, Value::Nil),
    }
}

fn get_tutorial_state(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let done = engine(lua).lock().input.tutorials.contains(&name);
    ret(lua, done)
}

fn set_tutorial_state(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let value = crate::convert::arg_bool(args.get(1));
    let eng = engine(lua);
    let mut e = eng.lock();
    if value {
        e.input.tutorials.insert(name);
    } else {
        e.input.tutorials.remove(&name);
    }
    none()
}

fn is_user_feature_enabled(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let on = rbx_base::flag_value(&name).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    ret(lua, on)
}

fn enum_ret(lua: &Lua, name: &str, item: &str) -> Result<Ret> {
    match crate::types::LEnumItem::by_name(name, item) {
        Some(i) => ret(lua, crate::types::enum_item_value(lua, i)?),
        None => ret(lua, Value::Nil),
    }
}

fn device_type(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let touch = engine(lua).lock().input.pointer.touch_enabled;
    enum_ret(lua, "DeviceType", if touch { "Phone" } else { "Desktop" })
}

fn device_level(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    enum_ret(lua, "DeviceLevel", "High")
}

fn mouse_location(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let p = engine(lua).lock().input.pointer.position;
    ret(lua, crate::types::LVector2(p))
}

fn mouse_delta(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let e = engine(lua);
    let e = e.lock();
    ret(
        lua,
        crate::types::LVector2(e.input.pointer.last_frame_delta),
    )
}

fn mouse_button_pressed(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let button = args
        .first()
        .and_then(|v| enum_value(lua, v, "UserInputType").ok())
        .unwrap_or(u32::MAX);
    let down = engine(lua)
        .lock()
        .input
        .pointer
        .pressed
        .contains_key(&button);
    ret(lua, down)
}

fn string_for_key_code(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let code = args
        .first()
        .and_then(|v| enum_value(lua, v, "KeyCode").ok())
        .unwrap_or(0);
    let text = match code {
        97..=122 => ((code as u8) - 32) as char,
        33..=126 => code as u8 as char,
        _ => {
            let name = rbx_reflection::database()
                .enum_desc("KeyCode")
                .and_then(|e| e.name_of(code))
                .unwrap_or("")
                .to_string();
            return ret(lua, name);
        }
    };
    ret(lua, text.to_string())
}

fn image_for_key_code(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}
