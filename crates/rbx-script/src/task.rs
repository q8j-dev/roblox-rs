use crate::runtime::{
    Sleeper, WakeArgs, current_info, engine, resume_thread, thread_key, with_state, yield_sentinel,
};
use crate::types::rt;
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Result, Table, Thread, Value};

fn adopt(lua: &Lua, func: Function) -> Result<Thread> {
    let info = current_info(lua);
    let thread = lua.create_thread(func)?;
    with_state(lua, |s| {
        s.threads.insert(thread_key(&thread), info.clone());
        if let Some(script) = info.script {
            s.script_threads
                .entry(script)
                .or_default()
                .push(thread.clone());
        }
    });
    Ok(thread)
}

fn target(lua: &Lua, v: Value, what: &str) -> Result<Thread> {
    match v {
        Value::Function(f) => adopt(lua, f),
        Value::Thread(t) => Ok(t),
        _ => Err(rt(format!(
            "invalid argument #1 to '{what}' (function or thread expected)"
        ))),
    }
}

fn sleep(lua: &Lua, thread: Thread, seconds: f64, resume: WakeArgs) {
    let now = engine(lua).lock().game_time;
    with_state(lua, |s| {
        s.sleepers.push(Sleeper {
            wake: now + seconds.max(0.0),
            start: now,
            thread,
            resume,
        });
    });
}

pub fn create(lua: &Lua, wrap: impl Fn(Function) -> Result<Function>) -> Result<Table> {
    let task = lua.create_table()?;
    task.set(
        "spawn",
        lua.create_function(|lua, (v, args): (Value, MultiValue)| {
            let thread = target(lua, v, "spawn")?;
            resume_thread(lua, &thread, args);
            Ok(thread)
        })?,
    )?;
    task.set(
        "defer",
        lua.create_function(|lua, (v, args): (Value, MultiValue)| {
            let thread = target(lua, v, "defer")?;
            with_state(lua, |s| s.deferred.push((thread.clone(), args)));
            Ok(thread)
        })?,
    )?;
    task.set(
        "delay",
        lua.create_function(|lua, (t, v, args): (Option<f64>, Value, MultiValue)| {
            let thread = target(lua, v, "delay")?;
            sleep(lua, thread.clone(), t.unwrap_or(0.0), WakeArgs::Args(args));
            Ok(thread)
        })?,
    )?;
    let wait = lua.create_function(|lua, t: Option<f64>| {
        sleep(
            lua,
            lua.current_thread(),
            t.unwrap_or(0.0),
            WakeArgs::Elapsed,
        );
        yield_sentinel(lua)
    })?;
    task.set("wait", wrap(wait)?)?;
    task.set(
        "cancel",
        lua.create_function(|lua, thread: Thread| {
            let key = thread_key(&thread);
            with_state(lua, |s| {
                s.sleepers.retain(|x| thread_key(&x.thread) != key);
                s.deferred.retain(|(t, _)| thread_key(t) != key);
            });
            if thread.status() == ThreadStatus::Resumable {
                let coroutine: Table = lua.globals().get("coroutine")?;
                let close: Function = coroutine.get("close")?;
                close.call::<MultiValue>(thread)?;
            }
            Ok(())
        })?,
    )?;
    task.set_readonly(true);
    Ok(task)
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Identity, ScriptRuntime};
    use parking_lot::Mutex;
    use rbx_dom::DataModel;
    use std::sync::Arc;

    #[test]
    fn task_library_orders_spawn_defer_and_delay() {
        let mut engine = Engine::new(DataModel::new());
        engine.running = true;
        let rt = ScriptRuntime::new(Arc::new(Mutex::new(engine))).expect("runtime");
        rt.execute(
            r#"
local log = {}
task.defer(function() table.insert(log, "defer") end)
task.spawn(function(x) table.insert(log, "spawn " .. x) end, 1)
task.delay(0.05, function(y) table.insert(log, "delay " .. y) end, 2)
local cancelled = task.delay(0.05, function() table.insert(log, "cancelled") end)
task.cancel(cancelled)
task.spawn(function()
    local dt = task.wait(0.1)
    table.insert(log, "waited " .. tostring(dt >= 0.1))
    print(table.concat(log, ","))
end)
"#,
            "Test",
            Identity::GameScript,
        )
        .expect("execute");
        for _ in 0..20 {
            rt.step(1.0 / 60.0);
        }
        let out: Vec<String> = rt
            .engine
            .lock()
            .output
            .iter()
            .map(|m| m.text.clone())
            .collect();
        assert!(
            out.iter().any(|t| t == "spawn 1,defer,delay 2,waited true"),
            "{out:?}"
        );
    }
}
