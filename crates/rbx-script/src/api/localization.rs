use super::{Registry, Ret, none, ret};
use crate::convert::arg_string;
use crate::instance::push_instance;
use crate::runtime::{engine, instance_from_value, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Table, Value};
use rbx_dom::{InstanceId, Variant};
use serde_json::{Map, Value as Json};
use std::collections::HashMap;

struct Parsed {
    hash: u64,
    by_key: HashMap<String, std::sync::Arc<Json>>,
    by_source: HashMap<String, std::sync::Arc<Json>>,
}

#[derive(Default)]
pub struct LocalizationState {
    translators: HashMap<InstanceId, Vec<InstanceId>>,
    pub core_tables: Vec<InstanceId>,
    parsed: HashMap<InstanceId, Parsed>,
}

fn content_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn indexed(
    lua: &Lua,
    table: InstanceId,
    by_key: bool,
    needle: &str,
) -> Option<std::sync::Arc<Json>> {
    let text = engine(lua)
        .lock()
        .dm
        .get(table, "Contents")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let hash = content_hash(&text);
    let fresh = with_state(lua, |s| {
        s.localization
            .parsed
            .get(&table)
            .is_some_and(|p| p.hash == hash)
    });
    if !fresh {
        let mut parsed = Parsed {
            hash,
            by_key: HashMap::new(),
            by_source: HashMap::new(),
        };
        if let Ok(Json::Array(list)) = serde_json::from_str::<Json>(&text) {
            for e in list {
                let e = std::sync::Arc::new(e);
                if let Some(k) = e.get("key").and_then(Json::as_str) {
                    parsed
                        .by_key
                        .entry(k.to_string())
                        .or_insert_with(|| e.clone());
                }
                if let Some(src) = e.get("source").and_then(Json::as_str) {
                    parsed
                        .by_source
                        .entry(src.to_string())
                        .or_insert_with(|| e.clone());
                }
            }
        }
        with_state(lua, |s| s.localization.parsed.insert(table, parsed));
    }
    with_state(lua, |s| {
        let p = s.localization.parsed.get(&table)?;
        if by_key {
            p.by_key.get(needle).cloned()
        } else {
            p.by_source.get(needle).cloned()
        }
    })
}

pub fn register(r: &mut Registry) {
    r.add("LocalizationTable", &["SetEntries"], set_entries);
    r.add("LocalizationTable", &["GetTranslator"], get_translator);
    r.add("LocalizationTable", &["RemoveKey"], remove_key);
    r.add(
        "LocalizationTable",
        &["SetIsExemptFromUGCAnalytics"],
        set_exempt,
    );
    r.add("Translator", &["FormatByKey"], format_by_key);
    r.add(
        "Translator",
        &["Translate", "RobloxOnlyTranslate"],
        translate,
    );
    r.add(
        "LocalizationService",
        &["GetTranslatorForPlayer", "GetTranslatorForPlayerAsync"],
        translator_for_player,
    );
    r.add(
        "LocalizationService",
        &["GetIsLoadingInternalTranslations"],
        loading_internal,
    );
    r.add(
        "LocalizationService",
        &["IsLoadingInternalTranslationsSettingChanged"],
        internal_setting_changed,
    );
}

fn entries(lua: &Lua, table: InstanceId) -> Vec<Json> {
    let text = engine(lua)
        .lock()
        .dm
        .get(table, "Contents")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    match serde_json::from_str::<Json>(&text) {
        Ok(Json::Array(a)) => a,
        _ => Vec::new(),
    }
}

fn store(lua: &Lua, table: InstanceId, list: Vec<Json>) -> Result<()> {
    let text = Json::Array(list).to_string();
    engine(lua)
        .lock()
        .dm
        .set(table, "Contents", Variant::String(text))
        .map_err(|e| rt(e.to_string()))
}

fn lua_string(t: &Table, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| t.get::<Option<String>>(*k).ok().flatten())
}

fn set_entries(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(Value::Table(list)) = args.first() else {
        return Err(rt("SetEntries expects an array of entries"));
    };
    let mut out = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let entry = entry?;
        let mut obj = Map::new();
        for (field, keys) in [
            ("key", &["Key", "key"][..]),
            ("source", &["Source", "source"][..]),
            ("context", &["Context", "context"][..]),
            ("example", &["Example", "example"][..]),
        ] {
            if let Some(v) = lua_string(&entry, keys) {
                obj.insert(field.into(), Json::String(v));
            }
        }
        let mut values = Map::new();
        if let Some(Value::Table(v)) = ["Values", "values"]
            .iter()
            .find_map(|k| entry.get::<Value>(*k).ok().filter(|v| !v.is_nil()))
        {
            for pair in v.pairs::<String, String>() {
                let (locale, text) = pair?;
                values.insert(locale, Json::String(text));
            }
        }
        obj.insert("values".into(), Json::Object(values));
        out.push(Json::Object(obj));
    }
    store(lua, id, out)?;
    none()
}

fn remove_key(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).unwrap_or_default();
    let list: Vec<Json> = entries(lua, id)
        .into_iter()
        .filter(|e| e.get("key").and_then(Json::as_str) != Some(key.as_str()))
        .collect();
    store(lua, id, list)?;
    none()
}

fn set_exempt(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let v = matches!(args.first(), Some(Value::Boolean(true)));
    let _ = engine(lua)
        .lock()
        .dm
        .set(id, "IsExemptFromUGCAnalytics", Variant::Bool(v));
    none()
}

fn make_translator(lua: &Lua, locale: &str, tables: Vec<InstanceId>) -> Result<Ret> {
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let id =
            e.dm.create_internal("Translator")
                .map_err(|err| rt(err.to_string()))?;
        let _ =
            e.dm.set(id, "LocaleId", Variant::String(locale.to_string()));
        id
    };
    with_state(lua, |s| s.localization.translators.insert(id, tables));
    ret(lua, push_instance(lua, id)?)
}

fn get_translator(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let locale = arg_string(args.first()).unwrap_or_else(|| "en-us".into());
    make_translator(lua, &locale, vec![id])
}

fn roblox_locale(lua: &Lua) -> String {
    let eng = engine(lua);
    let e = eng.lock();
    e.dm.find_service("LocalizationService")
        .and_then(|s| e.dm.get(s, "RobloxLocaleId").ok())
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "en-us".into())
}

fn translator_for_player(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let locale = roblox_locale(lua);
    let mut tables = with_state(lua, |s| s.localization.core_tables.clone());
    {
        let eng = engine(lua);
        let e = eng.lock();
        if let Some(service) = e.dm.find_service("LocalizationService") {
            tables.extend(
                e.dm.descendants(service)
                    .into_iter()
                    .filter(|d| e.dm.is_a(*d, "LocalizationTable")),
            );
        }
    }
    make_translator(lua, &locale, tables)
}

fn loading_internal(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn internal_setting_changed(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn pick(values: &Map<String, Json>, locale: &str) -> Option<String> {
    let lang = locale.split('-').next().unwrap_or(locale);
    [locale, lang, "en-us", "en"]
        .iter()
        .find_map(|l| values.get(*l).and_then(Json::as_str))
        .map(str::to_string)
}

fn lookup(lua: &Lua, translator: InstanceId, by_key: bool, needle: &str) -> Option<String> {
    let locale = engine(lua)
        .lock()
        .dm
        .get(translator, "LocaleId")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "en-us".into());
    let tables = with_state(lua, |s| {
        s.localization
            .translators
            .get(&translator)
            .cloned()
            .unwrap_or_default()
    });
    for t in tables {
        if let Some(e) = indexed(lua, t, by_key, needle) {
            let values = e.get("values").and_then(Json::as_object);
            return values
                .and_then(|v| pick(v, &locale))
                .or_else(|| e.get("source").and_then(Json::as_str).map(str::to_string));
        }
    }
    None
}

fn arg_text(lua: &Lua, args: &Value, key: &str) -> Option<String> {
    let Value::Table(t) = args else {
        return None;
    };
    let v: Value = match key.parse::<i64>() {
        Ok(i) => t.get(i).ok()?,
        Err(_) => t.get(key).ok()?,
    };
    match v {
        Value::Nil => None,
        Value::String(s) => Some(s.to_string_lossy().to_string()),
        Value::Number(n) => Some(crate::format::lua_number_str(n)),
        Value::Integer(i) => Some(i.to_string()),
        other => lua
            .globals()
            .get::<mlua::Function>("tostring")
            .ok()?
            .call::<String>(other)
            .ok(),
    }
}

pub fn substitute(lua: &Lua, text: &str, args: &Value) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let inner = &after[..close];
                let name = inner.split(':').next().unwrap_or(inner).trim();
                match arg_text(lua, args, name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('{');
                        out.push_str(inner);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn format_by_key(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let text = lookup(lua, id, true, &key).ok_or_else(|| {
        rt(format!(
            "Translator.FormatByKey failed to find a translation for key: {key}"
        ))
    })?;
    let params = args.get(1).cloned().unwrap_or(Value::Nil);
    ret(lua, substitute(lua, &text, &params))
}

fn translate(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.get(1)).unwrap_or_default();
    let _context = args.first().and_then(|v| instance_from_value(lua, v));
    let found = lookup(lua, id, false, &text);
    ret(lua, found.unwrap_or(text))
}
