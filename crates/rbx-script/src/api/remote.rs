use super::{Registry, Ret, none};
use crate::api::bindables::{callback, copy_args};
use crate::instance::{LInstance, push_instance};
use crate::runtime::{
    Resume, RunMode, ThreadInfo, current_info, engine, fire_lua, has_listeners, make_ready_lua,
    thread_key, with_state,
};
use crate::types::*;
use mlua::thread::ThreadStatus;
use mlua::{Lua, MultiValue, Result, Thread, Value};
use rbx_dom::{
    InstanceId, RemoteCall, RemoteDelivery, RemoteKind, RemoteTarget, RemoteValue, Variant,
};
use rustc_hash::FxHashMap as HashMap;
use std::collections::VecDeque;

pub const QUEUE_LIMIT: usize = 256;
const MAX_DEPTH: u32 = 64;

pub enum Reply {
    Local(Thread),
    Remote {
        instance: InstanceId,
        call: u32,
        target: RemoteTarget,
    },
}

struct Delayed {
    callback: &'static str,
    player: Option<InstanceId>,
    args: Vec<Value>,
    reply: Reply,
}

#[derive(Default)]
pub struct RemoteState {
    next_call: u32,
    waiting: HashMap<(InstanceId, u32), (Thread, Option<InstanceId>)>,
    pub replies: HashMap<usize, Reply>,
    delayed: HashMap<InstanceId, VecDeque<Delayed>>,
    latched: HashMap<(InstanceId, &'static str), VecDeque<Vec<Value>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Server,
    Client,
    Solo,
}

fn role(lua: &Lua) -> Role {
    match engine(lua).lock().mode {
        RunMode::Server => Role::Server,
        RunMode::Client => Role::Client,
        RunMode::Standalone => Role::Solo,
    }
}

fn local_player(lua: &Lua) -> Option<InstanceId> {
    let eng = engine(lua);
    let e = eng.lock();
    e.dm.local_player.filter(|p| e.dm.contains(*p))
}

fn userdata_variant(ud: &mlua::AnyUserData) -> Option<Variant> {
    macro_rules! try_types {
        ($($t:ident => $v:ident),* $(,)?) => {
            $(if let Ok(x) = ud.borrow::<$t>() {
                return Some(Variant::$v(x.0.clone()));
            })*
        };
    }
    if let Ok(i) = ud.borrow::<LInstance>() {
        return Some(Variant::Ref(Some(i.id)));
    }
    try_types!(
        LVector3 => Vector3,
        LVector2 => Vector2,
        LVector3int16 => Vector3int16,
        LVector2int16 => Vector2int16,
        LCFrame => CFrame,
        LColor3 => Color3,
        LBrickColor => BrickColor,
        LUDim => UDim,
        LUDim2 => UDim2,
        LRay => Ray,
        LRegion3 => Region3,
        LRegion3int16 => Region3int16,
        LRect2D => Rect2D,
        LFaces => Faces,
        LAxes => Axes,
        LNumberRange => NumberRange,
        LNumberSequence => NumberSequence,
        LColorSequence => ColorSequence,
        LPhysicalProperties => PhysicalProperties,
    );
    None
}

pub fn to_remote(v: &Value, depth: u32) -> Result<RemoteValue> {
    if depth > MAX_DEPTH {
        return Err(rt("tables cannot be cyclic"));
    }
    Ok(match v {
        Value::Nil => RemoteValue::Value(Variant::Nil),
        Value::Boolean(b) => RemoteValue::Value(Variant::Bool(*b)),
        Value::Integer(i) => RemoteValue::Value(Variant::Double(*i as f64)),
        Value::Number(n) => RemoteValue::Value(Variant::Double(*n)),
        Value::String(s) => RemoteValue::Value(Variant::String(
            String::from_utf8_lossy(&s.as_bytes()).into_owned(),
        )),
        Value::UserData(ud) => {
            if let Ok(item) = ud.borrow::<LEnumItem>() {
                return Ok(RemoteValue::Enum {
                    ty: item.enum_name.to_string(),
                    value: item.value,
                });
            }
            RemoteValue::Value(userdata_variant(ud).unwrap_or(Variant::Nil))
        }
        Value::Table(t) => {
            let len = t.raw_len();
            if len > 0 {
                let mut items = Vec::with_capacity(len);
                for i in 1..=len {
                    let item: Value = t.raw_get(i)?;
                    items.push(to_remote(&item, depth + 1)?);
                }
                RemoteValue::Array(items)
            } else {
                let mut items = Vec::new();
                for pair in t.clone().pairs::<Value, Value>() {
                    let (k, val) = pair?;
                    if let Value::String(s) = k {
                        items.push((
                            String::from_utf8_lossy(&s.as_bytes()).into_owned(),
                            to_remote(&val, depth + 1)?,
                        ));
                    }
                }
                if items.is_empty() {
                    RemoteValue::Array(Vec::new())
                } else {
                    RemoteValue::Map(items)
                }
            }
        }
        _ => RemoteValue::Value(Variant::Nil),
    })
}

pub fn from_remote(lua: &Lua, v: &RemoteValue) -> Result<Value> {
    Ok(match v {
        RemoteValue::Value(Variant::Double(n)) => Value::Number(*n),
        RemoteValue::Value(v) => crate::convert::variant_to_lua(lua, v, None)?,
        RemoteValue::Enum { ty, value } => match LEnumItem::lookup(ty, *value) {
            Some(item) => Value::UserData(lua.create_userdata(item)?),
            None => Value::Nil,
        },
        RemoteValue::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, from_remote(lua, item)?)?;
            }
            Value::Table(t)
        }
        RemoteValue::Map(items) => {
            let t = lua.create_table()?;
            for (k, item) in items {
                t.raw_set(k.as_str(), from_remote(lua, item)?)?;
            }
            Value::Table(t)
        }
    })
}

fn remote_args(args: &[Value]) -> Result<Vec<RemoteValue>> {
    args.iter().map(|a| to_remote(a, 0)).collect()
}

fn lua_args(lua: &Lua, args: &[RemoteValue]) -> Result<Vec<Value>> {
    args.iter().map(|a| from_remote(lua, a)).collect()
}

fn player_arg(lua: &Lua, v: Option<&Value>, what: &str) -> Result<InstanceId> {
    let id = match v {
        Some(Value::UserData(ud)) => ud.borrow::<LInstance>().ok().map(|i| i.id),
        _ => None,
    };
    let is_player = id.is_some_and(|i| engine(lua).lock().dm.class_of(i) == Some("Player"));
    match id {
        Some(i) if is_player => Ok(i),
        _ => Err(rt(format!(
            "{what}: player argument must be a Player object"
        ))),
    }
}

fn queue(lua: &Lua, call: RemoteCall) {
    engine(lua).lock().dm.queue_remote(call);
}

pub fn fire_or_latch(lua: &Lua, target: InstanceId, event: &'static str, args: Vec<Value>) {
    if has_listeners(lua, target, event) {
        fire_lua(lua, target, event, MultiValue::from_vec(args));
        return;
    }
    let dropped = with_state(lua, |s| {
        let q = s.remote.latched.entry((target, event)).or_default();
        if q.len() >= QUEUE_LIMIT {
            return true;
        }
        q.push_back(args);
        false
    });
    if dropped {
        let name = engine(lua).lock().dm.full_name(target);
        engine(lua).lock().print(
            crate::runtime::MessageType::Warning,
            format!("Remote event invocation queue exhausted for {name}; did you forget to implement {event}?"),
        );
    }
}

pub fn flush_latched(lua: &Lua, target: InstanceId, event: &'static str) {
    if !matches!(event, "OnServerEvent" | "OnClientEvent") {
        return;
    }
    let queued = with_state(lua, |s| s.remote.latched.remove(&(target, event)));
    for args in queued.into_iter().flatten() {
        fire_lua(lua, target, event, MultiValue::from_vec(args));
    }
}

fn fire_server(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    match role(lua) {
        Role::Server => Err(rt("FireServer can only be called from the client")),
        Role::Client => {
            let args = remote_args(args)?;
            queue(
                lua,
                RemoteCall {
                    instance: id,
                    target: RemoteTarget::Server,
                    kind: RemoteKind::Event,
                    args,
                },
            );
            none()
        }
        Role::Solo => {
            let mut values = vec![match local_player(lua) {
                Some(p) => push_instance(lua, p)?,
                None => Value::Nil,
            }];
            values.extend(copy_args(lua, args)?.into_vec());
            fire_or_latch(lua, id, "OnServerEvent", values);
            none()
        }
    }
}

fn fire_client(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    if role(lua) == Role::Client {
        return Err(rt("FireClient can only be called from the server"));
    }
    let player = player_arg(lua, args.first(), "FireClient")?;
    let rest = &args[1..];
    if role(lua) == Role::Server {
        let args = remote_args(rest)?;
        queue(
            lua,
            RemoteCall {
                instance: id,
                target: RemoteTarget::Player(player),
                kind: RemoteKind::Event,
                args,
            },
        );
    } else {
        let values = copy_args(lua, rest)?.into_vec();
        fire_or_latch(lua, id, "OnClientEvent", values);
    }
    none()
}

fn fire_all_clients(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    match role(lua) {
        Role::Client => Err(rt("FireAllClients can only be called from the server")),
        Role::Server => {
            let args = remote_args(args)?;
            queue(
                lua,
                RemoteCall {
                    instance: id,
                    target: RemoteTarget::AllPlayers,
                    kind: RemoteKind::Event,
                    args,
                },
            );
            none()
        }
        Role::Solo => {
            let values = copy_args(lua, args)?.into_vec();
            fire_or_latch(lua, id, "OnClientEvent", values);
            none()
        }
    }
}

fn start_call(lua: &Lua, id: InstanceId, player: Option<InstanceId>) -> u32 {
    let caller = lua.current_thread();
    with_state(lua, |s| {
        s.remote.next_call = s.remote.next_call.wrapping_add(1).max(1);
        let call = s.remote.next_call;
        s.remote.waiting.insert((id, call), (caller, player));
        call
    })
}

fn invoke_server(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    match role(lua) {
        Role::Server => Err(rt("InvokeServer can only be called from the client")),
        Role::Client => {
            let values = remote_args(args)?;
            let call = start_call(lua, id, None);
            queue(
                lua,
                RemoteCall {
                    instance: id,
                    target: RemoteTarget::Server,
                    kind: RemoteKind::Invoke(call),
                    args: values,
                },
            );
            Ok(Ret::Yield)
        }
        Role::Solo => {
            let player = local_player(lua);
            let args = copy_args(lua, args)?.into_vec();
            let caller = lua.current_thread();
            local_invoke(
                lua,
                id,
                "OnServerInvoke",
                player,
                args,
                Reply::Local(caller),
            )?;
            Ok(Ret::Yield)
        }
    }
}

fn invoke_client(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    if role(lua) == Role::Client {
        return Err(rt("InvokeClient can only be called from the server"));
    }
    let player = player_arg(lua, args.first(), "InvokeClient")?;
    let rest = &args[1..];
    if role(lua) == Role::Server {
        let values = remote_args(rest)?;
        let call = start_call(lua, id, Some(player));
        queue(
            lua,
            RemoteCall {
                instance: id,
                target: RemoteTarget::Player(player),
                kind: RemoteKind::Invoke(call),
                args: values,
            },
        );
    } else {
        let args = copy_args(lua, rest)?.into_vec();
        let caller = lua.current_thread();
        local_invoke(lua, id, "OnClientInvoke", None, args, Reply::Local(caller))?;
    }
    Ok(Ret::Yield)
}

pub fn check_callback(lua: &Lua, name: &str) -> Result<()> {
    match (name, role(lua)) {
        ("OnServerInvoke", Role::Client) => {
            Err(rt("OnServerInvoke can only be implemented on the server"))
        }
        ("OnClientInvoke", Role::Server) => {
            Err(rt("OnClientInvoke can only be implemented on the client"))
        }
        _ => Ok(()),
    }
}

pub fn callback_changed(lua: &Lua, id: InstanceId) -> Result<()> {
    let queued = with_state(lua, |s| s.remote.delayed.remove(&id));
    let mut queued = queued.unwrap_or_default();
    while let Some(d) = queued.pop_front() {
        if callback(lua, id, d.callback).is_none() {
            queued.push_front(d);
            break;
        }
        local_invoke(lua, id, d.callback, d.player, d.args, d.reply)?;
    }
    if !queued.is_empty() {
        with_state(lua, |s| {
            let q = s.remote.delayed.entry(id).or_default();
            for d in queued.into_iter().rev() {
                q.push_front(d);
            }
        });
    }
    Ok(())
}

fn local_invoke(
    lua: &Lua,
    id: InstanceId,
    name: &'static str,
    player: Option<InstanceId>,
    args: Vec<Value>,
    reply: Reply,
) -> Result<()> {
    let Some(func) = callback(lua, id, name) else {
        let full = with_state(lua, |s| {
            let q = s.remote.delayed.entry(id).or_default();
            if q.len() >= QUEUE_LIMIT {
                return Err(reply);
            }
            q.push_back(Delayed {
                callback: name,
                player,
                args,
                reply,
            });
            Ok(())
        });
        if let Err(reply) = full {
            let fname = engine(lua).lock().dm.full_name(id);
            send_reply(
                lua,
                reply,
                Err(format!(
                    "Remote function invocation queue exhausted for {fname}; did you forget to implement {name}?"
                )),
            );
        }
        return Ok(());
    };
    let mut values = Vec::with_capacity(args.len() + 1);
    if let Some(p) = player {
        values.push(push_instance(lua, p)?);
    }
    values.extend(args);
    let thread = lua.create_thread(func)?;
    let info = match &reply {
        Reply::Local(caller) => with_state(lua, |s| s.threads.get(&thread_key(caller)).cloned())
            .unwrap_or_else(|| current_info(lua)),
        Reply::Remote { .. } => ThreadInfo {
            script: None,
            identity: crate::security::Identity::GameScript,
        },
    };
    with_state(lua, |s| {
        s.threads.insert(thread_key(&thread), info);
    });
    match thread.resume::<MultiValue>(MultiValue::from_vec(values)) {
        Ok(values) => {
            if thread.status() == ThreadStatus::Resumable {
                with_state(lua, |s| {
                    s.remote.replies.insert(thread_key(&thread), reply);
                });
            } else {
                with_state(lua, |s| {
                    s.threads.remove(&thread_key(&thread));
                });
                send_reply(lua, reply, Ok(values));
            }
        }
        Err(e) => {
            with_state(lua, |s| {
                s.threads.remove(&thread_key(&thread));
            });
            send_reply(lua, reply, Err(crate::runtime::format_error(&e)));
        }
    }
    Ok(())
}

pub fn send_reply(lua: &Lua, reply: Reply, result: std::result::Result<MultiValue, String>) {
    match reply {
        Reply::Local(caller) => match result {
            Ok(values) => {
                let copied = copy_args(lua, &values.clone().into_vec()).unwrap_or(values);
                make_ready_lua(lua, caller, copied);
            }
            Err(msg) => with_state(lua, |s| s.ready.push_back((caller, Resume::Error(msg)))),
        },
        Reply::Remote {
            instance,
            call,
            target,
        } => {
            let (kind, args) = match result {
                Ok(values) => match remote_args(&values.into_vec()) {
                    Ok(args) => (RemoteKind::Return(call), args),
                    Err(e) => (
                        RemoteKind::Fail(call),
                        vec![RemoteValue::Value(Variant::String(e.to_string()))],
                    ),
                },
                Err(msg) => (
                    RemoteKind::Fail(call),
                    vec![RemoteValue::Value(Variant::String(msg))],
                ),
            };
            queue(
                lua,
                RemoteCall {
                    instance,
                    target,
                    kind,
                    args,
                },
            );
        }
    }
}

pub fn process_deliveries(lua: &Lua, deliveries: Vec<RemoteDelivery>) {
    for d in deliveries {
        if let Err(e) = deliver(lua, d) {
            engine(lua).lock().print(
                crate::runtime::MessageType::Error,
                crate::runtime::format_error(&e),
            );
        }
    }
    fail_departed(lua);
}

fn deliver(lua: &Lua, d: RemoteDelivery) -> Result<()> {
    let server = role(lua) == Role::Server;
    match d.kind {
        RemoteKind::Event => {
            let mut values = Vec::with_capacity(d.args.len() + 1);
            let event = if server {
                values.push(match d.player {
                    Some(p) => push_instance(lua, p)?,
                    None => Value::Nil,
                });
                "OnServerEvent"
            } else {
                "OnClientEvent"
            };
            values.extend(lua_args(lua, &d.args)?);
            fire_or_latch(lua, d.instance, event, values);
        }
        RemoteKind::Invoke(call) => {
            let args = lua_args(lua, &d.args)?;
            if server {
                let Some(player) = d.player else {
                    return Ok(());
                };
                local_invoke(
                    lua,
                    d.instance,
                    "OnServerInvoke",
                    Some(player),
                    args,
                    Reply::Remote {
                        instance: d.instance,
                        call,
                        target: RemoteTarget::Player(player),
                    },
                )?;
            } else {
                local_invoke(
                    lua,
                    d.instance,
                    "OnClientInvoke",
                    None,
                    args,
                    Reply::Remote {
                        instance: d.instance,
                        call,
                        target: RemoteTarget::Server,
                    },
                )?;
            }
        }
        RemoteKind::Return(call) | RemoteKind::Fail(call) => {
            let waiting = with_state(lua, |s| {
                let key = (d.instance, call);
                match s.remote.waiting.get(&key) {
                    Some((_, player)) if server && *player != d.player => None,
                    Some(_) => s.remote.waiting.remove(&key),
                    None => None,
                }
            });
            let Some((thread, _)) = waiting else {
                return Ok(());
            };
            if let RemoteKind::Return(_) = d.kind {
                let values = lua_args(lua, &d.args)?;
                make_ready_lua(lua, thread, MultiValue::from_vec(values));
            } else {
                let msg = match d.args.first() {
                    Some(RemoteValue::Value(Variant::String(s))) => s.clone(),
                    _ => "remote function failed".to_string(),
                };
                with_state(lua, |s| s.ready.push_back((thread, Resume::Error(msg))));
            }
        }
    }
    Ok(())
}

fn fail_departed(lua: &Lua) {
    if role(lua) != Role::Server {
        return;
    }
    let players: Vec<InstanceId> = with_state(lua, |s| {
        s.remote.waiting.values().filter_map(|(_, p)| *p).collect()
    });
    if players.is_empty() {
        return;
    }
    let gone: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        players
            .into_iter()
            .filter(|p| e.dm.parent_of(*p).and_then(|pp| e.dm.class_of(pp)) != Some("Players"))
            .collect()
    };
    for p in gone {
        let calls: Vec<((InstanceId, u32), Thread)> = with_state(lua, |s| {
            let keys: Vec<(InstanceId, u32)> = s
                .remote
                .waiting
                .iter()
                .filter(|(_, (_, pl))| *pl == Some(p))
                .map(|(k, _)| *k)
                .collect();
            keys.into_iter()
                .filter_map(|k| s.remote.waiting.remove(&k).map(|(t, _)| (k, t)))
                .collect()
        });
        for ((rf, _), thread) in calls {
            let msg = {
                let eng = engine(lua);
                let e = eng.lock();
                format!(
                    "Player {} disconnected during remote call to {}",
                    e.dm.name_of(p),
                    e.dm.full_name(rf)
                )
            };
            with_state(lua, |s| s.ready.push_back((thread, Resume::Error(msg))));
        }
    }
}

pub fn register(r: &mut Registry) {
    r.add("RemoteEvent", &["FireServer"], fire_server);
    r.add("RemoteEvent", &["FireClient"], fire_client);
    r.add("RemoteEvent", &["FireAllClients"], fire_all_clients);
    r.add("RemoteFunction", &["InvokeServer"], invoke_server);
    r.add("RemoteFunction", &["InvokeClient"], invoke_client);
}
