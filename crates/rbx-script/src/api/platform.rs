use super::{Registry, Ret, ret};
use crate::types::{LEnumItem, enum_item_value};
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

pub fn register(r: &mut Registry) {
    r.add(
        "ThirdPartyUserService",
        &[
            "IsSingleSignOnSupported",
            "IsChatRestrictionSupported",
            "IsAccountSwitchingSupported",
            "HaveActiveUser",
        ],
        unsupported,
    );
    r.add(
        "ThirdPartyUserService",
        &["GetUserPlatformName"],
        platform_name,
    );
    r.add(
        "ThirdPartyUserService",
        &["GetVoiceChatRestrictionStatus"],
        not_restricted,
    );
    r.add(
        "AvatarChatService",
        &["GetClientFeaturesAsync", "PollClientFeatures"],
        no_features,
    );
    r.add("AvatarChatService", &["IsEnabled"], feature_enabled);
    r.add("DataModel", &["IsPioneerApp"], unsupported);
    r.add("DataModel", &["GetPioneerRootPlaceId"], no_features);
    r.add("DataModel", &["GetPioneerSource"], pioneer_source);
    r.add(
        "AvatarCreationService",
        &["GetValidationRules"],
        validation_rules,
    );
    r.add("VideoCaptureService", &["GetCameraDevices"], empty_map);
    r.add(
        "UserGameSettings",
        &["GetDefaultFramerateCap"],
        default_framerate_cap,
    );
    r.add("Chat", &["RequestModerationModeEnabled"], moderation_mode);
    r.add(
        "TextChatService",
        &["IsProtectedChatEnabled", "HasAllocatedUniverseChatContext"],
        unsupported,
    );
    r.add(
        "Player",
        &["SetChatTranslationSettingsLocaleId"],
        set_translation_locale,
    );
    r.add(
        "AdService",
        &["SetAdGuiInteractivityHandlerInitialized"],
        nothing,
    );
    r.add("TextChatService", &["SendExpChatLoadSuccess"], nothing);
    r.add(
        "UGCValidationService",
        &["RegisterUGCValidationFunction"],
        register_validation_function,
    );
    r.add(
        "TeleportService",
        &["GetThirdPartyTeleportInfo"],
        third_party_teleport,
    );
}

fn default_framerate_cap(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 60)
}

fn register_validation_function(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = crate::convert::arg_string(args.first()).unwrap_or_default();
    if let Some(Value::Function(f)) = args.get(1) {
        let table = match lua.named_registry_value::<Value>("rbx_ugc_validation")? {
            Value::Table(t) => t,
            _ => {
                let t = lua.create_table()?;
                lua.set_named_registry_value("rbx_ugc_validation", &t)?;
                t
            }
        };
        table.set(name, f.clone())?;
    }
    let _ = id;
    ret(lua, Value::Nil)
}

fn nothing(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}

fn moderation_mode(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let on = matches!(args.first(), Some(Value::Boolean(true)));
    let changed = {
        let eng = crate::runtime::engine(lua);
        let mut e = eng.lock();
        let was = e.dm.get_bool(id, "ModerationMode");
        let _ = e.dm.set(id, "ModerationMode", rbx_dom::Variant::Bool(on));
        was != on
    };
    if changed {
        crate::runtime::fire_lua(
            lua,
            id,
            "ModerationModeEnabledChanged",
            mlua::MultiValue::from_vec(vec![Value::Boolean(on)]),
        );
    }
    ret(lua, Value::Nil)
}

fn set_translation_locale(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let locale = crate::convert::arg_string(args.first()).unwrap_or_default();
    {
        let eng = crate::runtime::engine(lua);
        let mut e = eng.lock();
        if let Some(settings) = e.dm.find_service("UserGameSettings") {
            let _ = e.dm.set(
                settings,
                "ChatTranslationLocale",
                rbx_dom::Variant::String(locale.clone()),
            );
        }
    }
    crate::runtime::fire_lua(
        lua,
        id,
        "PlayerChatTranslationSettingsLocaleSetFromLua",
        mlua::MultiValue::from_vec(vec![Value::String(lua.create_string(&locale)?)]),
    );
    ret(lua, Value::Nil)
}

fn empty_map(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn third_party_teleport(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0)
}

fn decode(lua: &Lua, v: &serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    let nums = |a: &J| -> Vec<f64> {
        a.as_array()
            .map(|a| a.iter().filter_map(J::as_f64).collect())
            .unwrap_or_default()
    };
    let at = |n: &[f64], i: usize| n.get(i).copied().unwrap_or(0.0) as f32;
    Ok(match v {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(_) => Value::Nil,
        J::Object(o) => {
            if let Some(entries) = o.get("$Table").and_then(J::as_array) {
                let t = lua.create_table()?;
                for e in entries {
                    if let Some([k, x]) = e.as_array().map(Vec::as_slice) {
                        t.raw_set(decode(lua, k)?, decode(lua, x)?)?;
                    }
                }
                Value::Table(t)
            } else if let Some(a) = o.get("$Vector3") {
                let n = nums(a);
                Value::Vector(mlua::Vector::new(
                    n.first().copied().unwrap_or(0.0) as f32,
                    n.get(1).copied().unwrap_or(0.0) as f32,
                    n.get(2).copied().unwrap_or(0.0) as f32,
                ))
            } else if let Some(a) = o.get("$Vector2") {
                let n = nums(a);
                crate::convert::variant_to_lua(
                    lua,
                    &rbx_dom::Variant::Vector2(rbx_math::Vec2::new(at(&n, 0), at(&n, 1))),
                    None,
                )?
            } else if let Some(a) = o.get("$Color3") {
                let n = nums(a);
                crate::convert::variant_to_lua(
                    lua,
                    &rbx_dom::Variant::Color3(rbx_math::Color3 {
                        r: at(&n, 0),
                        g: at(&n, 1),
                        b: at(&n, 2),
                    }),
                    None,
                )?
            } else if let Some(name) = o.get("$Enum").and_then(J::as_str) {
                match name
                    .split_once('.')
                    .and_then(|(e, i)| LEnumItem::by_name(e, i))
                {
                    Some(item) => enum_item_value(lua, item)?,
                    None => Value::Nil,
                }
            } else {
                Value::Nil
            }
        }
    })
}

fn validation_rules(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let dir = crate::runtime::engine(lua)
        .lock()
        .core_package
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let json = dir
        .and_then(|d| rbx_base::vfs::read_to_string(&d.join("ValidationRules.json")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let value = decode(lua, &json)?;
    match value {
        Value::Table(_) => ret(lua, value),
        _ => ret(lua, lua.create_table()?),
    }
}

fn pioneer_source(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    match LEnumItem::by_name("PioneerSource", "Roblox") {
        Some(item) => ret(lua, enum_item_value(lua, item)?),
        None => ret(lua, Value::Nil),
    }
}

fn no_features(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0)
}

fn feature_enabled(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mask = crate::convert::arg_number(args.first()).unwrap_or(0.0) as i64;
    let bit = args
        .get(1)
        .and_then(|v| mlua::FromLua::from_lua(v.clone(), lua).ok())
        .map(|e: LEnumItem| e.value)
        .unwrap_or(0);
    ret(lua, bit < 63 && mask & (1i64 << bit) != 0)
}

fn unsupported(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn platform_name(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}

fn not_restricted(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    match LEnumItem::by_name("ChatRestrictionStatus", "NotRestricted") {
        Some(item) => ret(lua, enum_item_value(lua, item)?),
        None => ret(lua, Value::Nil),
    }
}
