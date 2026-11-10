use super::bindables::invoke_callback;
use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, arg_string, enum_value};
use crate::runtime::{engine, with_state};
use crate::types::{LVector2, rt};
use mlua::{FromLua, Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_math::Vec2;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

pub struct GuiState {
    pub hidden_gui_types: HashSet<u32>,
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
    pub last_view: Option<rbx_dom::gui::GuiView>,
}

impl Default for GuiState {
    fn default() -> Self {
        GuiState {
            hidden_gui_types: HashSet::default(),
            topbar_transparency: 0.5,
            core_gui_disabled: HashSet::default(),
            keys: HashSet::default(),
            special_keys: HashSet::default(),
            error_message: String::new(),
            ui_message: (0, String::new()),
            menu_open: false,
            center_dialogs: Vec::new(),
            selection_groups: HashMap::default(),
            button_verbs: HashMap::default(),
            requested_verbs: Vec::new(),
            last_view: None,
        }
    }
}

pub fn register(r: &mut Registry) {
    r.add("TextService", &["GetTextSize"], get_text_size);
    r.add("GuiService", &["SetGlobalGuiInset"], set_global_gui_inset);
    r.add("GuiService", &["GetGuiInset"], get_gui_inset);
    r.add("GuiService", &["SetTopbarInset"], set_topbar_inset);
    r.add("GuiService", &["GetInsetArea"], get_inset_area);
    r.add("DataModel", &["IsLoaded"], is_loaded);
    r.add(
        "GuiService",
        &["GetNotificationTypeList"],
        notification_types,
    );
    r.add(
        "GuiService",
        &[
            "BroadcastNotification",
            "CloseInspectMenu",
            "SetPurchasePromptIsShown",
            "SetEmotesMenuOpen",
            "ClearError",
        ],
        ignore,
    );
    r.add(
        "GuiService",
        &["DismissNotification", "GetInspectMenuEnabled"],
        no_value,
    );
    r.add("GuiService", &["GetGuiIsVisible"], gui_is_visible);
    r.add(
        "GuiService",
        &[
            "ToggleGuiIsVisibleForCaptures",
            "ToggleGuiIsVisibleIfAllowed",
        ],
        toggle_gui_visible,
    );
    r.add("GuiService", &["GetSafeZoneOffsets"], safe_zone_offsets);
    r.add("GuiService", &["GetResolutionScale"], one_int);
    r.add("GuiService", &["GetRawScreenScale"], one_int);
    r.add(
        "GuiService",
        &["GetEffectiveUIScaleHundredths", "GetAutoUIScaleHundredths"],
        hundred,
    );
    r.add("GuiService", &["SetUIScaleMultiplier"], ignore);
    r.add(
        "GuiService",
        &["GetHardwareSafeViewport"],
        get_screen_resolution,
    );
    r.add(
        "GuiService",
        &["GetErrorCode", "GetErrorType"],
        connection_ok,
    );
    r.add("AdService", &["GetAdTeleportInfo"], no_ad_teleport);
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

fn get_gui_inset(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let i = engine(lua).lock().dm.gui_view.inset;
    ret(
        lua,
        (
            LVector2(Vec2::new(i[0], i[1])),
            LVector2(Vec2::new(i[2], i[3])),
        ),
    )
}

fn set_topbar_inset(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let rect = match args.first() {
        Some(v) => crate::types::LRect2D::from_lua(v.clone(), lua)?.0,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    engine(lua)
        .lock()
        .dm
        .set_unchecked(id, "TopbarInset", Variant::Rect2D(rect));
    none()
}

fn get_inset_area(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kind = args
        .first()
        .and_then(|v| enum_value(lua, v, "ScreenInsets").ok())
        .unwrap_or(0);
    let (screen, inset, topbar) = {
        let eng = engine(lua);
        let e = eng.lock();
        let topbar = match e.dm.get(id, "TopbarInset") {
            Ok(Variant::Rect2D(r)) => Some(r),
            _ => None,
        };
        (e.dm.gui_view.screen, e.dm.gui_view.inset, topbar)
    };
    let rect = match kind {
        2 => rbx_math::Rect2D::xyxy(inset[0], inset[1], screen.x - inset[2], screen.y - inset[3]),
        3 => topbar.unwrap_or_default(),
        _ => rbx_math::Rect2D::xyxy(0.0, 0.0, screen.x, screen.y),
    };
    ret(lua, crate::types::LRect2D(rect))
}

fn is_loaded(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, true)
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

pub fn sync_view(e: &mut crate::runtime::Engine) {
    let view = e.dm.gui_view;
    if e.gui.last_view == Some(view) {
        return;
    }
    let first = e.gui.last_view.is_none();
    e.gui.last_view = Some(view);
    if first {
        return;
    }
    let mut parents = Vec::new();
    if let Some(core) = e.dm.find_service("CoreGui") {
        parents.push(core);
    }
    if let Some(pg) =
        e.dm.local_player
            .and_then(|p| e.dm.find_first_child_of_class(p, "PlayerGui"))
    {
        parents.push(pg);
    }
    let mut ids = Vec::new();
    for parent in parents {
        for &c in e.dm.children_of(parent) {
            if matches!(e.dm.class_of(c), Some("ScreenGui" | "GuiMain")) {
                ids.push(c);
                ids.extend(
                    e.dm.descendants(c)
                        .into_iter()
                        .filter(|&d| e.dm.is_a(d, "GuiBase2d")),
                );
            }
        }
    }
    for id in ids {
        e.dm.notify_changed(id, "AbsoluteSize");
        e.dm.notify_changed(id, "AbsolutePosition");
    }
}

const NOTIFICATION_TYPES: &[(&str, i32)] = &[
    ("VIEW_PROFILE", 0),
    ("REPORT_ABUSE", 1),
    ("VIEW_GAME_DETAILS", 2),
    ("SHOW_TAB_BAR", 3),
    ("HIDE_TAB_BAR", 4),
    ("UNREAD_COUNT", 5),
    ("PRIVACY_SETTINGS", 6),
    ("BACK_BUTTON_NOT_CONSUMED", 7),
    ("PURCHASE_ROBUX", 8),
    ("VIEW_NOTIFICATIONS", 9),
    ("APP_READY", 10),
    ("CLOSE_MODAL", 11),
    ("VIEW_GAME_DETAILS_ANIMATED", 12),
    ("SEARCH_GAMES", 15),
    ("GAMES_SEE_ALL", 16),
    ("ACTION_LOG_OUT", 17),
    ("LAUNCH_CONVERSATION", 18),
    ("UNIVERSAL_FRIENDS", 19),
    ("OPEN_CUSTOM_WEBVIEW", 20),
    ("SET_STATUS_BAR_STYLE", 22),
    ("OPEN_YOUTUBE_VIDEO", 23),
    ("DID_LOG_OUT", 24),
    ("OPEN_BUILDERS_CLUB", 25),
    ("ACTION_LOG_IN", 26),
    ("OPEN_SETTINGS_WEBVIEW", 27),
    ("DID_LOG_IN", 28),
    ("OPEN_CAPTCHA_VIEW", 29),
    ("DID_SIGN_UP", 30),
    ("LUA_UNAUTHORIZED_LOG_OUT", 31),
    ("NATIVE_SHARE", 32),
    ("SDK_SHARE", 33),
    ("SDK_OPEN_URL", 34),
    ("NATIVE_EXIT", 35),
    ("RESTART_LUA_APP", 36),
    ("IDENTITY_VERIFICATION", 38),
    ("SCAN_QR_CODE", 39),
    ("SHOW_FRIEND_QR_CODE", 40),
    ("DID_SWITCH_ACCOUNT", 42),
    ("GAME_STREAMING_STATUS_CHANGED", 43),
    ("POST_PURCHASE", 44),
];

fn notification_types(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let t = lua.create_table()?;
    for (k, v) in NOTIFICATION_TYPES {
        t.set(*k, *v)?;
    }
    ret(lua, t)
}

fn no_value(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn gui_type(lua: &Lua, args: &[Value]) -> u32 {
    args.first()
        .and_then(|v| enum_value(lua, v, "GuiType").ok())
        .unwrap_or(0)
}

fn gui_is_visible(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let t = gui_type(lua, args);
    let hidden = engine(lua).lock().gui.hidden_gui_types.contains(&t);
    ret(lua, !hidden)
}

fn toggle_gui_visible(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let t = gui_type(lua, args);
    let eng = engine(lua);
    let mut e = eng.lock();
    if !e.gui.hidden_gui_types.remove(&t) {
        e.gui.hidden_gui_types.insert(t);
    }
    none()
}

fn safe_zone_offsets(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let t = lua.create_table()?;
    for k in ["top", "bottom", "left", "right"] {
        t.set(k, 0)?;
    }
    ret(lua, t)
}

fn one_int(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 1)
}

fn hundred(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 100)
}

fn connection_ok(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    match crate::types::LEnumItem::by_name("ConnectionError", "OK") {
        Some(i) => ret(lua, crate::types::enum_item_value(lua, i)?),
        None => ret(lua, 0),
    }
}

fn no_ad_teleport(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, (0, 0))
}
