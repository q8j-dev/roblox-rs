use super::bindables::invoke_callback;
use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, arg_string, enum_value};
use crate::runtime::{engine, with_state};
use crate::types::{LVector2, rt};
use mlua::{FromLua, Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_math::Vec2;
use std::collections::{HashMap, HashSet};

pub struct GuiState {
    pub topbar_transparency: f32,
    pub core_gui_disabled: HashSet<u32>,
    pub keys: HashSet<String>,
    pub special_keys: HashSet<u32>,
    pub error_message: String,
    pub ui_message: (u32, String),
    pub menu_open: bool,
    pub center_dialogs: Vec<InstanceId>,
    pub selection_groups: HashMap<String, Vec<InstanceId>>,
    pub button_verbs: HashMap<InstanceId, String>,
    pub requested_verbs: Vec<String>,
}

impl Default for GuiState {
    fn default() -> Self {
        GuiState {
            topbar_transparency: 0.5,
            core_gui_disabled: HashSet::new(),
            keys: HashSet::new(),
            special_keys: HashSet::new(),
            error_message: String::new(),
            ui_message: (0, String::new()),
            menu_open: false,
            center_dialogs: Vec::new(),
            selection_groups: HashMap::new(),
            button_verbs: HashMap::new(),
            requested_verbs: Vec::new(),
        }
    }
}

pub fn register(r: &mut Registry) {
    r.add("TextService", &["GetTextSize"], get_text_size);
    r.add("GuiService", &["SetGlobalGuiInset"], set_global_gui_inset);
    r.add(
        "GuiService",
        &["GetScreenResolution"],
        get_screen_resolution,
    );
    r.add("GuiService", &["IsTenFootInterface"], is_ten_foot);
    r.add("GuiService", &["GetBrickCount"], brick_count);
    r.add("GuiService", &["AddKey"], add_key);
    r.add("GuiService", &["RemoveKey"], remove_key);
    r.add("GuiService", &["AddSpecialKey"], add_special_key);
    r.add("GuiService", &["RemoveSpecialKey"], remove_special_key);
    r.add("GuiService", &["SetMenuIsOpen"], set_menu_open);
    r.add("GuiService", &["SetErrorMessage"], set_error_message);
    r.add("GuiService", &["GetErrorMessage"], get_error_message);
    r.add("GuiService", &["SetUiMessage"], set_ui_message);
    r.add("GuiService", &["GetUiMessage"], get_ui_message);
    r.add("GuiService", &["AddCenterDialog"], add_center_dialog);
    r.add("GuiService", &["RemoveCenterDialog"], remove_center_dialog);
    r.add("GuiService", &["OpenBrowserWindow"], ignore);
    r.add("GuiService", &["ToggleFullscreen"], ignore);
    r.add("GuiService", &["ShowStatsBasedOnInputString"], ignore);
    r.add("GuiService", &["AddSelectionParent"], add_selection_parent);
    r.add("GuiService", &["AddSelectionTuple"], add_selection_tuple);
    r.add(
        "GuiService",
        &["RemoveSelectionGroup"],
        remove_selection_group,
    );
    r.add(
        "GuiService",
        &["GetClosestDialogToPosition"],
        closest_dialog,
    );
    r.add("StarterGui", &["SetCoreGuiEnabled"], set_core_gui_enabled);
    r.add("StarterGui", &["GetCoreGuiEnabled"], get_core_gui_enabled);
    r.add("StarterGui", &["RegisterSetCore"], register_set_core);
    r.add("StarterGui", &["RegisterGetCore"], register_get_core);
    r.add("StarterGui", &["SetCore"], set_core);
    r.add("StarterGui", &["GetCore"], get_core);
}

fn vector2(lua: &Lua, v: Vec2) -> Result<Ret> {
    ret(lua, LVector2(v))
}

pub fn text_font(dm: &rbx_dom::DataModel, id: InstanceId) -> rbx_text::Font {
    match dm.get(id, "Font") {
        Ok(Variant::Enum(v)) => {
            rbx_text::Font::from_enum(v as i64).unwrap_or(rbx_text::Font::Legacy)
        }
        _ => rbx_text::Font::Legacy,
    }
}

pub fn text_size(dm: &rbx_dom::DataModel, id: InstanceId) -> f32 {
    match dm.get(id, "FontSize") {
        Ok(Variant::Enum(v)) => rbx_text::font_size(v as i64),
        _ => 12.0,
    }
}

fn text_wrapped(dm: &rbx_dom::DataModel, id: InstanceId) -> bool {
    matches!(dm.get(id, "TextWrapped"), Ok(Variant::Bool(true)))
        || matches!(dm.get(id, "TextWrap"), Ok(Variant::Bool(true)))
}

fn text_measure(dm: &rbx_dom::DataModel, id: InstanceId) -> ([f32; 2], bool) {
    let text = match dm.get(id, "Text") {
        Ok(Variant::String(s)) => s,
        _ => String::new(),
    };
    let rect = rbx_dom::gui::absolute_size(dm, id);
    let available = if text_wrapped(dm, id) {
        [rect.x, rect.y]
    } else {
        [0.0, 0.0]
    };
    let (size, fits) =
        rbx_text::typesetter(text_font(dm, id)).measure(&text, text_size(dm, id), available);
    let fits = fits && size[0] <= rect.x && size[1] <= rect.y;
    (size, fits)
}

pub fn get_gui_property(
    lua: &Lua,
    id: InstanceId,
    class: &str,
    name: &str,
) -> Result<Option<Value>> {
    let value = {
        let eng = engine(lua);
        let e = eng.lock();
        let dm = &e.dm;
        match name {
            "AbsolutePosition" if dm.is_a(id, "GuiBase2d") => {
                Some(Variant::Vector2(rbx_dom::gui::absolute_position(dm, id)))
            }
            "AbsoluteSize" if dm.is_a(id, "GuiBase2d") => {
                Some(Variant::Vector2(rbx_dom::gui::absolute_size(dm, id)))
            }
            "AbsoluteWindowSize" if class == "ScrollingFrame" => {
                Some(Variant::Vector2(rbx_dom::gui::absolute_window_size(dm, id)))
            }
            "TextBounds" if matches!(class, "TextLabel" | "TextButton" | "TextBox") => {
                if dm.frontend_processing {
                    let (size, _) = text_measure(dm, id);
                    Some(Variant::Vector2(Vec2::new(size[0], size[1])))
                } else {
                    Some(Variant::Vector2(Vec2::ZERO))
                }
            }
            "TextFits" if matches!(class, "TextLabel" | "TextButton" | "TextBox") => Some(
                Variant::Bool(dm.frontend_processing && text_measure(dm, id).1),
            ),
            "ViewportSize" if class == "Camera" => Some(Variant::Vector2(dm.gui_view.screen)),
            _ => None,
        }
    };
    match value {
        Some(Variant::Vector2(v)) => Ok(Some(Value::UserData(lua.create_userdata(LVector2(v))?))),
        Some(Variant::Bool(b)) => Ok(Some(Value::Boolean(b))),
        _ => Ok(None),
    }
}

fn get_text_size(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let size = arg_number(args.get(1)).unwrap_or(12.0) as f32;
    let font = match args.get(2) {
        Some(v) => enum_value(lua, v, "Font")?,
        None => 0,
    };
    let frame = match args.get(3) {
        Some(v) => LVector2::from_lua(v.clone(), lua)
            .map(|v| v.0)
            .unwrap_or(Vec2::ZERO),
        None => Vec2::ZERO,
    };
    let Some(font) = rbx_text::Font::from_enum(font as i64) else {
        return vector2(lua, Vec2::ZERO);
    };
    let (out, _) = rbx_text::typesetter(font).measure(&text, size, [frame.x, frame.y]);
    vector2(lua, Vec2::new(out[0], out[1]))
}

fn set_global_gui_inset(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let v: Vec<f32> = (0..4)
        .map(|i| arg_number(args.get(i)).unwrap_or(0.0) as f32)
        .collect();
    let eng = engine(lua);
    eng.lock().dm.gui_view.inset = [v[0], v[1], v[2], v[3]];
    none()
}

fn get_screen_resolution(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let screen = engine(lua).lock().dm.gui_view.screen;
    vector2(lua, screen)
}

fn is_ten_foot(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn brick_count(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let e = eng.lock();
    let count =
        e.dm.find_service("Workspace")
            .map(|ws| {
                e.dm.descendants(ws)
                    .into_iter()
                    .filter(|d| e.dm.is_a(*d, "BasePart"))
                    .count()
            })
            .unwrap_or(0);
    ret(lua, count as i64)
}

fn add_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).unwrap_or_default();
    if key.chars().count() != 1 {
        return Err(rt(
            "GuiService:AddKey requires a string with a single character",
        ));
    }
    engine(lua).lock().gui.keys.insert(key);
    none()
}

fn remove_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).unwrap_or_default();
    if key.chars().count() != 1 {
        return Err(rt(
            "GuiService:RemoveKey requires a string with a single character",
        ));
    }
    engine(lua).lock().gui.keys.remove(&key);
    none()
}

fn add_special_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = match args.first() {
        Some(v) => enum_value(lua, v, "SpecialKey")?,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    engine(lua).lock().gui.special_keys.insert(key);
    none()
}

fn remove_special_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = match args.first() {
        Some(v) => enum_value(lua, v, "SpecialKey")?,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    engine(lua).lock().gui.special_keys.remove(&key);
    none()
}

fn set_menu_open(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let open = arg_bool(args.first());
    let changed = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let changed = e.gui.menu_open != open;
        e.gui.menu_open = open;
        changed
    };
    if changed {
        let event = if open { "MenuOpened" } else { "MenuClosed" };
        crate::runtime::fire_lua(lua, id, event, MultiValue::new());
    }
    none()
}

fn set_error_message(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let msg = arg_string(args.first()).unwrap_or_default();
    engine(lua).lock().gui.error_message = msg.clone();
    crate::runtime::fire_lua(
        lua,
        id,
        "ErrorMessageChanged",
        MultiValue::from_vec(vec![Value::String(lua.create_string(&msg)?)]),
    );
    none()
}

fn get_error_message(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let msg = engine(lua).lock().gui.error_message.clone();
    ret(lua, msg)
}

fn set_ui_message(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kind = match args.first() {
        Some(v) => enum_value(lua, v, "UiMessageType")?,
        None => 0,
    };
    let msg = arg_string(args.get(1)).unwrap_or_default();
    engine(lua).lock().gui.ui_message = (kind, msg.clone());
    let kind_value = crate::convert::variant_to_lua(
        lua,
        &Variant::Enum(kind),
        Some(&rbx_reflection::ValueType::Enum("UiMessageType".into())),
    )?;
    crate::runtime::fire_lua(
        lua,
        id,
        "UiMessageChanged",
        MultiValue::from_vec(vec![kind_value, Value::String(lua.create_string(&msg)?)]),
    );
    none()
}

fn get_ui_message(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let msg = engine(lua).lock().gui.ui_message.1.clone();
    ret(lua, msg)
}

fn add_center_dialog(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(dialog) = crate::convert::arg_instance(lua, args.first()) else {
        return Err(rt("Argument 1 missing or nil"));
    };
    let show = match args.get(2) {
        Some(Value::Function(f)) => Some(f.clone()),
        _ => None,
    };
    engine(lua).lock().gui.center_dialogs.push(dialog);
    if let Some(f) = show {
        let _: Result<MultiValue> = f.call(());
    }
    none()
}

fn remove_center_dialog(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    if let Some(dialog) = crate::convert::arg_instance(lua, args.first()) {
        engine(lua)
            .lock()
            .gui
            .center_dialogs
            .retain(|d| *d != dialog);
    }
    none()
}

fn ignore(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn add_selection_parent(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let parent = crate::convert::arg_instance(lua, args.get(1));
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.gui.selection_groups.contains_key(&name) {
        return Err(rt(format!(
            "GuiService:AddSelectionParent: selection group {name} already exists"
        )));
    }
    e.gui
        .selection_groups
        .insert(name, parent.into_iter().collect());
    none()
}

fn add_selection_tuple(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let items: Vec<InstanceId> = args
        .iter()
        .skip(1)
        .filter_map(|v| crate::convert::arg_instance(lua, Some(v)))
        .collect();
    engine(lua).lock().gui.selection_groups.insert(name, items);
    none()
}

fn remove_selection_group(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    engine(lua).lock().gui.selection_groups.remove(&name);
    none()
}

fn closest_dialog(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}

fn core_gui_type(lua: &Lua, args: &[Value]) -> Result<u32> {
    match args.first() {
        Some(v) => enum_value(lua, v, "CoreGuiType"),
        None => Err(rt("Argument 1 missing or nil")),
    }
}

fn set_core_gui_enabled(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kind = core_gui_type(lua, args)?;
    let enabled = arg_bool(args.get(1));
    let all = rbx_reflection::database()
        .enum_desc("CoreGuiType")
        .and_then(|d| d.items.iter().find(|i| i.name == "All").map(|i| i.value));
    let kinds: Vec<u32> = if Some(kind) == all {
        rbx_reflection::database()
            .enum_desc("CoreGuiType")
            .map(|d| {
                d.items
                    .iter()
                    .map(|i| i.value)
                    .filter(|v| Some(*v) != all)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![kind]
    };
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        for k in &kinds {
            if enabled {
                e.gui.core_gui_disabled.remove(k);
            } else {
                e.gui.core_gui_disabled.insert(*k);
            }
        }
    }
    let kind_value = crate::convert::variant_to_lua(
        lua,
        &Variant::Enum(kind),
        Some(&rbx_reflection::ValueType::Enum("CoreGuiType".into())),
    )?;
    crate::runtime::fire_lua(
        lua,
        id,
        "CoreGuiChangedSignal",
        MultiValue::from_vec(vec![kind_value, Value::Boolean(enabled)]),
    );
    none()
}

fn get_core_gui_enabled(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kind = core_gui_type(lua, args)?;
    let disabled = engine(lua).lock().gui.core_gui_disabled.contains(&kind);
    ret(lua, !disabled)
}

fn register_core(lua: &Lua, id: InstanceId, args: &[Value], prefix: &str) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let key = format!("{prefix}{name}");
    match args.get(1) {
        Some(Value::Function(f)) => {
            let reg = lua.create_registry_value(f.clone())?;
            with_state(lua, |s| {
                s.callbacks.insert((id, key), reg);
            });
        }
        _ => {
            with_state(lua, |s| {
                s.callbacks.remove(&(id, key));
            });
        }
    }
    none()
}

fn register_set_core(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    register_core(lua, id, args, "SetCore:")
}

fn register_get_core(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    register_core(lua, id, args, "GetCore:")
}

fn set_core(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let value = args.get(1).cloned().unwrap_or(Value::Nil);
    invoke_callback(
        lua,
        id,
        &format!("SetCore:{name}"),
        MultiValue::from_vec(vec![value]),
        &format!("{name} has not been registered by the CoreScripts"),
    )
}

fn get_core(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let key = format!("GetCore:{name}");
    let registered = with_state(lua, |s| s.callbacks.contains_key(&(id, key.clone())));
    if !registered {
        return Err(rt(format!(
            "{name} has not been registered by the CoreScripts"
        )));
    }
    invoke_callback(lua, id, &key, MultiValue::new(), "")
}
