use crate::convert::variant_to_lua;
use crate::instance::{LInstance, push_instance};
use crate::security::Identity;
use crate::types::rt;
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, RegistryKey, Table, Thread, Value};
use parking_lot::Mutex;
use rbx_base::Stopwatch;
use rbx_dom::{DataModel, DomEvent, EventCursor, InstanceId, Variant, database};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    Output,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct OutputMessage {
    pub kind: MessageType,
    pub text: String,
    pub time: f64,
}

#[derive(Clone, Debug)]
pub struct Fired {
    pub target: InstanceId,
    pub event: &'static str,
    pub args: Vec<Variant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RunMode {
    #[default]
    Standalone,
    Server,
    Client,
}

pub struct Engine {
    pub dm: DataModel,
    pub cursor: EventCursor,
    pub output: Vec<OutputMessage>,
    pub fired: VecDeque<Fired>,
    pub mode: RunMode,
    pub local_player: Option<InstanceId>,
    pub clock: Stopwatch,
    pub game_time: f64,
    pub running: bool,
    pub load_string_enabled: bool,
    pub hooks: Vec<Box<dyn EngineHook>>,
    pub content_root: Option<std::path::PathBuf>,
    pub test: crate::api::test::TestState,
}

pub trait EngineHook: Send {
    fn name(&self) -> &'static str;
    fn as_any(&mut self) -> &mut dyn std::any::Any;
}

impl Engine {
    pub fn new(mut dm: DataModel) -> Engine {
        let cursor = dm.subscribe();
        Engine {
            dm,
            cursor,
            output: Vec::new(),
            fired: VecDeque::new(),
            mode: RunMode::Standalone,
            local_player: None,
            clock: Stopwatch::new(),
            game_time: 0.0,
            running: false,
            load_string_enabled: false,
            hooks: Vec::new(),
            content_root: None,
            test: Default::default(),
        }
    }

    pub fn print(&mut self, kind: MessageType, text: impl Into<String>) {
        let text = text.into();
        match kind {
            MessageType::Error => tracing::error!("{text}"),
            MessageType::Warning => tracing::warn!("{text}"),
            _ => tracing::info!("{text}"),
        }
        self.output.push(OutputMessage {
            kind,
            text,
            time: self.game_time,
        });
    }

    pub fn fire(&mut self, target: InstanceId, event: &'static str, args: Vec<Variant>) {
        self.fired.push_back(Fired {
            target,
            event,
            args,
        });
    }

    pub fn backend_processing(&self) -> bool {
        self.dm.find_service("NetworkClient").is_none()
    }

    pub fn frontend_processing(&self) -> bool {
        self.dm.find_service("NetworkServer").is_none()
    }
}

pub type Shared = Arc<Mutex<Engine>>;

pub struct Conn {
    pub target: InstanceId,
    pub event: &'static str,
    pub func: Function,
    pub script: Option<InstanceId>,
    pub identity: Identity,
}

pub struct Sleeper {
    pub wake: f64,
    pub start: f64,
    pub thread: Thread,
}

#[derive(Clone)]
pub struct ThreadInfo {
    pub script: Option<InstanceId>,
    pub identity: Identity,
}

pub enum Resume {
    Values(Vec<Variant>),
    Lua(MultiValue),
    Error(String),
}

pub struct ChildWaiter {
    pub parent: InstanceId,
    pub name: String,
    pub thread: Thread,
}

pub struct State {
    pub engine: Shared,
    pub release: Arc<Mutex<Vec<InstanceId>>>,
    pub conns: HashMap<u64, Conn>,
    pub by_event: HashMap<(InstanceId, &'static str), Vec<u64>>,
    pub next_conn: u64,
    pub waiters: HashMap<(InstanceId, &'static str), Vec<Thread>>,
    pub sleepers: Vec<Sleeper>,
    pub ready: VecDeque<(Thread, Resume)>,
    pub threads: HashMap<usize, ThreadInfo>,
    pub script_threads: HashMap<InstanceId, Vec<Thread>>,
    pub running_scripts: HashMap<InstanceId, bool>,
    pub modules: HashMap<InstanceId, RegistryKey>,
    pub child_waiters: Vec<ChildWaiter>,
    pub depth: u32,
    pub globals: Option<RegistryKey>,
    pub instance_cache: Option<RegistryKey>,
    pub method_cache: Option<RegistryKey>,
    pub current: Vec<ThreadInfo>,
    pub finish_hooks: HashMap<usize, u32>,
    pub joins: HashMap<usize, Thread>,
    pub callbacks: HashMap<(InstanceId, String), RegistryKey>,
}

static SEED: AtomicU64 = AtomicU64::new(0x2545_f491_4f6c_dd1d);

pub fn random_seed() -> u64 {
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    x
}

pub fn engine(lua: &Lua) -> Shared {
    match lua.app_data_ref::<State>() {
        Some(s) => s.engine.clone(),
        None => panic!("script runtime state is missing"),
    }
}

pub fn with_state<R>(lua: &Lua, f: impl FnOnce(&mut State) -> R) -> R {
    match lua.app_data_mut::<State>() {
        Some(mut s) => f(&mut s),
        None => panic!("script runtime state is missing"),
    }
}

pub fn thread_key(t: &Thread) -> usize {
    t.to_pointer() as usize
}

pub fn current_info(lua: &Lua) -> ThreadInfo {
    let t = lua.current_thread();
    with_state(lua, |s| {
        s.threads
            .get(&thread_key(&t))
            .cloned()
            .or_else(|| s.current.last().cloned())
            .unwrap_or(ThreadInfo {
                script: None,
                identity: Identity::GameScript,
            })
    })
}

pub fn current_identity(lua: &Lua) -> Identity {
    current_info(lua).identity
}

pub fn yield_sentinel(lua: &Lua) -> mlua::Result<Value> {
    lua.named_registry_value("rbx_yield")
}

pub struct ScriptRuntime {
    pub lua: Lua,
    pub engine: Shared,
}

const PRELUDE: &str = r#"
local yield = coroutine.yield
local pcall = pcall
local error = error
local select = select
local type = type
local tostring = tostring
local SENTINEL = ...
local function clean(e)
    if type(e) ~= "string" then
        e = tostring(e)
    end
    local i = string.find(e, "\nstack traceback:", 1, true)
    if i then
        e = string.sub(e, 1, i - 1)
    end
    e = string.gsub(e, "^callback error: ", "")
    e = string.gsub(e, "^runtime error: ", "")
    return e
end
local function wrap(native)
    return function(...)
        local r = table.pack(pcall(native, ...))
        if not r[1] then
            error(clean(r[2]), 2)
        end
        if r[2] == SENTINEL then
            return yield()
        end
        return table.unpack(r, 2, r.n)
    end
end
local function wrap_index(native)
    return function(self, key)
        local ok, v = pcall(native, self, key)
        if not ok then
            error(clean(v), 2)
        end
        return v
    end
end
local function wrap_newindex(native)
    return function(self, key, value)
        local ok, v = pcall(native, self, key, value)
        if not ok then
            error(clean(v), 2)
        end
    end
end
return wrap, wrap_index, wrap_newindex, clean
"#;

impl ScriptRuntime {
    pub fn new(engine: Shared) -> mlua::Result<ScriptRuntime> {
        let lua = Lua::new();
        lua.set_app_data(State {
            engine: engine.clone(),
            release: Arc::new(Mutex::new(Vec::new())),
            conns: HashMap::new(),
            by_event: HashMap::new(),
            next_conn: 1,
            waiters: HashMap::new(),
            sleepers: Vec::new(),
            ready: VecDeque::new(),
            threads: HashMap::new(),
            script_threads: HashMap::new(),
            running_scripts: HashMap::new(),
            modules: HashMap::new(),
            child_waiters: Vec::new(),
            depth: 0,
            globals: None,
            instance_cache: None,
            method_cache: None,
            current: Vec::new(),
            finish_hooks: HashMap::new(),
            joins: HashMap::new(),
            callbacks: HashMap::new(),
        });
        let sentinel = lua.create_table()?;
        lua.set_named_registry_value("rbx_yield", sentinel.clone())?;
        let (wrap, wrap_index, wrap_newindex, clean): (Function, Function, Function, Function) =
            lua.load(PRELUDE).set_name("=prelude").call(sentinel)?;
        lua.set_named_registry_value("rbx_wrap", wrap)?;
        lua.set_named_registry_value("rbx_clean", clean)?;
        let index = lua.create_function(|lua, (ud, key): (mlua::AnyUserData, Value)| {
            let id = ud.borrow::<LInstance>()?.id;
            crate::instance::index(lua, id, key)
        })?;
        let newindex =
            lua.create_function(|lua, (ud, key, value): (mlua::AnyUserData, Value, Value)| {
                let id = ud.borrow::<LInstance>()?.id;
                crate::instance::newindex(lua, id, key, value)
            })?;
        lua.set_named_registry_value("rbx_instance_index", wrap_index.call::<Function>(index)?)?;
        lua.set_named_registry_value(
            "rbx_instance_newindex",
            wrap_newindex.call::<Function>(newindex)?,
        )?;
        let cache = lua.create_table()?;
        let mt = lua.create_table()?;
        mt.set("__mode", "v")?;
        cache.set_metatable(Some(mt))?;
        let cache_key = lua.create_registry_value(cache)?;
        let methods = lua.create_registry_value(lua.create_table()?)?;
        with_state(&lua, |s| {
            s.instance_cache = Some(cache_key);
            s.method_cache = Some(methods);
        });
        let globals = crate::globals::create_globals(&lua)?;
        let gkey = lua.create_registry_value(globals)?;
        with_state(&lua, |s| s.globals = Some(gkey));
        Ok(ScriptRuntime { lua, engine })
    }

    pub fn globals(&self) -> mlua::Result<Table> {
        globals_table(&self.lua)
    }

    pub fn execute(&self, source: &str, name: &str, identity: Identity) -> mlua::Result<()> {
        let env = new_environment(&self.lua, None)?;
        let func = self
            .lua
            .load(source)
            .set_name(format!("={name}"))
            .set_environment(env)
            .into_function()?;
        let thread = self.lua.create_thread(func)?;
        with_state(&self.lua, |s| {
            s.threads.insert(
                thread_key(&thread),
                ThreadInfo {
                    script: None,
                    identity,
                },
            );
        });
        resume_thread(&self.lua, &thread, MultiValue::new());
        pump(&self.lua);
        Ok(())
    }

    pub fn step(&self, dt: f64) {
        {
            let eng = self.engine.clone();
            let mut e = eng.lock();
            e.game_time += dt;
        }
        pump(&self.lua);
        resume_sleepers(&self.lua);
        run_ready(&self.lua);
        pump(&self.lua);
        crate::api::test::on_timeout(&self.lua);
        self.collect();
    }

    pub fn run_ready(&self) {
        run_ready(&self.lua);
    }

    pub fn pump(&self) {
        pump(&self.lua);
    }

    pub fn collect(&self) {
        let released: Vec<InstanceId> =
            with_state(&self.lua, |s| std::mem::take(&mut *s.release.lock()));
        let eng = self.engine.clone();
        let mut e = eng.lock();
        for id in released {
            e.dm.release(id);
        }
    }

    pub fn scan_scripts(&self) {
        scan_scripts(&self.lua);
        pump(&self.lua);
    }

    pub fn fire(&self, target: InstanceId, event: &'static str, args: Vec<Variant>) {
        self.engine.lock().fire(target, event, args);
        pump(&self.lua);
    }

    pub fn running_thread_count(&self) -> usize {
        with_state(&self.lua, |s| s.threads.len())
    }
}

pub fn globals_table(lua: &Lua) -> mlua::Result<Table> {
    let key = with_state(lua, |s| {
        s.globals.as_ref().map(|k| lua.registry_value::<Table>(k))
    });
    match key {
        Some(t) => t,
        None => Ok(lua.globals()),
    }
}

pub fn new_environment(lua: &Lua, script: Option<InstanceId>) -> mlua::Result<Table> {
    let globals = globals_table(lua)?;
    let env = lua.create_table()?;
    let mt = lua.create_table()?;
    mt.set("__index", globals)?;
    env.set_metatable(Some(mt))?;
    match script {
        Some(id) => env.set("script", push_instance(lua, id)?)?,
        None => env.set("script", Value::Nil)?,
    }
    Ok(env)
}

pub fn report_error(lua: &Lua, err: &mlua::Error) {
    let text = format_error(err);
    let eng = engine(lua);
    eng.lock().print(MessageType::Error, text);
}

pub fn format_error(err: &mlua::Error) -> String {
    match err {
        mlua::Error::CallbackError { cause, traceback } => {
            let inner = format_error(cause);
            let _ = traceback;
            inner
        }
        mlua::Error::RuntimeError(s) => s.clone(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::WithContext { cause, .. } => format_error(cause),
        other => other.to_string(),
    }
}

pub fn resume_thread(lua: &Lua, thread: &Thread, args: MultiValue) {
    if thread.status() != ThreadStatus::Resumable {
        return;
    }
    let info = with_state(lua, |s| s.threads.get(&thread_key(thread)).cloned());
    if let Some(i) = &info
        && let Some(script) = i.script
    {
        let alive = with_state(lua, |s| {
            s.running_scripts.get(&script).copied().unwrap_or(false)
        });
        if !alive {
            return;
        }
    }
    let result = thread.resume::<MultiValue>(args);
    let key = thread_key(thread);
    let mut error_text = None;
    if let Err(e) = &result {
        let mut text = format_error(e);
        if let Some(info) = &info
            && let Some(script) = info.script
        {
            let eng = engine(lua);
            let name = eng.lock().dm.full_name(script);
            if !text.contains(&name) {
                text = format!("{name}: {text}");
            }
        }
        let eng = engine(lua);
        eng.lock().print(MessageType::Error, text.clone());
        error_text = Some(text);
    }
    if thread.status() != ThreadStatus::Resumable {
        let (hook, join) = with_state(lua, |s| {
            s.threads.remove(&key);
            (s.finish_hooks.remove(&key), s.joins.remove(&key))
        });
        if let Some(run) = hook {
            crate::api::test::on_script_finished(lua, run, error_text.clone());
        }
        if let Some(caller) = join {
            match result {
                Ok(values) => make_ready_lua(lua, caller, values),
                Err(_) => {
                    let msg = error_text.unwrap_or_default();
                    with_state(lua, |s| s.ready.push_back((caller, Resume::Error(msg))));
                }
            }
        }
    }
}

pub fn spawn_function(lua: &Lua, func: Function, args: MultiValue, info: ThreadInfo) {
    let thread = match lua.create_thread(func) {
        Ok(t) => t,
        Err(e) => {
            report_error(lua, &e);
            return;
        }
    };
    let key = thread_key(&thread);
    with_state(lua, |s| {
        s.threads.insert(key, info.clone());
        if let Some(script) = info.script {
            s.script_threads
                .entry(script)
                .or_default()
                .push(thread.clone());
        }
    });
    resume_thread(lua, &thread, args);
}

pub fn schedule_wait(lua: &Lua, seconds: f64) {
    let thread = lua.current_thread();
    let now = engine(lua).lock().game_time;
    let delay = seconds.max(1.0 / 30.0);
    with_state(lua, |s| {
        s.sleepers.push(Sleeper {
            wake: now + delay,
            start: now,
            thread,
        });
    });
}

pub fn schedule_delay(lua: &Lua, seconds: f64, func: Function) -> mlua::Result<()> {
    let info = current_info(lua);
    let thread = lua.create_thread(func)?;
    let now = engine(lua).lock().game_time;
    let delay = seconds.max(1.0 / 30.0);
    with_state(lua, |s| {
        s.threads.insert(thread_key(&thread), info.clone());
        if let Some(script) = info.script {
            s.script_threads
                .entry(script)
                .or_default()
                .push(thread.clone());
        }
        s.sleepers.push(Sleeper {
            wake: now + delay,
            start: now,
            thread,
        });
    });
    Ok(())
}

fn resume_sleepers(lua: &Lua) {
    let now = engine(lua).lock().game_time;
    let mut due = with_state(lua, |s| {
        let mut due = Vec::new();
        let mut i = 0;
        while i < s.sleepers.len() {
            if s.sleepers[i].wake <= now {
                due.push(s.sleepers.swap_remove(i));
            } else {
                i += 1;
            }
        }
        due
    });
    due.sort_by(|a, b| {
        a.wake
            .partial_cmp(&b.wake)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for s in due {
        let elapsed = now - s.start;
        let args = MultiValue::from_vec(vec![Value::Number(elapsed), Value::Number(now)]);
        resume_thread(lua, &s.thread, args);
        pump(lua);
    }
}

pub fn make_ready(lua: &Lua, thread: Thread, values: Vec<Variant>) {
    with_state(lua, |s| s.ready.push_back((thread, Resume::Values(values))));
}

pub fn make_ready_lua(lua: &Lua, thread: Thread, values: MultiValue) {
    with_state(lua, |s| s.ready.push_back((thread, Resume::Lua(values))));
}

fn run_ready(lua: &Lua) {
    loop {
        let next = with_state(lua, |s| s.ready.pop_front());
        let Some((thread, values)) = next else { break };
        let args = match values {
            Resume::Error(msg) => {
                if thread.status() == ThreadStatus::Resumable {
                    let r = thread.resume_error::<MultiValue>(Value::String(
                        match lua.create_string(&msg) {
                            Ok(s) => s,
                            Err(_) => continue,
                        },
                    ));
                    if let Err(e) = r {
                        report_error(lua, &e);
                    }
                }
                continue;
            }
            Resume::Lua(m) => m,
            Resume::Values(v) => {
                let mut out = Vec::new();
                for x in v {
                    match variant_to_lua(lua, &x, None) {
                        Ok(val) => out.push(val),
                        Err(e) => report_error(lua, &e),
                    }
                }
                MultiValue::from_vec(out)
            }
        };
        resume_thread(lua, &thread, args);
        pump(lua);
    }
}

fn event_arg_values(lua: &Lua, class: &str, event: &str, args: &[Variant]) -> MultiValue {
    let desc = database().find_event(class, event);
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let ty = desc.and_then(|d| d.params.get(i)).map(|p| &p.ty);
        match variant_to_lua(lua, a, ty) {
            Ok(v) => out.push(v),
            Err(e) => {
                report_error(lua, &e);
                out.push(Value::Nil);
            }
        }
    }
    MultiValue::from_vec(out)
}

pub fn fire_lua(lua: &Lua, target: InstanceId, event: &'static str, args: MultiValue) {
    let (handlers, waiters) = with_state(lua, |s| {
        let ids = s
            .by_event
            .get(&(target, event))
            .cloned()
            .unwrap_or_default();
        let handlers: Vec<(Function, ThreadInfo)> = ids
            .iter()
            .filter_map(|id| s.conns.get(id))
            .map(|c| {
                (
                    c.func.clone(),
                    ThreadInfo {
                        script: c.script,
                        identity: c.identity,
                    },
                )
            })
            .collect();
        let waiters = s.waiters.remove(&(target, event)).unwrap_or_default();
        (handlers, waiters)
    });
    if handlers.is_empty() && waiters.is_empty() {
        return;
    }
    let depth = with_state(lua, |s| {
        s.depth += 1;
        s.depth
    });
    if depth > 64 {
        with_state(lua, |s| s.depth -= 1);
        engine(lua).lock().print(
            MessageType::Error,
            "Maximum event re-entrancy depth exceeded",
        );
        return;
    }
    for (func, info) in handlers {
        if let Some(script) = info.script {
            let alive = with_state(lua, |s| {
                s.running_scripts.get(&script).copied().unwrap_or(false)
            });
            if !alive {
                continue;
            }
        }
        spawn_function(lua, func, args.clone(), info);
    }
    for t in waiters {
        if t.status() == ThreadStatus::Running {
            make_ready_lua(lua, t, args.clone());
        } else {
            resume_thread(lua, &t, args.clone());
        }
    }
    with_state(lua, |s| s.depth -= 1);
}

pub fn pump(lua: &Lua) {
    for _ in 0..1000 {
        let eng = engine(lua);
        let (dom, fired) = {
            let mut e = eng.lock();
            let cursor = e.cursor;
            let dom = e.dm.drain_events(cursor);
            let fired: Vec<Fired> = e.fired.drain(..).collect();
            (dom, fired)
        };
        if dom.is_empty() && fired.is_empty() {
            return;
        }
        for ev in dom {
            dispatch_dom_event(lua, &ev);
        }
        for f in fired {
            let class = eng.lock().dm.class_of(f.target).unwrap_or("Instance");
            let args = event_arg_values(lua, class, f.event, &f.args);
            fire_lua(lua, f.target, f.event, args);
        }
    }
}

fn has_listeners(lua: &Lua, target: InstanceId, event: &'static str) -> bool {
    with_state(lua, |s| {
        s.by_event
            .get(&(target, event))
            .is_some_and(|v| !v.is_empty())
            || s.waiters
                .get(&(target, event))
                .is_some_and(|v| !v.is_empty())
    })
}

fn dispatch_dom_event(lua: &Lua, ev: &DomEvent) {
    match ev {
        DomEvent::PropertyChanged { id, property } => {
            crate::scripts::on_property_changed(lua, *id, property);
            let eng = engine(lua);
            let class = eng.lock().dm.class_of(*id);
            let Some(class) = class else { return };
            let value_changed = database()
                .find_event(class, "Changed")
                .map(|e| e.class != "Instance")
                .unwrap_or(false);
            if value_changed {
                if *property == "Value" && has_listeners(lua, *id, "Changed") {
                    let v = eng.lock().dm.get(*id, "Value").unwrap_or(Variant::Nil);
                    let ty = eng
                        .lock()
                        .dm
                        .property_descriptor(*id, "Value")
                        .map(|d| d.ty.clone());
                    let val = variant_to_lua(lua, &v, ty.as_ref()).unwrap_or(Value::Nil);
                    fire_lua(lua, *id, "Changed", MultiValue::from_vec(vec![val]));
                }
            } else {
                let scriptable =
                    matches!(*property, "Name" | "Parent" | "Archivable" | "RobloxLocked")
                        || eng
                            .lock()
                            .dm
                            .property_descriptor(*id, property)
                            .map(|d| d.is_scriptable())
                            .unwrap_or(false);
                if scriptable
                    && has_listeners(lua, *id, "Changed")
                    && let Ok(s) = lua.create_string(property)
                {
                    fire_lua(
                        lua,
                        *id,
                        "Changed",
                        MultiValue::from_vec(vec![Value::String(s)]),
                    );
                }
            }
            if *property == "Name" {
                crate::scripts::on_name_changed(lua, *id);
            }
        }
        DomEvent::ChildAdded { parent, child } => {
            crate::scripts::on_child_added(lua, *parent, *child);
            for name in ["ChildAdded", "childAdded"] {
                if has_listeners(lua, *parent, name)
                    && let Ok(v) = push_instance(lua, *child)
                {
                    fire_lua(lua, *parent, name, MultiValue::from_vec(vec![v]));
                }
            }
        }
        DomEvent::ChildRemoved { parent, child } => {
            if has_listeners(lua, *parent, "ChildRemoved")
                && let Ok(v) = push_instance(lua, *child)
            {
                fire_lua(lua, *parent, "ChildRemoved", MultiValue::from_vec(vec![v]));
            }
        }
        DomEvent::DescendantAdded {
            ancestor,
            descendant,
        } => {
            if has_listeners(lua, *ancestor, "DescendantAdded")
                && let Ok(v) = push_instance(lua, *descendant)
            {
                fire_lua(
                    lua,
                    *ancestor,
                    "DescendantAdded",
                    MultiValue::from_vec(vec![v]),
                );
            }
        }
        DomEvent::DescendantRemoving {
            ancestor,
            descendant,
        } => {
            if has_listeners(lua, *ancestor, "DescendantRemoving")
                && let Ok(v) = push_instance(lua, *descendant)
            {
                fire_lua(
                    lua,
                    *ancestor,
                    "DescendantRemoving",
                    MultiValue::from_vec(vec![v]),
                );
            }
        }
        DomEvent::AncestryChanged { id, child, parent } => {
            crate::scripts::on_ancestry_changed(lua, *id);
            if has_listeners(lua, *id, "AncestryChanged") {
                let c = push_instance(lua, *child).unwrap_or(Value::Nil);
                let p = match parent {
                    Some(p) => push_instance(lua, *p).unwrap_or(Value::Nil),
                    None => Value::Nil,
                };
                fire_lua(
                    lua,
                    *id,
                    "AncestryChanged",
                    MultiValue::from_vec(vec![c, p]),
                );
            }
        }
        DomEvent::Destroyed { id } => {
            crate::scripts::on_destroyed(lua, *id);
            with_state(lua, |s| {
                let keys: Vec<(InstanceId, &'static str)> = s
                    .by_event
                    .keys()
                    .filter(|(t, _)| t == id)
                    .copied()
                    .collect();
                for k in keys {
                    if let Some(ids) = s.by_event.remove(&k) {
                        for c in ids {
                            s.conns.remove(&c);
                        }
                    }
                }
            });
        }
        DomEvent::Created { .. } => {}
    }
}

pub fn scan_scripts(lua: &Lua) {
    crate::scripts::scan_all(lua);
}

pub fn instance_from_value(lua: &Lua, v: &Value) -> Option<InstanceId> {
    let _ = lua;
    match v {
        Value::UserData(ud) => ud.borrow::<LInstance>().ok().map(|i| i.id),
        _ => None,
    }
}

pub fn expect_instance(lua: &Lua, v: &Value, what: &str) -> mlua::Result<InstanceId> {
    instance_from_value(lua, v).ok_or_else(|| rt(format!("{what} expected, got {}", v.type_name())))
}
