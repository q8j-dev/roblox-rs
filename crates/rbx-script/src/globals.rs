use crate::convert::arg_number;
use crate::instance::push_instance;
use crate::runtime::{
    MessageType, current_identity, current_info, engine, expect_instance, new_environment, pump,
    schedule_delay, schedule_wait, yield_sentinel,
};
use crate::types::{register_constructors, rt};
use mlua::{Function, Lua, MultiValue, Result, Table, Value};
use rbx_dom::database;

fn tostring_all(lua: &Lua, args: &MultiValue) -> Result<String> {
    let tostring: Function = lua.globals().get("tostring")?;
    let mut parts = Vec::with_capacity(args.len());
    for a in args.iter() {
        let s: String = tostring.call(a.clone())?;
        parts.push(s);
    }
    Ok(parts.join(" "))
}

fn wrap(lua: &Lua, f: Function) -> Result<Function> {
    let w: Function = lua.named_registry_value("rbx_wrap")?;
    w.call(f)
}

pub fn create_globals(lua: &Lua) -> Result<Table> {
    let g = lua.create_table()?;
    let base = lua.globals();
    for name in [
        "assert",
        "error",
        "getmetatable",
        "setmetatable",
        "ipairs",
        "pairs",
        "next",
        "pcall",
        "xpcall",
        "rawequal",
        "rawget",
        "rawset",
        "rawlen",
        "select",
        "tonumber",
        "tostring",
        "type",
        "typeof",
        "unpack",
        "newproxy",
        "gcinfo",
        "getfenv",
        "setfenv",
        "math",
        "string",
        "table",
        "coroutine",
        "os",
        "debug",
        "utf8",
        "bit32",
        "buffer",
        "vector",
        "_VERSION",
    ] {
        let v: Value = base.get(name)?;
        if !v.is_nil() {
            g.set(name, v)?;
        }
    }
    if g.get::<Value>("unpack")?.is_nil() {
        let table: Table = base.get("table")?;
        g.set("unpack", table.get::<Value>("unpack")?)?;
    }
    g.set("ypcall", base.get::<Value>("pcall")?)?;
    g.set("_G", lua.create_table()?)?;
    g.set("shared", lua.create_table()?)?;

    let base_co: Table = base.get("coroutine")?;
    let co = lua.create_table()?;
    for pair in base_co.pairs::<Value, Value>() {
        let (k, v) = pair?;
        co.raw_set(k, v)?;
    }
    let create = lua.create_function(|lua, f: Function| {
        let t = lua.create_thread(f)?;
        crate::runtime::record_thread_origin(lua, &t, &crate::runtime::current_info(lua))?;
        Ok(t)
    })?;
    co.raw_set("create", create.clone())?;
    let wrap_co: Function = lua
        .load(
            "local create, resume, pack, unpack = ...\n\
             return function(f)\n\
               local co = create(f)\n\
               return function(...)\n\
                 local r = pack(resume(co, ...))\n\
                 if not r[1] then error(r[2], 0) end\n\
                 return unpack(r, 2, r.n)\n\
               end\n\
             end",
        )
        .set_name("=coroutine")
        .call((
            create,
            base_co.get::<Function>("resume")?,
            base.get::<Table>("table")?.get::<Function>("pack")?,
            base.get::<Table>("table")?.get::<Function>("unpack")?,
        ))?;
    co.raw_set("wrap", wrap_co)?;
    g.set("coroutine", co)?;

    let os: Table = base.get("os")?;
    let safe_os = lua.create_table()?;
    for name in ["time", "clock", "difftime", "date"] {
        safe_os.set(name, os.get::<Value>(name)?)?;
    }
    g.set("os", safe_os)?;

    g.set(
        "collectgarbage",
        lua.create_function(|lua, (opt, _arg): (Option<String>, Option<Value>)| {
            match opt.as_deref().unwrap_or("collect") {
                "count" => Ok(lua.used_memory() as f64 / 1024.0),
                other => Err(rt(format!(
                    "collectgarbage must be called with 'count'; got '{other}'"
                ))),
            }
        })?,
    )?;

    g.set(
        "print",
        lua.create_function(|lua, args: MultiValue| {
            let text = tostring_all(lua, &args)?;
            engine(lua).lock().print(MessageType::Output, text);
            Ok(())
        })?,
    )?;
    g.set(
        "warn",
        lua.create_function(|lua, args: MultiValue| {
            let text = tostring_all(lua, &args)?;
            engine(lua).lock().print(MessageType::Warning, text);
            Ok(())
        })?,
    )?;
    g.set(
        "printidentity",
        lua.create_function(|lua, prefix: Option<String>| {
            let id = current_identity(lua);
            let text = format!(
                "{} {}",
                prefix.unwrap_or_else(|| "Current identity is".into()),
                id.level()
            );
            engine(lua).lock().print(MessageType::Output, text);
            Ok(())
        })?,
    )?;

    let wait_native = lua.create_function(|lua, t: Option<Value>| {
        let seconds = arg_number(t.as_ref()).unwrap_or(0.0);
        schedule_wait(lua, seconds);
        yield_sentinel(lua)
    })?;
    let wait = wrap(lua, wait_native)?;
    g.set("wait", wait.clone())?;
    g.set("Wait", wait)?;

    let delay = lua.create_function(|lua, (t, f): (Value, Value)| {
        let seconds = arg_number(Some(&t)).unwrap_or(0.0);
        let Value::Function(f) = f else {
            return Err(rt("delay expects a function"));
        };
        schedule_delay(lua, seconds, f)
    })?;
    g.set("task", crate::task::create(lua, |f| wrap(lua, f))?)?;
    g.set("delay", delay.clone())?;
    g.set("Delay", delay)?;
    let spawn = lua.create_function(|lua, f: Value| {
        let Value::Function(f) = f else {
            return Err(rt("Spawn function requires 1 argument"));
        };
        schedule_delay(lua, 0.0, f)
    })?;
    g.set("spawn", spawn.clone())?;
    g.set("Spawn", spawn)?;

    g.set(
        "tick",
        lua.create_function(|lua, ()| Ok(engine(lua).lock().tick()))?,
    )?;
    g.set(
        "time",
        lua.create_function(|lua, ()| Ok(engine(lua).lock().game_time))?,
    )?;
    let elapsed = lua.create_function(|lua, ()| Ok(engine(lua).lock().clock.elapsed()))?;
    g.set("elapsedTime", elapsed.clone())?;
    g.set("ElapsedTime", elapsed)?;
    let version =
        lua.create_function(|_, ()| Ok(format!("0.{}.0.0", env!("CARGO_PKG_VERSION_MINOR"))))?;
    g.set("version", version.clone())?;
    g.set("Version", version)?;

    let root = engine(lua).lock().dm.root();
    let game = push_instance(lua, root)?;
    g.set("game", game.clone())?;
    g.set("Game", game)?;
    let ws = {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.dm.get_service("Workspace").ok()
    };
    if let Some(ws) = ws {
        let w = push_instance(lua, ws)?;
        g.set("workspace", w.clone())?;
        g.set("Workspace", w)?;
    }

    let settings_fn = lua.create_function(|lua, ()| {
        let id = crate::api::services::settings_root(lua, "GlobalSettings")?;
        push_instance(lua, id)
    })?;
    g.set("settings", settings_fn)?;
    g.set(
        "PluginManager",
        lua.create_function(|lua, ()| {
            let id = crate::api::plugin::manager(lua)?;
            push_instance(lua, id)
        })?,
    )?;
    g.set(
        "UserSettings",
        lua.create_function(|lua, ()| {
            let id = crate::api::services::settings_root(lua, "UserSettings")?;
            push_instance(lua, id)
        })?,
    )?;
    let stats = lua.create_function(|lua, ()| {
        let id = {
            let eng = engine(lua);
            let mut e = eng.lock();
            crate::api::stats::ensure_tree(&mut e).ok_or_else(|| rt("Stats is not available"))?
        };
        push_instance(lua, id)
    })?;
    g.set("stats", stats.clone())?;
    g.set("Stats", stats)?;

    let instance = lua.create_table()?;
    instance.set(
        "new",
        lua.create_function(|lua, (class, parent): (String, Option<Value>)| {
            let id = {
                let eng = engine(lua);
                let mut e = eng.lock();
                let desc = database().class(&class);
                let ok = desc
                    .map(|d| d.creatable && d.functionality.is_script_creatable())
                    .unwrap_or(false);
                if !ok {
                    return Err(rt(format!(
                        "Unable to create an Instance of type \"{class}\""
                    )));
                }
                e.dm.create(&class).map_err(|e| rt(e.to_string()))?
            };
            let handle = push_instance(lua, id)?;
            if let Some(p) = parent
                && !p.is_nil()
            {
                let pid = expect_instance(lua, &p, "Object")?;
                let r = engine(lua).lock().dm.set_parent(id, Some(pid));
                r.map_err(|e| rt(e.to_string()))?;
                pump(lua);
            }
            Ok(handle)
        })?,
    )?;
    g.set("Instance", instance)?;

    let require_native = lua.create_function(|lua, m: Value| -> Result<Value> {
        crate::runtime::require_module(lua, m)
    })?;
    let wrap: Function = lua.named_registry_value("rbx_wrap")?;
    let require: Function = wrap.call(require_native)?;
    g.set("require", require)?;

    g.set(
        "loadstring",
        lua.create_function(
            |lua, (src, name): (String, Option<String>)| -> Result<MultiValue> {
                let enabled = {
                    let eng = engine(lua);
                    let e = eng.lock();
                    let sss = e.dm.find_service("ServerScriptService");
                    let flag = sss
                        .map(|s| e.dm.get_bool(s, "LoadStringEnabled"))
                        .unwrap_or(false);
                    flag || e.load_string_enabled
                };
                if !enabled {
                    return Err(rt("loadstring() is not available"));
                }
                let info = current_info(lua);
                let env = new_environment(lua, info.script)?;
                match lua
                    .load(&src)
                    .set_mode(mlua::chunk::ChunkMode::Text)
                    .set_name(format!("={}", name.unwrap_or_else(|| "[string]".into())))
                    .set_environment(env)
                    .into_function()
                {
                    Ok(f) => Ok(MultiValue::from_vec(vec![Value::Function(f)])),
                    Err(e) => Ok(MultiValue::from_vec(vec![
                        Value::Nil,
                        Value::String(lua.create_string(crate::runtime::format_error(&e))?),
                    ])),
                }
            },
        )?,
    )?;

    g.set(
        "LoadLibrary",
        lua.create_function(|lua, name: String| crate::libraries::load_library(lua, &name))?,
    )?;

    register_constructors(lua, &g)?;
    crate::datatypes::register(lua, &g)?;
    crate::signal::register(lua)?;
    let wrapper: Function = lua.named_registry_value("rbx_wrap")?;
    for name in [
        "Vector3",
        "Vector2",
        "Vector3int16",
        "Vector2int16",
        "CFrame",
        "Color3",
        "BrickColor",
        "UDim",
        "UDim2",
        "Ray",
        "Region3",
        "Region3int16",
        "Rect",
        "Faces",
        "Axes",
        "NumberRange",
        "NumberSequence",
        "NumberSequenceKeypoint",
        "ColorSequence",
        "ColorSequenceKeypoint",
        "PhysicalProperties",
        "Font",
        "Path2DControlPoint",
        "DateTime",
        "TweenInfo",
        "RaycastParams",
        "Content",
        "Random",
        "Instance",
    ] {
        if let Value::Table(t) = g.get::<Value>(name)? {
            let mut entries = Vec::new();
            for pair in t.pairs::<Value, Value>() {
                let (k, v) = pair?;
                if let Value::Function(f) = v {
                    entries.push((k, f));
                }
            }
            for (k, f) in entries {
                t.raw_set(k, wrapper.call::<Function>(f)?)?;
            }
            t.set_readonly(true);
        }
    }
    for name in [
        "print",
        "warn",
        "printidentity",
        "delay",
        "Delay",
        "spawn",
        "Spawn",
        "settings",
        "UserSettings",
        "PluginManager",
        "stats",
        "Stats",
        "loadstring",
        "LoadLibrary",
    ] {
        if let Value::Function(f) = g.get::<Value>(name)? {
            g.raw_set(name, wrapper.call::<Function>(f)?)?;
        }
    }
    for lib in [
        "math",
        "string",
        "table",
        "coroutine",
        "os",
        "debug",
        "utf8",
        "bit32",
        "buffer",
        "vector",
    ] {
        if let Value::Table(t) = g.get::<Value>(lib)? {
            t.set_readonly(true);
        }
    }
    let string_mt: Table = lua.load("return getmetatable('')").eval()?;
    string_mt.raw_set("__metatable", "The metatable is locked")?;
    string_mt.set_readonly(true);
    g.set_readonly(true);
    Ok(g)
}
