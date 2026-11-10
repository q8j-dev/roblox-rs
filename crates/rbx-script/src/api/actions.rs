use super::{Registry, Ret, none, ret};
use crate::convert::{arg_number, arg_string, variant_to_lua};
use crate::runtime::{engine, fire_lua, with_state};
use crate::types::{LEnumItem, rt};
use mlua::{FromLua, Function, Lua, MultiValue, RegistryKey, Result, Table, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_reflection::ValueType;

#[derive(Clone, Debug, PartialEq)]
pub enum ActionInput {
    KeyCode(u32),
    InputType(u32),
    PlayerAction(u32),
    Character(String),
}

pub struct Action {
    pub name: String,
    pub func: RegistryKey,
    pub inputs: Vec<ActionInput>,
    pub touch: bool,
    pub core: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub position: Option<rbx_math::UDim2>,
    pub priority: i32,
}

#[derive(Default)]
pub struct ActionState {
    pub actions: Vec<Action>,
}

pub fn register(r: &mut Registry) {
    r.add(
        "ContextActionService",
        &["BindAction", "BindActionToInputTypes"],
        bind_action,
    );
    r.add(
        "ContextActionService",
        &["BindCoreAction"],
        bind_core_action,
    );
    r.add("ContextActionService", &["UnbindAction"], unbind_action);
    r.add(
        "ContextActionService",
        &["BindActionAtPriority"],
        bind_action_at_priority,
    );
    r.add(
        "ContextActionService",
        &["BindCoreActionAtPriority"],
        bind_core_action_at_priority,
    );
    r.add(
        "ContextActionService",
        &["GetAllBoundCoreActionInfo"],
        get_all_bound_core_action_info,
    );
    r.add(
        "ContextActionService",
        &["UnbindCoreAction"],
        unbind_core_action,
    );
    r.add("ContextActionService", &["UnbindAllActions"], unbind_all);
    r.add(
        "ContextActionService",
        &["GetBoundActionInfo"],
        get_bound_action_info,
    );
    r.add(
        "ContextActionService",
        &["GetAllBoundActionInfo"],
        get_all_bound_action_info,
    );
    r.add("ContextActionService", &["SetTitle"], set_title);
    r.add("ContextActionService", &["SetDescription"], set_description);
    r.add("ContextActionService", &["SetImage"], set_image);
    r.add("ContextActionService", &["SetPosition"], set_position);
    r.add("ContextActionService", &["CallFunction"], call_function);
    r.add("ContextActionService", &["GetButton"], get_button);
    r.add(
        "ContextActionService",
        &["FireActionButtonFoundSignal"],
        button_found,
    );
    r.add(
        "ContextActionService",
        &["GetCurrentLocalToolIcon"],
        current_tool_icon,
    );
    r.add(
        "ContextActionService",
        &["BindActivate", "UnbindActivate"],
        ignore,
    );
    r.add(
        "PlayerGui",
        &["GetTopbarTransparency"],
        get_topbar_transparency,
    );
    r.add(
        "PlayerGui",
        &["SetTopbarTransparency"],
        set_topbar_transparency,
    );
}

fn parse_inputs(lua: &Lua, args: &[Value]) -> Vec<ActionInput> {
    let mut out = Vec::new();
    for v in args {
        if let Ok(item) = LEnumItem::from_lua(v.clone(), lua) {
            match item.enum_name {
                "KeyCode" => out.push(ActionInput::KeyCode(item.value)),
                "UserInputType" => out.push(ActionInput::InputType(item.value)),
                "PlayerActions" => out.push(ActionInput::PlayerAction(item.value)),
                _ => {}
            }
        } else if let Value::String(s) = v {
            out.push(ActionInput::Character(s.to_string_lossy().to_string()));
        }
    }
    out
}

fn input_value(lua: &Lua, input: &ActionInput) -> Result<Value> {
    match input {
        ActionInput::KeyCode(k) => variant_to_lua(
            lua,
            &Variant::Enum(*k),
            Some(&ValueType::Enum("KeyCode".into())),
        ),
        ActionInput::InputType(t) => variant_to_lua(
            lua,
            &Variant::Enum(*t),
            Some(&ValueType::Enum("UserInputType".into())),
        ),
        ActionInput::PlayerAction(a) => variant_to_lua(
            lua,
            &Variant::Enum(*a),
            Some(&ValueType::Enum("PlayerActions".into())),
        ),
        ActionInput::Character(c) => Ok(Value::String(lua.create_string(c)?)),
    }
}

fn info_table(lua: &Lua, a: &Action, order: usize) -> Result<Table> {
    let t = lua.create_table()?;
    if let Some(v) = &a.title {
        t.set("title", v.as_str())?;
    }
    if let Some(v) = &a.description {
        t.set("description", v.as_str())?;
    }
    if let Some(v) = &a.image {
        t.set("image", v.as_str())?;
    }
    if let Some(p) = a.position {
        t.set("position", variant_to_lua(lua, &Variant::UDim2(p), None)?)?;
    }
    t.set("createTouchButton", a.touch)?;
    let inputs = lua.create_table()?;
    for (i, input) in a.inputs.iter().enumerate() {
        inputs.raw_set(i + 1, input_value(lua, input)?)?;
    }
    t.set("inputTypes", inputs)?;
    t.set("stackOrder", order as i64)?;
    if a.core {
        t.set("isCore", true)?;
    }
    Ok(t)
}

fn remove(lua: &Lua, id: InstanceId, name: &str, core: bool) -> Result<()> {
    let removed = with_state(lua, |s| {
        let pos = s
            .actions
            .actions
            .iter()
            .position(|a| a.name == name && a.core == core);
        pos.map(|p| s.actions.actions.remove(p))
    });
    if let Some(a) = removed {
        let info = info_table(lua, &a, 0)?;
        lua.remove_registry_value(a.func)?;
        fire_lua(
            lua,
            id,
            "BoundActionRemoved",
            MultiValue::from_vec(vec![
                Value::String(lua.create_string(name)?),
                Value::Table(info),
            ]),
        );
    }
    Ok(())
}

fn bind(lua: &Lua, id: InstanceId, args: &[Value], core: bool) -> Result<Ret> {
    bind_at(lua, id, args, core, None)
}

fn bind_at(
    lua: &Lua,
    id: InstanceId,
    args: &[Value],
    core: bool,
    priority: Option<i32>,
) -> Result<Ret> {
    let name = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let func = match args.get(1) {
        Some(Value::Function(f)) => f.clone(),
        _ => return Err(rt("Argument 2 missing or nil")),
    };
    let touch = crate::convert::arg_bool(args.get(2));
    let first_input = if priority.is_some() { 4 } else { 3 };
    let inputs = parse_inputs(lua, args.get(first_input..).unwrap_or(&[]));
    let priority = priority.unwrap_or(2000);
    remove(lua, id, &name, core)?;
    let action = Action {
        name: name.clone(),
        func: lua.create_registry_value(func)?,
        inputs,
        touch,
        core,
        title: None,
        description: None,
        image: None,
        position: None,
        priority,
    };
    let info = info_table(lua, &action, 0)?;
    with_state(lua, |s| s.actions.actions.push(action));
    fire_lua(
        lua,
        id,
        "BoundActionAdded",
        MultiValue::from_vec(vec![
            Value::String(lua.create_string(&name)?),
            Value::Boolean(touch),
            Value::Table(info),
        ]),
    );
    none()
}

fn bind_action(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    bind(lua, id, args, false)
}

fn bind_core_action(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    bind(lua, id, args, true)
}

fn priority_arg(lua: &Lua, args: &[Value]) -> i32 {
    match args.get(3) {
        Some(v) => crate::convert::arg_number(Some(v))
            .map(|n| n as i32)
            .or_else(|| {
                crate::convert::enum_value(lua, v, "ContextActionPriority")
                    .ok()
                    .map(|n| n as i32)
            })
            .unwrap_or(2000),
        None => 2000,
    }
}

fn bind_action_at_priority(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = priority_arg(lua, args);
    bind_at(lua, id, args, false, Some(p))
}

fn bind_core_action_at_priority(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = priority_arg(lua, args);
    bind_at(lua, id, args, true, Some(p))
}

fn get_all_bound_core_action_info(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let out = lua.create_table()?;
    with_state(lua, |s| -> Result<()> {
        for (i, a) in s.actions.actions.iter().enumerate() {
            if a.core {
                out.set(a.name.as_str(), info_table(lua, a, i)?)?;
            }
        }
        Ok(())
    })?;
    ret(lua, out)
}

fn unbind_action(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    remove(lua, id, &name, false)?;
    none()
}

fn unbind_core_action(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    remove(lua, id, &name, true)?;
    none()
}

fn unbind_all(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let names: Vec<String> = with_state(lua, |s| {
        s.actions
            .actions
            .iter()
            .filter(|a| !a.core)
            .map(|a| a.name.clone())
            .collect()
    });
    for n in names {
        remove(lua, id, &n, false)?;
    }
    none()
}

fn get_bound_action_info(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let t = with_state(lua, |s| {
        s.actions
            .actions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, a)| a.name == name && !a.core)
            .map(|(i, a)| info_table(lua, a, i))
    });
    match t {
        Some(t) => ret(lua, t?),
        None => ret(lua, lua.create_table()?),
    }
}

fn get_all_bound_action_info(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let out = lua.create_table()?;
    with_state(lua, |s| -> Result<()> {
        for (i, a) in s.actions.actions.iter().enumerate() {
            if !a.core {
                out.set(a.name.as_str(), info_table(lua, a, i)?)?;
            }
        }
        Ok(())
    })?;
    ret(lua, out)
}

fn update(lua: &Lua, id: InstanceId, args: &[Value], key: &str) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let value = args.get(1).cloned().unwrap_or(Value::Nil);
    let changed = with_state(lua, |s| {
        let a = s
            .actions
            .actions
            .iter_mut()
            .rev()
            .find(|a| a.name == name && !a.core)?;
        match key {
            "title" => a.title = arg_string(Some(&value)),
            "description" => a.description = arg_string(Some(&value)),
            "image" => a.image = arg_string(Some(&value)),
            "position" => {
                a.position = crate::types::LUDim2::from_lua(value.clone(), lua)
                    .ok()
                    .map(|u| u.0)
            }
            _ => {}
        }
        Some(())
    });
    if changed.is_some() {
        let t = lua.create_table()?;
        t.set(key, value)?;
        fire_lua(
            lua,
            id,
            "BoundActionChanged",
            MultiValue::from_vec(vec![
                Value::String(lua.create_string(&name)?),
                Value::String(lua.create_string(key)?),
                Value::Table(t),
            ]),
        );
    }
    none()
}

fn set_title(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    update(lua, id, args, "title")
}

fn set_description(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    update(lua, id, args, "description")
}

fn set_image(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    update(lua, id, args, "image")
}

fn set_position(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    update(lua, id, args, "position")
}

fn action_function(lua: &Lua, name: &str) -> Option<Function> {
    with_state(lua, |s| {
        s.actions
            .actions
            .iter()
            .rev()
            .find(|a| a.name == name)
            .and_then(|a| lua.registry_value::<Function>(&a.func).ok())
    })
}

fn call_function(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    if let Some(f) = action_function(lua, &name) {
        let mut call = vec![Value::String(lua.create_string(&name)?)];
        call.extend(args.iter().skip(1).cloned());
        crate::runtime::spawn_function(
            lua,
            f,
            MultiValue::from_vec(call),
            crate::runtime::current_info(lua),
        );
    }
    none()
}

fn get_button(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    fire_lua(
        lua,
        id,
        "GetActionButtonEvent",
        MultiValue::from_vec(vec![Value::String(lua.create_string(&name)?)]),
    );
    ret(lua, Value::Nil)
}

fn button_found(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn current_tool_icon(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}

fn ignore(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn player_action_matches(action: u32, key: u32) -> bool {
    matches!(
        (action, key),
        (0, 119) | (0, 273) | (1, 115) | (1, 274) | (2, 97) | (3, 100) | (4, 32)
    )
}

fn binding_matches(inputs: &[ActionInput], event: &ActionInput, input_type: u32) -> Option<bool> {
    let key = match event {
        ActionInput::KeyCode(k) => Some(*k),
        _ => None,
    };
    for i in inputs {
        match (i, key) {
            (ActionInput::KeyCode(k), Some(code)) if *k == code => return Some(true),
            (ActionInput::Character(c), Some(code)) => {
                let mut chars = c.chars();
                if let (Some(ch), None) = (chars.next(), chars.next())
                    && ch as u32 == code
                {
                    return Some(true);
                }
            }
            (ActionInput::PlayerAction(a), Some(code)) if player_action_matches(*a, code) => {
                return Some(false);
            }
            (ActionInput::InputType(t), _) if *t == input_type => return Some(true),
            _ => {}
        }
    }
    None
}

pub fn dispatch(
    lua: &Lua,
    input: &ActionInput,
    input_type: u32,
    state: u32,
    object: InstanceId,
    core: bool,
    menu_open: bool,
) -> bool {
    let target = with_state(lua, |s| {
        s.actions
            .actions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.core == core)
            .filter_map(|(i, a)| {
                binding_matches(&a.inputs, input, input_type).map(|sink| (i, a, sink))
            })
            .max_by_key(|(i, a, _)| (a.priority, *i))
            .and_then(|(_, a, sink)| {
                lua.registry_value::<Function>(&a.func)
                    .ok()
                    .map(|f| (a.name.clone(), f, sink))
            })
    });
    let Some((name, func, sink)) = target else {
        return false;
    };
    let state = if menu_open { 3 } else { state };
    let Ok(state_value) = variant_to_lua(
        lua,
        &Variant::Enum(state),
        Some(&ValueType::Enum("UserInputState".into())),
    ) else {
        return false;
    };
    let Ok(obj) = crate::instance::push_instance(lua, object) else {
        return false;
    };
    let Ok(name_value) = lua.create_string(&name) else {
        return false;
    };
    crate::runtime::spawn_function(
        lua,
        func,
        MultiValue::from_vec(vec![Value::String(name_value), state_value, obj]),
        crate::runtime::ThreadInfo {
            script: None,
            identity: if core {
                crate::security::Identity::RobloxGameScript
            } else {
                crate::security::Identity::GameScript
            },
        },
    );
    sink
}

fn get_topbar_transparency(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let t = engine(lua).lock().gui.topbar_transparency;
    ret(lua, t as f64)
}

fn set_topbar_transparency(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let t = arg_number(args.first()).unwrap_or(0.5).clamp(0.0, 1.0) as f32;
    let changed = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let changed = e.gui.topbar_transparency != t;
        e.gui.topbar_transparency = t;
        changed
    };
    if changed {
        fire_lua(
            lua,
            id,
            "TopbarTransparencyChangedSignal",
            MultiValue::from_vec(vec![Value::Number(t as f64)]),
        );
    }
    none()
}
