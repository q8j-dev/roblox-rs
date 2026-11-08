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
    pub seq: u64,
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
    pub virtual_clock: Option<f64>,
    pub frame_started: rbx_base::time::Instant,
    pub last_tick: f64,
    pub clock_carry: f64,
    pub frame_clock_peak: f64,
    pub running: bool,
    pub stopped: bool,
    pub load_string_enabled: bool,
    pub hooks: Vec<Box<dyn EngineHook>>,
    pub content_root: Option<std::path::PathBuf>,
    pub asset_dirs: Vec<std::path::PathBuf>,
    pub test: crate::api::test::TestState,
    pub world: rbx_physics::SharedWorld,
    pub physics_analyzer_break: bool,
    pub camel_case_violations: usize,
    pub model_in_primary: HashMap<InstanceId, rbx_math::Mat3>,
    pub ownership: HashMap<InstanceId, (Option<InstanceId>, bool)>,
    pub debris_timers: Vec<(f64, InstanceId)>,
    pub debris_queue: Vec<InstanceId>,
    pub debris_legacy_max_items: bool,
    pub pending_explosions: Vec<InstanceId>,
    pub exploded: std::collections::HashSet<InstanceId>,
    pub unparent_timers: Vec<(f64, InstanceId)>,
    pub gui_copied: std::collections::HashSet<InstanceId>,
    pub mice: HashMap<InstanceId, InstanceId>,
    pub player_data: HashMap<(InstanceId, String), Variant>,
    pub registered_content: HashMap<String, Vec<u8>>,
    pub tracks: HashMap<InstanceId, crate::api::animation::TrackState>,
    pub active_tracks: HashMap<InstanceId, Vec<InstanceId>>,
    pub kfs_cache: HashMap<String, Option<InstanceId>>,
    pub active_kfs: HashMap<String, InstanceId>,
    pub render_bindings: Vec<(String, i32, mlua::RegistryKey, Identity)>,
    pub network_requests: Vec<NetworkRequest>,
    pub client_count: usize,
    pub net_stats: Vec<NetPeerStats>,
    pub stats_sources: HashMap<InstanceId, crate::api::stats::StatSource>,
    pub fps: f64,
    pub jobs_stats_window: f64,
    pub threads_throttled: f64,
    pub base_url: String,
    pub gui: crate::api::gui::GuiState,
    pub plugins: crate::api::plugin::PluginState,
    pub input: crate::api::input::InputState,
    pub camera: crate::api::camera::CameraState,
    pub audio: rbx_audio::AudioEngine,
    pub audio_state: crate::api::sound::AudioState,
    pub join_after_move: (Option<InstanceId>, Option<InstanceId>),
    pub selection: Vec<InstanceId>,
    pub clipboard: Vec<InstanceId>,
    pub animate_fallback: bool,
    pub host_selection: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetPeerStats {
    pub name: String,
    pub sent_physics: f64,
    pub sent_data: f64,
    pub received_physics: f64,
    pub received_data: f64,
    pub sent_kbps: f64,
    pub received_kbps: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NetworkRequest {
    StartServer {
        port: u16,
    },
    StopServer,
    Connect {
        user_id: i64,
        server: String,
        port: u16,
        ticket: String,
    },
    Disconnect,
}

pub trait EngineHook: Send {
    fn name(&self) -> &'static str;
    fn as_any(&mut self) -> &mut dyn std::any::Any;
}

impl Engine {
    pub fn tick(&mut self) -> f64 {
        let value = match self.virtual_clock {
            Some(base) => {
                let in_frame = self.frame_started.elapsed().as_secs_f64();
                self.frame_clock_peak = self.frame_clock_peak.max(in_frame);
                base + self.game_time + self.clock_carry + in_frame
            }
            None => rbx_base::time::SystemTime::now()
                .duration_since(rbx_base::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
        };
        let value = value.max(self.last_tick);
        self.last_tick = value;
        value
    }

    pub fn process_after_load(&mut self) {
        let world = self.world.clone();
        let new_properties = {
            let mut w = world.lock();
            w.join_all(&mut self.dm);
            w.mode == rbx_physics::world::PhysicalMode::New
        };
        if new_properties {
            self.dm.convert_to_new_physical_properties();
        }
    }

    pub fn new(mut dm: DataModel) -> Engine {
        let cursor = dm.subscribe();
        let world = rbx_physics::shared_world();
        rbx_physics::WorldHooksImpl::install(&mut dm, world.clone());
        Engine {
            dm,
            cursor,
            output: Vec::new(),
            fired: VecDeque::new(),
            mode: RunMode::Standalone,
            local_player: None,
            clock: Stopwatch::new(),
            game_time: 0.0,
            virtual_clock: None,
            frame_started: rbx_base::time::Instant::now(),
            last_tick: 0.0,
            clock_carry: 0.0,
            frame_clock_peak: 0.0,
            running: false,
            stopped: false,
            load_string_enabled: false,
            hooks: Vec::new(),
            content_root: None,
            asset_dirs: Vec::new(),
            test: Default::default(),
            world,
            physics_analyzer_break: true,
            camel_case_violations: 0,
            model_in_primary: HashMap::new(),
            ownership: HashMap::new(),
            debris_timers: Vec::new(),
            debris_queue: Vec::new(),
            debris_legacy_max_items: false,
            pending_explosions: Vec::new(),
            exploded: std::collections::HashSet::new(),
            unparent_timers: Vec::new(),
            gui_copied: std::collections::HashSet::new(),
            mice: HashMap::new(),
            player_data: HashMap::new(),
            registered_content: HashMap::new(),
            tracks: HashMap::new(),
            active_tracks: HashMap::new(),
            kfs_cache: HashMap::new(),
            active_kfs: HashMap::new(),
            render_bindings: Vec::new(),
            network_requests: Vec::new(),
            client_count: 0,
            net_stats: Vec::new(),
            stats_sources: HashMap::new(),
            fps: 0.0,
            jobs_stats_window: 5.0,
            threads_throttled: 0.0,
            base_url: crate::api::http::DEFAULT_BASE_URL.to_string(),
            gui: Default::default(),
            plugins: Default::default(),
            input: Default::default(),
            camera: Default::default(),
            audio: rbx_audio::AudioEngine::disabled(),
            audio_state: Default::default(),
            join_after_move: (None, None),
            selection: Vec::new(),
            clipboard: Vec::new(),
            animate_fallback: false,
            host_selection: false,
        }
    }

    fn ownership_parts(&mut self, id: InstanceId) -> Result<Vec<InstanceId>, String> {
        let ws = self.dm.find_service("Workspace");
        if !ws.is_some_and(|w| self.dm.is_descendant_of(id, w)) {
            return Err(
                "Can only call Network Ownership API on a part that is descendent of Workspace"
                    .into(),
            );
        }
        if self.dm.find_service("NetworkClient").is_some() {
            return Err("Network Ownership API can only be called from the Server.".into());
        }
        if self.dm.class_of(id) == Some("Terrain") {
            return Err("Network Ownership API cannot be used on Terrain".into());
        }
        let world = self.world.clone();
        let mut w = world.lock();
        w.sync(&mut self.dm);
        w.rebuild_assemblies();
        match w.assembly_of(id) {
            Some(a) if a.anchored => Err("Network Ownership API cannot be called on Anchored parts or parts welded to Anchored parts.".into()),
            Some(_) => Ok(w
                .ownership_group_of(id)
                .map(|g| g.parts)
                .unwrap_or_else(|| vec![id])),
            None => Ok(vec![id]),
        }
    }

    pub fn reconcile_ownership(&mut self, parts: &[InstanceId]) -> (Option<InstanceId>, bool) {
        let mut entries = parts.iter().filter_map(|p| self.ownership.get(p).copied());
        let Some(first) = entries.next() else {
            return (None, true);
        };
        if entries.all(|e| e == first) {
            for p in parts {
                self.ownership.insert(*p, first);
            }
            return first;
        }
        for p in parts {
            self.ownership.insert(*p, (None, true));
        }
        (None, true)
    }

    pub fn can_set_network_owner(&mut self, id: InstanceId) -> Result<(), String> {
        self.ownership_parts(id).map(|_| ())
    }

    pub fn network_owner(&mut self, id: InstanceId) -> Option<InstanceId> {
        let parts = self.ownership_parts(id).ok()?;
        self.dm.find_service("NetworkServer")?;
        self.reconcile_ownership(&parts)
            .0
            .filter(|p| self.dm.contains(*p))
    }

    pub fn network_ownership_auto(&mut self, id: InstanceId) -> bool {
        match self.ownership_parts(id) {
            Ok(parts) => self.reconcile_ownership(&parts).1,
            Err(_) => true,
        }
    }

    pub fn set_network_owner(&mut self, id: InstanceId, player: Option<InstanceId>, auto: bool) {
        let Ok(parts) = self.ownership_parts(id) else {
            return;
        };
        let entry = if auto {
            (self.reconcile_ownership(&parts).0, true)
        } else {
            if self.backend_processing() && self.frontend_processing() {
                return;
            }
            (player, false)
        };
        for p in parts {
            self.ownership.insert(p, entry);
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
        let seq = self.dm.event_position();
        self.fired.push_back(Fired {
            target,
            event,
            args,
            seq,
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
    pub since: u64,
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
    pub touch_slots: HashMap<InstanceId, (usize, Option<InstanceId>)>,
    pub test_scripts: std::collections::HashSet<InstanceId>,
    pub reparent_guard: Vec<InstanceId>,
    pub waiters: HashMap<(InstanceId, &'static str), Vec<(Thread, u64)>>,
    pub dispatch_seq: Option<u64>,
    pub sleepers: Vec<Sleeper>,
    pub ready: VecDeque<(Thread, Resume)>,
    pub threads: HashMap<usize, ThreadInfo>,
    pub script_threads: HashMap<InstanceId, Vec<Thread>>,
    pub running_scripts: HashMap<InstanceId, bool>,
    pub script_activity: HashMap<InstanceId, (f64, u64)>,
    pub modules: HashMap<InstanceId, RegistryKey>,
    pub module_threads: HashMap<usize, InstanceId>,
    pub module_waiters: HashMap<InstanceId, Vec<Thread>>,
    pub module_errors: HashMap<InstanceId, String>,
    pub child_waiters: Vec<ChildWaiter>,
    pub depth: u32,
    pub globals: Option<RegistryKey>,
    pub instance_cache: Option<RegistryKey>,
    pub method_cache: Option<RegistryKey>,
    pub current: Vec<ThreadInfo>,
    pub finish_hooks: HashMap<usize, u32>,
    pub joins: HashMap<usize, Thread>,
    pub callbacks: HashMap<(InstanceId, String), RegistryKey>,
    pub pending_invokes: Vec<(InstanceId, String, Thread, Vec<Value>)>,
    pub remote: crate::api::remote::RemoteState,
    pub http: crate::api::http::HttpState,
    pub core_sources: HashMap<InstanceId, String>,
    pub actions: crate::api::actions::ActionState,
    pub tweens: crate::api::tween::TweenState,
    pub captures: HashMap<usize, Option<Result<MultiValue, String>>>,
    pub capture_threads: HashMap<usize, RegistryKey>,
    pub watchdog: Arc<Mutex<Watchdog>>,
}

#[derive(Debug)]
pub struct Watchdog {
    pub limit: Option<f64>,
    pub start: Option<Stopwatch>,
    pub identity: Identity,
    pub depth: u32,
}

impl Default for Watchdog {
    fn default() -> Self {
        Watchdog {
            limit: None,
            start: None,
            identity: Identity::GameScript,
            depth: 0,
        }
    }
}

pub fn set_script_timeout(lua: &Lua, seconds: f64) {
    let wd = with_state(lua, |s| s.watchdog.clone());
    wd.lock().limit = if seconds > 0.0 && seconds.is_finite() {
        Some(seconds)
    } else {
        None
    };
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
            touch_slots: HashMap::new(),
            test_scripts: std::collections::HashSet::new(),
            reparent_guard: Vec::new(),
            waiters: HashMap::new(),
            dispatch_seq: None,
            sleepers: Vec::new(),
            ready: VecDeque::new(),
            threads: HashMap::new(),
            script_threads: HashMap::new(),
            running_scripts: HashMap::new(),
            script_activity: HashMap::new(),
            modules: HashMap::new(),
            module_threads: HashMap::new(),
            module_waiters: HashMap::new(),
            module_errors: HashMap::new(),
            child_waiters: Vec::new(),
            depth: 0,
            globals: None,
            instance_cache: None,
            method_cache: None,
            current: Vec::new(),
            finish_hooks: HashMap::new(),
            joins: HashMap::new(),
            callbacks: HashMap::new(),
            pending_invokes: Vec::new(),
            remote: Default::default(),
            http: Default::default(),
            core_sources: HashMap::new(),
            actions: Default::default(),
            tweens: Default::default(),
            captures: HashMap::new(),
            capture_threads: HashMap::new(),
            watchdog: Arc::new(Mutex::new(Watchdog::default())),
        });
        let wd = with_state(&lua, |s| s.watchdog.clone());
        lua.set_interrupt(move |_| {
            let w = wd.lock();
            if let (Some(limit), Some(start)) = (w.limit, w.start.as_ref())
                && start.elapsed() > limit
                && matches!(
                    w.identity,
                    Identity::GameScript | Identity::RobloxGameScript
                )
            {
                return Err(rt("Game script timeout"));
            }
            Ok(mlua::VmState::Continue)
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

    pub fn execute_in_new_thread(
        &self,
        source: &str,
        identity: Identity,
        args: MultiValue,
    ) -> Result<MultiValue, String> {
        self.execute_named(source, "Script", identity, args)
    }

    pub fn execute_captured(
        &self,
        source: &str,
        name: &str,
        identity: Identity,
        args: MultiValue,
    ) -> Result<usize, String> {
        let env = new_environment(&self.lua, None).map_err(|e| e.to_string())?;
        let func = self
            .lua
            .load(source)
            .set_name(format!("={name}"))
            .set_environment(env)
            .into_function()
            .map_err(|e| format_error(&e))?;
        let thread = self.lua.create_thread(func).map_err(|e| e.to_string())?;
        let key = thread_key(&thread);
        with_state(&self.lua, |s| {
            s.threads.insert(
                key,
                ThreadInfo {
                    script: None,
                    identity,
                },
            );
            s.captures.insert(key, None);
        });
        let first = resume_thread_result(&self.lua, &thread, args);
        if thread.status() != ThreadStatus::Resumable {
            let outcome = first.unwrap_or_else(|| Ok(MultiValue::new()));
            with_state(&self.lua, |s| {
                s.captures.insert(key, Some(outcome));
            });
        }
        let thread_ref = self
            .lua
            .create_registry_value(thread)
            .map_err(|e| e.to_string())?;
        with_state(&self.lua, |s| s.capture_threads.insert(key, thread_ref));
        pump(&self.lua);
        Ok(key)
    }

    pub fn take_capture(&self, key: usize) -> Option<Result<MultiValue, String>> {
        let done = with_state(&self.lua, |s| match s.captures.get(&key) {
            Some(Some(_)) => s.captures.remove(&key).flatten(),
            _ => None,
        });
        if done.is_some() {
            with_state(&self.lua, |s| s.capture_threads.remove(&key));
        }
        done
    }

    pub fn drop_capture(&self, key: usize) {
        with_state(&self.lua, |s| {
            s.captures.remove(&key);
            s.capture_threads.remove(&key);
        });
    }

    pub fn execute_named(
        &self,
        source: &str,
        name: &str,
        identity: Identity,
        args: MultiValue,
    ) -> Result<MultiValue, String> {
        let env = new_environment(&self.lua, None).map_err(|e| e.to_string())?;
        let func = self
            .lua
            .load(source)
            .set_name(format!("={name}"))
            .set_environment(env)
            .into_function()
            .map_err(|e| format_error(&e))?;
        let thread = self.lua.create_thread(func).map_err(|e| e.to_string())?;
        with_state(&self.lua, |s| {
            s.threads.insert(
                thread_key(&thread),
                ThreadInfo {
                    script: None,
                    identity,
                },
            );
        });
        let result =
            resume_thread_result(&self.lua, &thread, args).unwrap_or_else(|| Ok(MultiValue::new()));
        pump(&self.lua);
        result
    }

    pub fn run_plugin(&self, source: &str, name: &str, dir: Option<std::path::PathBuf>, builtin: bool) -> Result<(), String> {
        {
            let mut e = self.engine.lock();
            e.plugins.current_dir = dir;
            e.plugins.loading_builtin = builtin;
        }
        let r = self.execute_named(source, name, Identity::StudioPlugin, MultiValue::new());
        {
            let mut e = self.engine.lock();
            e.plugins.current_dir = None;
            e.plugins.loading_builtin = false;
        }
        r.map(|_| ())
    }

    pub fn run_plugin_model(&self, data: &[u8], dir: Option<std::path::PathBuf>, builtin: bool) -> Result<(), String> {
        let roots = {
            let mut e = self.engine.lock();
            e.plugins.current_dir = dir;
            e.plugins.loading_builtin = builtin;
            e.dm.load_model(data).map_err(|err| err.to_string())?.0
        };
        let result = crate::api::plugin::create_model_plugin(&self.lua, &roots).map_err(|e| e.to_string());
        if result.is_ok() {
            let scripts: Vec<InstanceId> = {
                let e = self.engine.lock();
                roots
                    .iter()
                    .flat_map(|r| std::iter::once(*r).chain(e.dm.descendants(*r)))
                    .filter(|d| e.dm.class_of(*d) == Some("Script") && !e.dm.get_bool(*d, "Disabled"))
                    .collect()
            };
            for s in scripts {
                crate::scripts::start_script(&self.lua, s, Identity::StudioPlugin);
            }
            pump(&self.lua);
        }
        let mut e = self.engine.lock();
        e.plugins.current_dir = None;
        e.plugins.loading_builtin = false;
        result.map(|_| ())
    }

    pub fn click_plugin_button(&self, button: InstanceId) {
        fire_lua(&self.lua, button, "Click", MultiValue::new());
        pump(&self.lua);
    }

    pub fn input(&self, event: crate::api::user_input::InputEvent) {
        crate::api::user_input::process(&self.lua, event);
    }

    pub fn render_step(&self, dt: f64) {
        crate::api::user_input::render_step(&self.lua, dt);
    }

    pub fn set_touch_enabled(&self, enabled: bool) {
        self.engine.lock().input.pointer.touch_enabled = enabled;
    }

    pub fn window_focus(&self, focused: bool) {
        let uis = self.engine.lock().dm.find_service("UserInputService");
        if let Some(uis) = uis {
            fire_lua(
                &self.lua,
                uis,
                if focused {
                    "WindowFocused"
                } else {
                    "WindowFocusReleased"
                },
                MultiValue::new(),
            );
            pump(&self.lua);
        }
    }

    pub fn mouse_behavior(&self) -> u32 {
        let e = self.engine.lock();
        e.dm.find_service("UserInputService")
            .and_then(|u| e.dm.get(u, "MouseBehavior").ok())
            .and_then(|v| v.as_enum())
            .unwrap_or(0)
    }

    pub fn step(&self, dt: f64) {
        let running = {
            let eng = self.engine.clone();
            let mut e = eng.lock();
            e.game_time += dt;
            if dt > 0.0 {
                let rate = 1.0 / dt;
                e.fps = if e.fps == 0.0 {
                    rate
                } else {
                    e.fps * 0.9 + rate * 0.1
                };
            }
            e.clock_carry += e.frame_clock_peak;
            e.frame_clock_peak = 0.0;
            e.frame_started = rbx_base::time::Instant::now();
            let t = e.game_time;
            if let Some(ws) = e.dm.find_service("Workspace") {
                e.dm.store_silent(ws, "DistributedGameTime", Variant::Double(t));
            }
            crate::api::gui::sync_view(&mut e);
            let frontend = e.dm.find_service("NetworkServer").is_none();
            if frontend && e.running {
                crate::api::players::ensure_player_scripts(&mut e);
                crate::api::players::ensure_camera_subject(&mut e);
            }
            e.running
        };
        pump(&self.lua);
        if running {
            let (run_service, t) = {
                let e = self.engine.lock();
                (e.dm.find_service("RunService"), e.game_time)
            };
            if let Some(rs) = run_service {
                fire_lua(
                    &self.lua,
                    rs,
                    "Stepped",
                    MultiValue::from_vec(vec![Value::Number(t), Value::Number(dt)]),
                );
                pump(&self.lua);
            }
            self.step_explosions();
            crate::api::animation::step(&self.lua);
            crate::api::tween::step(&self.lua, dt as f32);
            pump(&self.lua);
            self.step_physics(dt);
        }
        crate::api::sound::step(&self.lua, dt);
        self.step_debris();
        crate::api::http::poll(&self.lua);
        resume_sleepers(&self.lua);
        run_ready(&self.lua);
        pump(&self.lua);
        if running {
            let run_service = self.engine.lock().dm.find_service("RunService");
            if let Some(rs) = run_service {
                fire_lua(
                    &self.lua,
                    rs,
                    "Heartbeat",
                    MultiValue::from_vec(vec![Value::Number(dt)]),
                );
                pump(&self.lua);
            }
        }
        crate::api::test::on_timeout(&self.lua);
        crate::api::stats::sync_network(&mut self.engine.lock());
        self.collect();
    }

    pub fn step_physics(&self, dt: f64) {
        let events = {
            let eng = self.engine.clone();
            let mut e = eng.lock();
            let world = e.world.clone();
            let mut w = world.lock();
            w.step(&mut e.dm, dt)
        };
        {
            let mut e = self.engine.lock();
            for (a, b) in &events.touched {
                if e.dm.contains(*a) && e.dm.contains(*b) {
                    e.dm.tools_on_touched(*a, *b);
                    e.dm.tools_on_touched(*b, *a);
                }
                if e.dm.contains(*a) && e.dm.contains(*b) {
                    e.dm.seats_on_touched(*a, *b);
                    e.dm.seats_on_touched(*b, *a);
                }
            }
            e.dm.tools_step();
        }
        for (a, b) in events.touched {
            self.fire_touch(a, b, "Touched");
        }
        for (a, b) in events.touch_ended {
            self.fire_touch(a, b, "TouchEnded");
        }
        crate::api::humanoid::fire_events(&self.lua, events.humanoid);
        for id in events.reached_target {
            fire_lua(&self.lua, id, "ReachedTarget", MultiValue::new());
        }
        if !events.fallen.is_empty() || !events.broken_joints.is_empty() {
            let eng = self.engine.clone();
            let mut e = eng.lock();
            for id in events.fallen.into_iter().chain(events.broken_joints) {
                if e.dm.contains(id) {
                    let _ = e.dm.destroy(id);
                }
            }
        }
        pump(&self.lua);
    }

    fn step_explosions(&self) {
        let pending: Vec<InstanceId> = std::mem::take(&mut self.engine.lock().pending_explosions);
        for id in pending {
            let result = {
                let eng = self.engine.clone();
                let mut guard = eng.lock();
                let e = &mut *guard;
                let Some(ws) = e.dm.find_service("Workspace") else {
                    continue;
                };
                if !e.dm.contains(id) || !e.dm.is_descendant_of(id, ws) || !e.exploded.insert(id) {
                    continue;
                }
                let f =
                    |n: &str, d: f32| e.dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(d);
                let radius = f("BlastRadius", 4.0);
                let pressure = f("BlastPressure", 500000.0);
                let percent = f("DestroyJointRadiusPercent", 1.0);
                let position =
                    e.dm.get(id, "Position")
                        .ok()
                        .and_then(|v| v.as_vec3())
                        .unwrap_or_default();
                let world = e.world.clone();
                let mut w = world.lock();
                let r = w.explosion(
                    &mut e.dm,
                    position,
                    radius,
                    pressure,
                    percent,
                    |dm, part| {
                        dm.parent_of(part).is_some_and(|c| {
                            dm.find_first_child_of_class(c, "ForceField").is_some()
                        })
                    },
                );
                drop(w);
                let mut life = 3.0f64;
                if radius >= 1.0 {
                    life -= 1.0 / radius as f64;
                }
                let t = e.game_time + life;
                e.unparent_timers.push((t, id));
                r
            };
            if has_listeners(&self.lua, id, "Hit") {
                for (part, dist) in result.hits {
                    if let Ok(v) = push_instance(&self.lua, part) {
                        fire_lua(
                            &self.lua,
                            id,
                            "Hit",
                            MultiValue::from_vec(vec![v, Value::Number(dist as f64)]),
                        );
                    }
                }
            }
            pump(&self.lua);
        }
    }

    fn step_debris(&self) {
        {
            let eng = self.engine.clone();
            let mut guard = eng.lock();
            let e = &mut *guard;
            let now = e.game_time;
            let due: Vec<InstanceId> = e
                .unparent_timers
                .iter()
                .filter(|(t, _)| *t <= now)
                .map(|(_, id)| *id)
                .collect();
            e.unparent_timers.retain(|(t, _)| *t > now);
            for id in due {
                if e.dm.contains(id) {
                    let _ = e.dm.set_parent(id, None);
                }
            }
        }
        {
            let eng = self.engine.clone();
            let mut guard = eng.lock();
            let e = &mut *guard;
            let now = e.game_time;
            let due: Vec<InstanceId> = e
                .debris_timers
                .iter()
                .filter(|(t, _)| *t <= now)
                .map(|(_, id)| *id)
                .collect();
            if due.is_empty() {
                return;
            }
            e.debris_timers.retain(|(t, _)| *t > now);
            for id in due {
                if e.dm.contains(id)
                    && let Err(err) = e.dm.destroy(id)
                {
                    e.print(MessageType::Warning, err.to_string());
                }
            }
            let dm = &e.dm;
            e.debris_queue.retain(|id| dm.contains(*id));
        }
        pump(&self.lua);
    }

    fn fire_touch(&self, a: InstanceId, b: InstanceId, event: &'static str) {
        let alive = {
            let e = self.engine.lock();
            e.dm.contains(a) && e.dm.contains(b)
        };
        if !alive {
            return;
        }
        for (target, other) in [(a, b), (b, a)] {
            if has_listeners(&self.lua, target, event)
                && let Ok(v) = push_instance(&self.lua, other)
            {
                fire_lua(&self.lua, target, event, MultiValue::from_vec(vec![v]));
            }
        }
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

    pub fn describe_waiting(&self) -> Vec<String> {
        let lua = &self.lua;
        let traceback = |t: &Thread| -> String {
            lua.globals()
                .get::<mlua::Table>("debug")
                .and_then(|d| d.get::<Function>("traceback"))
                .and_then(|f| f.call::<String>(t.clone()))
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        let (children, modules, events) = with_state(lua, |s| {
            (
                s.child_waiters
                    .iter()
                    .map(|w| (w.parent, w.name.clone(), w.thread.clone()))
                    .collect::<Vec<_>>(),
                s.module_waiters
                    .iter()
                    .flat_map(|(m, ts)| ts.iter().map(move |t| (*m, t.clone())))
                    .collect::<Vec<_>>(),
                s.waiters
                    .iter()
                    .flat_map(|((i, ev), ts)| ts.iter().map(move |(t, _)| (*i, *ev, t.clone())))
                    .collect::<Vec<_>>(),
            )
        });
        let e = self.engine.lock();
        for (parent, name, t) in children {
            out.push(format!(
                "WaitForChild {} in {}\n{}",
                name,
                e.dm.full_name(parent),
                traceback(&t)
            ));
        }
        for (m, t) in modules {
            out.push(format!("require {}\n{}", e.dm.full_name(m), traceback(&t)));
        }
        for (i, ev, t) in events {
            out.push(format!(
                "{}.{}:wait()\n{}",
                e.dm.full_name(i),
                ev,
                traceback(&t)
            ));
        }
        out
    }

    pub fn start_core_scripts(&self, starter: &str) -> mlua::Result<()> {
        crate::api::core::load_core_modules(&self.lua)?;
        crate::api::core::add_core_script(&self.lua, starter, None)?;
        pump(&self.lua);
        Ok(())
    }

    pub fn start_server_core_scripts(&self) -> mlua::Result<()> {
        crate::api::core::add_core_script(&self.lua, "ServerStarterScript", None)?;
        pump(&self.lua);
        Ok(())
    }

    pub fn fire(&self, target: InstanceId, event: &'static str, args: Vec<Variant>) {
        self.engine.lock().fire(target, event, args);
        pump(&self.lua);
    }

    pub fn script_activity(&self) -> Vec<(InstanceId, f64, u64)> {
        with_state(&self.lua, |s| {
            s.running_scripts
                .iter()
                .filter(|(_, alive)| **alive)
                .map(|(id, _)| {
                    let (t, n) = s.script_activity.get(id).copied().unwrap_or((0.0, 0));
                    (*id, t, n)
                })
                .collect()
        })
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
        Some(id) => {
            env.set("script", push_instance(lua, id)?)?;
            if let Some(p) = crate::api::plugin::plugin_for_script(lua, id) {
                env.set("plugin", push_instance(lua, p)?)?;
            }
        }
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
    resume_thread_result(lua, thread, args);
}

pub fn resume_thread_result(
    lua: &Lua,
    thread: &Thread,
    args: MultiValue,
) -> Option<Result<MultiValue, String>> {
    if thread.status() != ThreadStatus::Resumable {
        return None;
    }
    let info = with_state(lua, |s| s.threads.get(&thread_key(thread)).cloned());
    if let Some(i) = &info
        && let Some(script) = i.script
    {
        let alive = with_state(lua, |s| {
            s.running_scripts.get(&script).copied().unwrap_or(false)
        });
        if !alive {
            return None;
        }
        let test_script = with_state(lua, |s| s.test_scripts.contains(&script));
        let verdict = {
            let eng = engine(lua);
            let e = eng.lock();
            if e.dm.is_a(script, "BaseScript") {
                Some(e.dm.in_tree(script) && (test_script || !e.dm.get_bool(script, "Disabled")))
            } else {
                None
            }
        };
        if verdict == Some(false) {
            crate::scripts::stop_script(lua, script);
            return None;
        }
    }
    let wd = with_state(lua, |s| s.watchdog.clone());
    let previous_identity = {
        let mut w = wd.lock();
        if w.depth == 0 {
            w.start = Some(Stopwatch::new());
        }
        let prev = w.identity;
        w.identity = info.as_ref().map_or(Identity::GameScript, |i| i.identity);
        w.depth += 1;
        prev
    };
    let resume_started = rbx_base::time::Instant::now();
    let result = thread.resume::<MultiValue>(args);
    if let Some(script) = info.as_ref().and_then(|i| i.script) {
        let spent = resume_started.elapsed().as_secs_f64();
        with_state(lua, |s| {
            let entry = s.script_activity.entry(script).or_insert((0.0, 0));
            entry.0 += spent;
            entry.1 += 1;
        });
    }
    {
        let mut w = wd.lock();
        w.depth = w.depth.saturating_sub(1);
        w.identity = previous_identity;
        if w.depth == 0 {
            w.start = None;
        }
    }
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
        let (hook, join, module, reply) = with_state(lua, |s| {
            s.threads.remove(&key);
            (
                s.finish_hooks.remove(&key),
                s.joins.remove(&key),
                s.module_threads.remove(&key),
                s.remote.replies.remove(&key),
            )
        });
        let captured = with_state(lua, |s| s.captures.contains_key(&key));
        if captured {
            let outcome = match &result {
                Ok(values) => Ok(values.clone()),
                Err(_) => Err(error_text.clone().unwrap_or_default()),
            };
            with_state(lua, |s| {
                s.captures.insert(key, Some(outcome));
            });
        }
        if let Some(reply) = reply {
            let outcome = match &result {
                Ok(values) => Ok(values.clone()),
                Err(_) => Err(error_text.clone().unwrap_or_default()),
            };
            crate::api::remote::send_reply(lua, reply, outcome);
        }
        if let Some(module) = module {
            let outcome = match &result {
                Ok(values) => Ok(values.clone()),
                Err(_) => Err(error_text.clone().unwrap_or_default()),
            };
            module_finished(lua, module, outcome);
        }
        if let Some(run) = hook {
            crate::api::test::on_script_finished(lua, run, error_text.clone());
        }
        if let Some(caller) = join {
            match &result {
                Ok(values) => {
                    let copied = crate::api::bindables::copy_args(lua, &values.clone().into_vec())
                        .unwrap_or_else(|_| values.clone());
                    make_ready_lua(lua, caller, copied)
                }
                Err(_) => {
                    let msg = error_text.clone().unwrap_or_default();
                    with_state(lua, |s| s.ready.push_back((caller, Resume::Error(msg))));
                }
            }
        }
        return Some(match result {
            Ok(v) => Ok(v),
            Err(_) => Err(error_text.unwrap_or_default()),
        });
    }
    Some(Ok(MultiValue::new()))
}

fn module_finished(lua: &Lua, module: InstanceId, outcome: Result<MultiValue, String>) {
    let result = match outcome {
        Ok(values) if values.len() == 1 => {
            let v = values.into_iter().next().unwrap_or(Value::Nil);
            match lua.create_registry_value(v.clone()) {
                Ok(key) => {
                    with_state(lua, |s| {
                        s.modules.insert(module, key);
                    });
                    Ok(v)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        Ok(_) => Err("Module code did not return exactly one value".to_string()),
        Err(_) => Err("Requested module experienced an error while loading".to_string()),
    };
    let waiters = with_state(lua, |s| {
        if let Err(e) = &result {
            s.running_scripts.insert(module, false);
            s.module_errors.insert(module, e.clone());
        }
        s.module_waiters.remove(&module).unwrap_or_default()
    });
    for w in waiters {
        match &result {
            Ok(v) => make_ready_lua(lua, w, MultiValue::from_vec(vec![v.clone()])),
            Err(e) => with_state(lua, |s| s.ready.push_back((w, Resume::Error(e.clone())))),
        }
    }
}

pub fn require_module(lua: &Lua, m: Value) -> mlua::Result<Value> {
    let id = match instance_from_value(lua, &m) {
        Some(id) => id,
        None => {
            if let Value::Number(n) = m {
                return Err(rt(format!(
                    "Unable to load asset {n} as a module; asset loading is done through the content provider"
                )));
            }
            if let Value::Integer(n) = m {
                return Err(rt(format!(
                    "Unable to load asset {n} as a module; asset loading is done through the content provider"
                )));
            }
            return Err(rt("Attempted to call require with invalid argument(s)."));
        }
    };
    if let Some(v) = with_state(lua, |s| {
        s.modules.get(&id).map(|k| lua.registry_value::<Value>(k))
    }) {
        return v;
    }
    let caller = lua.current_thread();
    let loading = with_state(lua, |s| s.module_waiters.contains_key(&id));
    if loading {
        with_state(lua, |s| {
            s.module_waiters.entry(id).or_default().push(caller)
        });
        return yield_sentinel(lua);
    }
    if let Some(e) = with_state(lua, |s| s.module_errors.get(&id).cloned()) {
        return Err(rt(e));
    }
    let (class, source, name) = {
        let eng = engine(lua);
        let e = eng.lock();
        let class = e.dm.class_of(id).unwrap_or("");
        let src =
            e.dm.get(id, "Source")
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
        (class, src, e.dm.full_name(id))
    };
    if class != "ModuleScript" {
        return Err(rt("Attempted to call require with invalid argument(s)."));
    }
    let env = new_environment(lua, Some(id))?;
    let func = match lua
        .load(&source)
        .set_name(format!("={name}"))
        .set_environment(env)
        .into_function()
    {
        Ok(f) => f,
        Err(e) => {
            let text = format_error(&e);
            engine(lua).lock().print(MessageType::Error, text);
            with_state(lua, |s| {
                s.running_scripts.insert(id, false);
                s.module_errors.insert(
                    id,
                    "Requested module experienced an error while loading".to_string(),
                );
            });
            return Err(rt("Requested module experienced an error while loading"));
        }
    };
    let identity = current_info(lua).identity;
    let thread = lua.create_thread(func)?;
    let key = thread_key(&thread);
    with_state(lua, |s| {
        s.threads.insert(
            key,
            ThreadInfo {
                script: Some(id),
                identity,
            },
        );
        s.module_threads.insert(key, id);
        s.module_waiters.insert(id, Vec::new());
        s.running_scripts.insert(id, true);
        s.script_threads.entry(id).or_default().push(thread.clone());
    });
    match resume_thread_result(lua, &thread, MultiValue::new()) {
        Some(Ok(_)) if thread.status() == ThreadStatus::Resumable => {
            with_state(lua, |s| {
                s.module_waiters.entry(id).or_default().push(caller)
            });
            yield_sentinel(lua)
        }
        _ => match with_state(lua, |s| {
            s.modules.get(&id).map(|k| lua.registry_value::<Value>(k))
        }) {
            Some(v) => v,
            None => Err(rt(with_state(lua, |s| s.module_errors.get(&id).cloned())
                .unwrap_or_else(|| {
                    "Requested module experienced an error while loading".to_string()
                }))),
        },
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

fn waiting_threads_budget(lua: &Lua) -> f64 {
    let Ok(root) = crate::api::services::settings_root(lua, "GlobalSettings") else {
        return 0.1;
    };
    let eng = engine(lua);
    let e = eng.lock();
    e.dm.find_first_child_of_class(root, "LuaSettings")
        .and_then(|l| e.dm.get(l, "WaitingThreadsBudget").ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1)
}

fn resume_sleepers(lua: &Lua) {
    let budget = std::time::Duration::from_secs_f64((waiting_threads_budget(lua) / 60.0).max(0.0));
    let started = rbx_base::time::Instant::now();
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
    let mut throttling = false;
    let mut due = due.into_iter();
    for s in due.by_ref() {
        let elapsed = now - s.start;
        let args = MultiValue::from_vec(vec![Value::Number(elapsed), Value::Number(now)]);
        resume_thread(lua, &s.thread, args);
        pump(lua);
        if started.elapsed() > budget {
            throttling = true;
            break;
        }
    }
    let deferred: Vec<Sleeper> = due.collect();
    if !deferred.is_empty() {
        with_state(lua, |st| st.sleepers.extend(deferred));
    }
    let eng = engine(lua);
    let mut e = eng.lock();
    let sample = if throttling { 1.0 } else { 0.0 };
    e.threads_throttled += (sample - e.threads_throttled) * 0.05;
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
    let (handlers, waiters, seq) = with_state(lua, |s| {
        let seq = s.dispatch_seq;
        let ids = s
            .by_event
            .get(&(target, event))
            .cloned()
            .unwrap_or_default();
        let handlers: Vec<(Function, ThreadInfo)> = ids
            .iter()
            .filter_map(|id| s.conns.get(id))
            .filter(|c| seq.is_none_or(|q| c.since <= q))
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
        let all = s.waiters.remove(&(target, event)).unwrap_or_default();
        let (ready, later): (Vec<_>, Vec<_>) = all
            .into_iter()
            .partition(|(_, since)| seq.is_none_or(|q| *since <= q));
        if !later.is_empty() {
            s.waiters.insert((target, event), later);
        }
        let waiters: Vec<Thread> = ready.into_iter().map(|(t, _)| t).collect();
        s.dispatch_seq = None;
        (handlers, waiters, seq)
    });
    if handlers.is_empty() && waiters.is_empty() {
        with_state(lua, |s| s.dispatch_seq = seq);
        return;
    }
    let depth = with_state(lua, |s| {
        s.depth += 1;
        s.depth
    });
    if depth > 64 {
        with_state(lua, |s| {
            s.depth -= 1;
            s.dispatch_seq = seq;
        });
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
    with_state(lua, |s| {
        s.depth -= 1;
        s.dispatch_seq = seq;
    });
}

pub fn pump(lua: &Lua) {
    for _ in 0..1000 {
        let eng = engine(lua);
        let deliveries;
        let (dom, fired) = {
            let mut e = eng.lock();
            let cursor = e.cursor;
            let dom = e.dm.drain_events_from(cursor);
            for (level, text) in e.dm.drain_messages() {
                let kind = match level {
                    rbx_dom::MessageLevel::Output => MessageType::Output,
                    rbx_dom::MessageLevel::Info => MessageType::Info,
                    rbx_dom::MessageLevel::Warning => MessageType::Warning,
                    rbx_dom::MessageLevel::Error => MessageType::Error,
                };
                e.print(kind, text);
            }
            deliveries = e.dm.drain_deliveries();
            let signals = e.dm.drain_signals();
            for (target, event, args, seq) in signals {
                e.fired.push_back(Fired {
                    target,
                    event,
                    args,
                    seq,
                });
            }
            let fired: Vec<Fired> = e.fired.drain(..).collect();
            (dom, fired)
        };
        let (start, dom) = dom;
        let delivered = !deliveries.is_empty();
        if delivered {
            crate::api::remote::process_deliveries(lua, deliveries);
        }
        if dom.is_empty() && fired.is_empty() {
            if delivered {
                continue;
            }
            return;
        }
        for (i, ev) in dom.iter().enumerate() {
            with_state(lua, |s| s.dispatch_seq = Some(start + i as u64));
            dispatch_dom_event(lua, ev);
            with_state(lua, |s| s.dispatch_seq = None);
        }
        for f in fired {
            let class = eng.lock().dm.class_of(f.target).unwrap_or("Instance");
            let args = event_arg_values(lua, class, f.event, &f.args);
            with_state(lua, |s| s.dispatch_seq = Some(f.seq));
            fire_lua(lua, f.target, f.event, args);
            with_state(lua, |s| s.dispatch_seq = None);
        }
    }
}

pub fn has_listeners(lua: &Lua, target: InstanceId, event: &'static str) -> bool {
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
    let guarded = match ev {
        DomEvent::ChildAdded { child, .. } | DomEvent::ChildRemoved { child, .. } => Some(*child),
        DomEvent::DescendantAdded { descendant, .. }
        | DomEvent::DescendantRemoving { descendant, .. } => Some(*descendant),
        DomEvent::AncestryChanged { child, .. } => Some(*child),
        _ => None,
    };
    if let Some(g) = guarded {
        with_state(lua, |s| s.reparent_guard.push(g));
    }
    dispatch_dom_event_inner(lua, ev);
    if guarded.is_some() {
        with_state(lua, |s| {
            s.reparent_guard.pop();
        });
    }
}

fn dispatch_dom_event_inner(lua: &Lua, ev: &DomEvent) {
    match ev {
        DomEvent::PropertyChanged { id, property } => {
            crate::scripts::on_property_changed(lua, *id, property);
            if *property == "PlayCount" && engine(lua).lock().dm.class_of(*id) == Some("Sound") {
                crate::api::sound::on_play_count_changed(lua, *id);
            }
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
            if class == "Humanoid"
                && matches!(*property, "Health_XML" | "MaxHealth")
                && has_listeners(lua, *id, "HealthChanged")
            {
                let h = eng
                    .lock()
                    .dm
                    .get(*id, "Health_XML")
                    .ok()
                    .and_then(|v| v.as_f32())
                    .unwrap_or(0.0);
                fire_lua(
                    lua,
                    *id,
                    "HealthChanged",
                    MultiValue::from_vec(vec![Value::Number(h as f64)]),
                );
            }
        }
        DomEvent::ChildAdded { parent, child } => {
            crate::scripts::on_child_added(lua, *parent, *child);
            {
                let eng = engine(lua);
                let is_player = {
                    let e = eng.lock();
                    e.dm.class_of(*parent) == Some("Players")
                        && e.dm.class_of(*child) == Some("Player")
                };
                if is_player
                    && has_listeners(lua, *parent, "PlayerAdded")
                    && let Ok(v) = push_instance(lua, *child)
                {
                    fire_lua(lua, *parent, "PlayerAdded", MultiValue::from_vec(vec![v]));
                }
            }
            for name in ["ChildAdded", "childAdded"] {
                if has_listeners(lua, *parent, name)
                    && let Ok(v) = push_instance(lua, *child)
                {
                    fire_lua(lua, *parent, name, MultiValue::from_vec(vec![v]));
                }
            }
        }
        DomEvent::ChildRemoved { parent, child } => {
            {
                let eng = engine(lua);
                let is_player = {
                    let e = eng.lock();
                    e.dm.class_of(*parent) == Some("Players")
                        && e.dm.class_of(*child) == Some("Player")
                };
                if is_player
                    && has_listeners(lua, *parent, "PlayerRemoving")
                    && let Ok(v) = push_instance(lua, *child)
                {
                    fire_lua(
                        lua,
                        *parent,
                        "PlayerRemoving",
                        MultiValue::from_vec(vec![v]),
                    );
                }
            }
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
            crate::api::core::collection_changed(lua, *ancestor, *descendant, "ItemAdded");
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
            crate::api::core::collection_changed(lua, *ancestor, *descendant, "ItemRemoved");
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
            if engine(lua).lock().dm.class_of(*id) == Some("Sound") {
                crate::api::sound::on_ancestry_changed(lua, *id);
            }
            {
                let eng = engine(lua);
                let mut e = eng.lock();
                if e.dm.class_of(*id) == Some("Explosion") && !e.exploded.contains(id) {
                    e.pending_explosions.push(*id);
                }
            }
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
                s.touch_slots.remove(id);
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
