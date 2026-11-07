use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

static FLAGS: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

static DEFAULTS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    if let Ok(Value::Object(obj)) =
        serde_json::from_str::<Value>(include_str!("../data/flags.json"))
    {
        for (k, v) in obj {
            if let Some(d) = v.get("default").and_then(Value::as_str) {
                map.insert(k, d.to_string());
            }
        }
    }
    for (k, v) in ENGINE_VALUES {
        map.insert((*k).to_string(), (*v).to_string());
    }
    map
});

const ENGINE_VALUES: &[(&str, &str)] = &[
    ("UsePGSSolver", "true"),
    ("PGSUsesConstraintBasedBodyMovers", "true"),
    ("PGSWakePrimitivesWithBodyMoverPropertyChanges", "true"),
    ("MaterialPropertiesEnabled", "true"),
    ("FixTouchEndedReporting", "true"),
    ("UseInGameTopBar", "true"),
    ("LuaBasedBubbleChat", "true"),
    ("NewInGameDevConsole", "true"),
    ("EnableSetCoreTopbarEnabled", "true"),
    ("SetCoreMoveChat", "true"),
    ("SetCoreDisableChatBar", "true"),
    ("SetCoreSendNotifications", "true"),
    ("SetCoreDisableNotifications", "true"),
    ("PlayerDropDownEnabled", "true"),
    ("UseUserListMenu", "true"),
    ("AllowHideHudShortcut", "true"),
    ("MobileToggleChatVisibleIcon", "true"),
    ("UserServerFollowers", "true"),
    ("FilteringEnabledDialogFix", "true"),
    ("CustomEmitterRenderEnabled", "true"),
];

pub fn flag_value(name: &str) -> Option<String> {
    let name = strip_prefix(name);
    if let Some(v) = FLAGS.read().get(name) {
        return Some(v.clone());
    }
    DEFAULTS.get(name).cloned()
}

fn strip_prefix(name: &str) -> &str {
    for p in [
        "DFFlag", "DFInt", "DFString", "DFLog", "FFlag", "FInt", "FString", "FLog", "SFFlag",
    ] {
        if let Some(rest) = name.strip_prefix(p) {
            return rest;
        }
    }
    name
}

pub fn set_flag(name: &str, value: &str) {
    FLAGS
        .write()
        .insert(strip_prefix(name).to_string(), value.to_string());
}

pub fn flag_bool(name: &str, default: bool) -> bool {
    match flag_value(name).map(|s| s.to_ascii_lowercase()) {
        Some(v) if v == "true" || v == "1" => true,
        Some(v) if v == "false" || v == "0" => false,
        _ => default,
    }
}

pub fn flag_int(name: &str, default: i64) -> i64 {
    flag_value(name)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

pub fn flag_string(name: &str, default: &str) -> String {
    flag_value(name).unwrap_or_else(|| default.to_string())
}

pub fn load_flags_json(text: &str) -> Result<usize, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Value::Object(map) = value else {
        return Err("flags file must contain a JSON object".to_string());
    };
    let mut count = 0;
    for (k, v) in map {
        let s = match v {
            Value::String(s) => s,
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            other => other.to_string(),
        };
        set_flag(&k, &s);
        count += 1;
    }
    Ok(count)
}

pub fn load_flags_file(path: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    load_flags_json(&text)
}

pub fn all_values() -> Vec<(String, String)> {
    let mut map: HashMap<String, String> = DEFAULTS.clone();
    for (k, v) in FLAGS.read().iter() {
        map.insert(k.clone(), v.clone());
    }
    let mut v: Vec<(String, String)> = map.into_iter().collect();
    v.sort();
    v
}

pub fn all_flags() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = FLAGS
        .read()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_override_defaults() {
        assert!(flag_bool("TestOnlyFlagA", true));
        load_flags_json(r#"{"FFlagTestOnlyFlagA": false, "DFIntTestOnlyNumber": 7}"#).ok();
        assert!(!flag_bool("TestOnlyFlagA", true));
        assert_eq!(flag_int("TestOnlyNumber", 1), 7);
    }
}
