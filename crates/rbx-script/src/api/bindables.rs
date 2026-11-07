use super::{Registry, Ret, none};
use crate::runtime::{ThreadInfo, current_info, fire_lua, thread_key, with_state};
use crate::types::rt;
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Result, Value};
use rbx_dom::InstanceId;

pub fn copy_value(lua: &Lua, v: &Value, depth: u32) -> Result<Value> {
    if depth > 64 {
        return Err(rt("tables cannot be cyclic"));
    }
    Ok(match v {
        Value::Table(t) => {
            let out = lua.create_table()?;
            for pair in t.clone().pairs::<Value, Value>() {
                let (k, val) = pair?;
                out.raw_set(
                    copy_value(lua, &k, depth + 1)?,
                    copy_value(lua, &val, depth + 1)?,
                )?;
            }
            Value::Table(out)
        }
        other => other.clone(),
    })
}

pub fn copy_args(lua: &Lua, args: &[Value]) -> Result<MultiValue> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        out.push(copy_value(lua, a, 0)?);
    }
    Ok(MultiValue::from_vec(out))
}

fn fire(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let args = copy_args(lua, args)?;
    fire_lua(lua, id, "Event", args);
    none()
}

pub fn callback(lua: &Lua, id: InstanceId, name: &str) -> Option<Function> {
    with_state(lua, |s| {
        s.callbacks
            .get(&(id, name.to_string()))
            .and_then(|k| lua.registry_value::<Function>(k).ok())
    })
}

pub fn invoke_callback(
    lua: &Lua,
    id: InstanceId,
    name: &str,
    args: MultiValue,
    missing: &str,
) -> Result<Ret> {
    let caller = lua.current_thread();
    let Some(func) = callback(lua, id, name) else {
        if name == "OnInvoke" {
            with_state(lua, |s| {
                s.pending_invokes
                    .push((id, name.to_string(), caller, args.into_vec()))
            });
            return Ok(Ret::Yield);
        }
        return Err(rt(missing.to_string()));
    };
    let thread = lua.create_thread(func)?;
    let info = current_info(lua);
    with_state(lua, |s| {
        s.threads.insert(
            thread_key(&thread),
            ThreadInfo {
                script: info.script,
                identity: info.identity,
            },
        );
    });
    let result = thread.resume::<MultiValue>(args);
    match result {
        Ok(values) => {
            if thread.status() == ThreadStatus::Resumable {
                with_state(lua, |s| {
                    s.joins.insert(thread_key(&thread), caller);
                });
                let _ = values;
                Ok(Ret::Yield)
            } else {
                with_state(lua, |s| {
                    s.threads.remove(&thread_key(&thread));
                });
                Ok(Ret::Values(copy_args(lua, &values.into_vec())?))
            }
        }
        Err(e) => {
            with_state(lua, |s| {
                s.threads.remove(&thread_key(&thread));
            });
            Err(rt(crate::runtime::format_error(&e)))
        }
    }
}

fn invoke(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let args = copy_args(lua, args)?;
    invoke_callback(lua, id, "OnInvoke", args, "OnInvoke callback not set")
}

pub fn set_callback(lua: &Lua, id: InstanceId, name: &str, value: Value) -> Result<()> {
    match value {
        Value::Function(f) => {
            let key = lua.create_registry_value(f.clone())?;
            with_state(lua, |s| {
                s.callbacks.insert((id, name.to_string()), key);
            });
            let pending: Vec<(mlua::Thread, Vec<Value>)> = with_state(lua, |s| {
                let mut out = Vec::new();
                let mut i = 0;
                while i < s.pending_invokes.len() {
                    if s.pending_invokes[i].0 == id && s.pending_invokes[i].1 == name {
                        let (_, _, caller, args) = s.pending_invokes.remove(i);
                        out.push((caller, args));
                    } else {
                        i += 1;
                    }
                }
                out
            });
            for (caller, args) in pending {
                run_pending(lua, &f, caller, args)?;
            }
            Ok(())
        }
        Value::Nil => {
            with_state(lua, |s| {
                s.callbacks.remove(&(id, name.to_string()));
            });
            Ok(())
        }
        _ => Err(rt(format!("{name} must be set to a function"))),
    }
}

fn run_pending(lua: &Lua, func: &Function, caller: mlua::Thread, args: Vec<Value>) -> Result<()> {
    let thread = lua.create_thread(func.clone())?;
    let info = with_state(lua, |s| s.threads.get(&thread_key(&caller)).cloned())
        .unwrap_or_else(|| current_info(lua));
    with_state(lua, |s| {
        s.threads.insert(thread_key(&thread), info);
    });
    match thread.resume::<MultiValue>(MultiValue::from_vec(args)) {
        Ok(values) => {
            if thread.status() == ThreadStatus::Resumable {
                with_state(lua, |s| {
                    s.joins.insert(thread_key(&thread), caller);
                });
            } else {
                with_state(lua, |s| {
                    s.threads.remove(&thread_key(&thread));
                });
                let values = copy_args(lua, &values.into_vec())?;
                crate::runtime::make_ready_lua(lua, caller, values);
            }
        }
        Err(e) => {
            let msg = crate::runtime::format_error(&e);
            with_state(lua, |s| {
                s.threads.remove(&thread_key(&thread));
                s.ready
                    .push_back((caller, crate::runtime::Resume::Error(msg)));
            });
        }
    }
    Ok(())
}

pub fn register(r: &mut Registry) {
    r.add("BindableEvent", &["Fire"], fire);
    r.add("BindableFunction", &["Invoke"], invoke);
}
