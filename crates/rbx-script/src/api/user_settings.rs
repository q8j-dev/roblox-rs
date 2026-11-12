use crate::runtime::{Engine, engine, with_state};
use mlua::{Lua, MultiValue, Value};
use rbx_dom::{InstanceId, Variant};
use serde_json::{Map, Value as Json};

#[derive(Default)]
pub struct UserSettingsState {
    saved: Option<String>,
    countdown: u32,
}

const MIRRORED: [(&str, &str); 3] = [
    ("PreferredTransparency", "PreferredTransparency"),
    ("PreferredTextSize", "PreferredTextSize"),
    ("ReducedMotion", "ReducedMotionEnabled"),
];

fn to_json(v: &Variant) -> Option<Json> {
    Some(match v {
        Variant::Bool(b) => Json::Bool(*b),
        Variant::Int(i) => Json::from(*i),
        Variant::Int64(i) => Json::from(*i),
        Variant::Float(f) => serde_json::Number::from_f64(*f as f64).map(Json::Number)?,
        Variant::Double(f) => serde_json::Number::from_f64(*f).map(Json::Number)?,
        Variant::String(s) => Json::String(s.clone()),
        Variant::Enum(e) => Json::from(*e),
        _ => return None,
    })
}

fn from_json(current: &Variant, j: &Json) -> Option<Variant> {
    Some(match current {
        Variant::Bool(_) => Variant::Bool(j.as_bool()?),
        Variant::Int(_) => Variant::Int(j.as_i64()? as i32),
        Variant::Int64(_) => Variant::Int64(j.as_i64()?),
        Variant::Float(_) => Variant::Float(j.as_f64()? as f32),
        Variant::Double(_) => Variant::Double(j.as_f64()?),
        Variant::String(_) => Variant::String(j.as_str()?.to_string()),
        Variant::Enum(_) => Variant::Enum(j.as_u64()? as u32),
        _ => return None,
    })
}

fn property_names(e: &Engine, id: InstanceId) -> Vec<String> {
    let Some(class) = e.dm.class_of(id) else {
        return Vec::new();
    };
    let db = rbx_reflection::database();
    let mut out = Vec::new();
    for c in db.superclasses(class) {
        if c.name == "Instance" {
            break;
        }
        for p in &c.properties {
            if !p.read_only && !out.contains(&p.name) {
                out.push(p.name.clone());
            }
        }
    }
    out
}

fn snapshot(e: &Engine, id: InstanceId) -> String {
    let mut map = Map::new();
    for name in property_names(e, id) {
        if let Ok(v) = e.dm.get(id, &name)
            && let Some(j) = to_json(&v)
        {
            map.insert(name, j);
        }
    }
    Json::Object(map).to_string()
}

pub fn attach(lua: &Lua, id: InstanceId) {
    let eng = engine(lua);
    let mut e = eng.lock();
    e.user_game_settings = Some(id);
    let Some(path) = e.settings_path.clone() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(Json::Object(map)) = serde_json::from_str::<Json>(&text) else {
        return;
    };
    for (name, j) in map {
        if let Ok(current) = e.dm.get(id, &name)
            && let Some(v) = from_json(&current, &j)
        {
            let _ = e.dm.set(id, &name, v);
        }
    }
    let saved = snapshot(&e, id);
    drop(e);
    with_state(lua, |s| s.user_settings.saved = Some(saved));
}

pub fn sync(lua: &Lua) {
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(id) = e.user_game_settings.filter(|i| e.dm.contains(*i)) else {
        return;
    };
    if let Some(gui) = e.dm.find_service("GuiService") {
        for (from, to) in MIRRORED {
            if let Ok(v) = e.dm.get(id, from)
                && e.dm.get(gui, to).ok().as_ref() != Some(&v)
                && e.dm.property_descriptor(gui, to).is_some()
            {
                e.dm.set_unchecked(gui, to, v);
                e.dm.notify_changed(gui, to);
            }
        }
    }
    let Some(path) = e.settings_path.clone() else {
        return;
    };
    let due = with_state(lua, |s| {
        if s.user_settings.countdown == 0 {
            s.user_settings.countdown = 30;
            true
        } else {
            s.user_settings.countdown -= 1;
            false
        }
    });
    if !due {
        return;
    }
    let now = snapshot(&e, id);
    drop(e);
    let changed = with_state(lua, |s| {
        s.user_settings.saved.as_deref() != Some(now.as_str())
    });
    if changed {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, &now).is_ok() {
            with_state(lua, |s| s.user_settings.saved = Some(now));
        }
    }
}

pub fn set_fullscreen(lua: &Lua, on: bool) {
    let id = {
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.fullscreen == on {
            return;
        }
        e.fullscreen = on;
        e.user_game_settings
    };
    if let Some(id) = id {
        crate::runtime::fire_lua(
            lua,
            id,
            "FullscreenChanged",
            MultiValue::from_vec(vec![Value::Boolean(on)]),
        );
        crate::runtime::pump(lua);
    }
}
