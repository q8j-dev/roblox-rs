use crate::runtime::{engine, new_environment, with_state};
use crate::types::rt;
use mlua::{Function, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};

#[derive(Clone, Debug)]
pub struct LLibrary {
    pub name: String,
}

fn library_table(lua: &Lua, name: &str) -> Result<Table> {
    let libs: Table = lua.named_registry_value("rbx_libraries")?;
    match libs.raw_get::<Value>(name)? {
        Value::Table(t) => Ok(t),
        Value::String(s) => Err(rt(s.to_str()?.to_string())),
        _ => Err(rt(format!("Unknown library {name}"))),
    }
}

impl UserData for LLibrary {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(
            MetaMethod::Index,
            |lua, this, key: String| -> Result<Value> {
                match key.as_str() {
                    "Name" => Ok(Value::String(lua.create_string(&this.name)?)),
                    "GetApi" => {
                        let name = this.name.clone();
                        Ok(Value::Function(lua.create_function(
                            move |lua, _: Value| {
                                let t = library_table(lua, &name)?;
                                let out = lua.create_table()?;
                                let mut keys: Vec<String> = Vec::new();
                                for pair in t.pairs::<Value, Value>() {
                                    let (k, _) = pair?;
                                    if let Value::String(s) = k {
                                        let s = s.to_str()?.to_string();
                                        if !s.is_empty() {
                                            keys.push(s);
                                        }
                                    }
                                }
                                keys.sort();
                                for (i, k) in keys.into_iter().enumerate() {
                                    out.raw_set(i + 1, k)?;
                                }
                                Ok(out)
                            },
                        )?))
                    }
                    _ => library_table(lua, &this.name)?.get(key),
                }
            },
        );
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> {
                Err(rt(format!("{k} cannot be assigned to")))
            },
        );
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.name.clone()));
    }
}

pub fn load_library(lua: &Lua, name: &str) -> Result<Value> {
    if lua.named_registry_value::<Value>("rbx_libraries")?.is_nil() {
        lua.set_named_registry_value("rbx_libraries", lua.create_table()?)?;
    }
    let libs: Table = lua.named_registry_value("rbx_libraries")?;
    match libs.raw_get::<Value>(name)? {
        Value::Table(_) => {
            return Ok(Value::UserData(lua.create_userdata(LLibrary {
                name: name.to_string(),
            })?));
        }
        Value::String(s) => return Err(rt(s.to_str()?.to_string())),
        _ => {}
    }
    let root = engine(lua).lock().content_root.clone();
    let Some(root) = root else {
        return Err(rt(format!("Unknown library {name}")));
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(rt(format!("Unknown library {name}")));
    }
    let path = root
        .join("scripts")
        .join("Libraries")
        .join(format!("{name}.lua"));
    let Ok(source) = rbx_base::vfs::read_to_string(&path) else {
        return Err(rt(format!("Unknown library {name}")));
    };
    let env = new_environment(lua, None)?;
    let func: Function = lua
        .load(&source)
        .set_mode(mlua::chunk::ChunkMode::Text)
        .set_name(format!("={name}"))
        .set_environment(env)
        .into_function()?;
    with_state(lua, |s| {
        s.current.push(crate::runtime::ThreadInfo {
            script: None,
            identity: crate::security::Identity::RobloxGameScript,
        });
    });
    let result = func.call::<mlua::MultiValue>(());
    with_state(lua, |s| {
        s.current.pop();
    });
    let result = result?;
    let message = if result.len() != 1 {
        Some("Libraries should return exactly 1 result, and shouldn't wait")
    } else if let Some(Value::Table(t)) = result.front() {
        match t.raw_get::<Value>("Help")? {
            Value::Function(_) => None,
            _ => Some("Libraries must contain a \"Help\" function"),
        }
    } else {
        Some("Libraries should return exactly 1 table")
    };
    match message {
        Some(m) => {
            libs.raw_set(name, m)?;
            Err(rt(m))
        }
        None => {
            libs.raw_set(name, result.front().cloned().unwrap_or(Value::Nil))?;
            Ok(Value::UserData(lua.create_userdata(LLibrary {
                name: name.to_string(),
            })?))
        }
    }
}
