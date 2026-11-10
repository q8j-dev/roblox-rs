use super::{Registry, Ret, none, ret};
use crate::convert::arg_string;
use crate::instance::push_instance;
use crate::runtime::{current_info, engine, spawn_function, with_state};
use crate::types::rt;
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::InstanceId;
use std::collections::HashMap;

struct Session {
    parent: String,
    tag: String,
    created_ms: i64,
    metadata: HashMap<String, mlua::RegistryKey>,
}

#[derive(Default)]
pub struct ClientServicesState {
    requests: HashMap<InstanceId, RequestOptions>,
    sessions: HashMap<String, Session>,
    focused_contexts: Vec<String>,
    audio_contexts: Vec<i64>,
    audio_focus: Option<i64>,
}

const ROOT_SID: &str = "root";

#[derive(Clone)]
struct RequestOptions {
    url: String,
    method: String,
    body: Option<String>,
}

pub fn register(r: &mut Registry) {
    r.add("VRService", &["IsVRAppBuild", "GetUserCFrameEnabled"], no);
    r.add("VRService", &["GetUserCFrame"], identity_cframe);
    r.add(
        "VRService",
        &["RecenterUserHeadCFrame", "RequestNavigation"],
        ignore,
    );
    r.add("Player", &["GetUnder13"], no);
    r.add("FaceAnimatorService", &["IsStarted"], no);
    r.add("FaceAnimatorService", &["Init", "Start", "Stop"], ignore);
    r.add(
        "AchievementService",
        &["IsAvailable", "IsAchievementAvailable", "HasAchieved"],
        no,
    );
    r.add("InsertService", &["LoadLocalAsset"], load_local_asset);
    r.add(
        "Animator",
        &[
            "GetPlayingAnimationTracks",
            "GetPlayingAnimationTracksCoreScript",
        ],
        animator_tracks,
    );
    r.add("HttpService", &["GetUserAgent"], user_agent);
    r.add("HttpService", &["RequestInternal"], request_internal);
    r.add("HttpRequest", &["Start"], request_start);
    r.add("HttpRequest", &["Cancel"], request_cancel);
    r.add("Workspace", &["GetServerTimeNow"], server_time_now);
    r.add(
        "PlayerScripts",
        &[
            "GetRegisteredComputerCameraMovementModes",
            "GetRegisteredTouchCameraMovementModes",
            "GetRegisteredComputerMovementModes",
            "GetRegisteredTouchMovementModes",
        ],
        empty_array,
    );
    r.add("CaptureService", &["RetrieveCaptures"], empty_array);
    r.add(
        "CaptureService",
        &["IsCapturingVideo", "CanCaptureVideo"],
        no,
    );
    r.add("SessionService", &["GetRootSID"], session_root);
    r.add("SessionService", &["SetSession"], session_set);
    r.add("SessionService", &["RemoveSession"], session_remove);
    r.add("SessionService", &["ReplaceSession"], session_replace);
    r.add("SessionService", &["SessionExists"], session_exists);
    r.add("SessionService", &["GetSessionTag"], session_tag);
    r.add("SessionService", &["GetSessionID"], session_id_for);
    r.add("SessionService", &["SetMetadata"], session_set_metadata);
    r.add("SessionService", &["GetMetadata"], session_get_metadata);
    r.add(
        "SessionService",
        &["RemoveMetadata"],
        session_remove_metadata,
    );
    r.add(
        "SessionService",
        &["RemoveSessionsWithMetadataKey"],
        session_remove_with_key,
    );
    r.add(
        "SessionService",
        &["GenerateSessionInfoString"],
        session_info_string,
    );
    r.add(
        "SessionService",
        &["GetCreatedTimestampUtcMs"],
        session_created,
    );
    r.add("SessionService", &["AcquireContextFocus"], context_acquire);
    r.add("SessionService", &["ReleaseContextFocus"], context_release);
    r.add("SessionService", &["IsContextFocused"], context_focused);
    r.add(
        "ExperienceService",
        &["RegisterForExperienceJoin", "RegisterForExperienceLeave"],
        experience_register,
    );
    r.add("ExperienceService", &["IsInExperience"], yes);
    r.add(
        "ExperienceService",
        &["GetFollowUserId", "GetQueuePosition"],
        zero,
    );
    r.add("ExperienceService", &["GetPlaceJoinState"], empty_string);
    r.add(
        "ExperienceService",
        &[
            "GetPendingJoinAttempt",
            "ConsumePendingExperienceLeaveWithReason",
        ],
        nil,
    );
    r.add(
        "AudioFocusService",
        &["RegisterContextIdFromLua"],
        audio_register,
    );
    r.add(
        "AudioFocusService",
        &["AcquireFocus", "RequestFocus"],
        audio_acquire,
    );
    r.add("AudioFocusService", &["GetFocusedContextId"], audio_focused);
    r.add(
        "AudioFocusService",
        &["GetRegisteredContexts"],
        audio_registered,
    );
}

fn yes(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, true)
}

fn zero(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0)
}

fn empty_string(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}

fn nil(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}

fn experience_register(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(Value::Function(f)) = args.first().cloned() else {
        return Err(rt("Argument 1 missing or nil"));
    };
    let event = crate::signal::filtered_event("ExperienceService", "registration");
    let conn = crate::signal::connect(lua, id, event, f)?;
    ret(lua, lua.create_userdata(conn)?)
}

fn audio_register(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ctx = crate::convert::arg_number(args.first()).unwrap_or(0.0) as i64;
    with_state(lua, |s| {
        if !s.client_services.audio_contexts.contains(&ctx) {
            s.client_services.audio_contexts.push(ctx);
        }
    });
    none()
}

fn audio_acquire(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ctx = crate::convert::arg_number(args.first()).unwrap_or(0.0) as i64;
    with_state(lua, |s| s.client_services.audio_focus = Some(ctx));
    ret(lua, true)
}

fn audio_focused(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let f = with_state(lua, |s| s.client_services.audio_focus);
    ret(lua, f.unwrap_or(-1))
}

fn audio_registered(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let list = with_state(lua, |s| s.client_services.audio_contexts.clone());
    ret(lua, lua.create_sequence_from(list)?)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn server_time_now(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, now_ms() as f64 / 1000.0)
}

fn empty_array(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn sid_arg(args: &[Value], i: usize) -> String {
    arg_string(args.get(i)).unwrap_or_default()
}

fn session_root(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, ROOT_SID)
}

fn session_set(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let parent = sid_arg(args, 0);
    let child = sid_arg(args, 1);
    let tag = sid_arg(args, 2);
    with_state(lua, |s| {
        s.client_services.sessions.insert(
            child,
            Session {
                parent,
                tag,
                created_ms: now_ms(),
                metadata: HashMap::new(),
            },
        )
    });
    none()
}

fn remove_tree(s: &mut ClientServicesState, sid: &str) -> Vec<mlua::RegistryKey> {
    let children: Vec<String> = s
        .sessions
        .iter()
        .filter(|(_, v)| v.parent == sid)
        .map(|(k, _)| k.clone())
        .collect();
    let mut keys = Vec::new();
    for c in children {
        keys.extend(remove_tree(s, &c));
    }
    if let Some(session) = s.sessions.remove(sid) {
        keys.extend(session.metadata.into_values());
    }
    keys
}

fn session_remove(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let keys = with_state(lua, |s| remove_tree(&mut s.client_services, &sid));
    for k in keys {
        let _ = lua.remove_registry_value(k);
    }
    none()
}

fn session_replace(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let tag = sid_arg(args, 1);
    with_state(lua, |s| {
        if let Some(session) = s.client_services.sessions.get_mut(&sid) {
            session.tag = tag;
        }
    });
    none()
}

fn session_exists(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let exists =
        sid == ROOT_SID || with_state(lua, |s| s.client_services.sessions.contains_key(&sid));
    ret(lua, exists)
}

fn session_tag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let tag = with_state(lua, |s| {
        s.client_services.sessions.get(&sid).map(|x| x.tag.clone())
    });
    ret(lua, tag.unwrap_or_default())
}

fn session_id_for(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let structural = sid_arg(args, 0);
    let found = with_state(lua, |s| {
        s.client_services
            .sessions
            .iter()
            .find(|(k, v)| **k == structural || v.tag == structural)
            .map(|(k, _)| k.clone())
    });
    ret(lua, found.unwrap_or_default())
}

fn session_set_metadata(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let key = sid_arg(args, 1);
    let value = lua.create_registry_value(args.get(2).cloned().unwrap_or(Value::Nil))?;
    let old = with_state(lua, |s| {
        s.client_services
            .sessions
            .get_mut(&sid)
            .and_then(|session| session.metadata.insert(key, value))
    });
    if let Some(k) = old {
        let _ = lua.remove_registry_value(k);
    }
    none()
}

fn session_get_metadata(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let key = sid_arg(args, 1);
    let v = with_state(lua, |s| {
        s.client_services
            .sessions
            .get(&sid)
            .and_then(|session| session.metadata.get(&key))
            .and_then(|k| lua.registry_value::<Value>(k).ok())
    });
    ret(lua, v.unwrap_or(Value::Nil))
}

fn session_remove_metadata(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let key = sid_arg(args, 1);
    let old = with_state(lua, |s| {
        s.client_services
            .sessions
            .get_mut(&sid)
            .and_then(|session| session.metadata.remove(&key))
    });
    if let Some(k) = old {
        let _ = lua.remove_registry_value(k);
    }
    none()
}

fn session_remove_with_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = sid_arg(args, 0);
    let sids: Vec<String> = with_state(lua, |s| {
        s.client_services
            .sessions
            .iter()
            .filter(|(_, v)| v.metadata.contains_key(&key))
            .map(|(k, _)| k.clone())
            .collect()
    });
    for sid in sids {
        let keys = with_state(lua, |s| remove_tree(&mut s.client_services, &sid));
        for k in keys {
            let _ = lua.remove_registry_value(k);
        }
    }
    none()
}

fn session_info_string(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let text = with_state(lua, |s| {
        let mut parts: Vec<String> = s
            .client_services
            .sessions
            .iter()
            .map(|(k, v)| format!("{}:{}", v.tag, k))
            .collect();
        parts.sort();
        parts.join(",")
    });
    ret(lua, text)
}

fn session_created(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let sid = sid_arg(args, 0);
    let t = with_state(lua, |s| {
        s.client_services.sessions.get(&sid).map(|x| x.created_ms)
    });
    ret(lua, t.unwrap_or(0) as f64)
}

fn context_acquire(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ctx = sid_arg(args, 0);
    with_state(lua, |s| {
        s.client_services.focused_contexts.retain(|c| *c != ctx);
        s.client_services.focused_contexts.push(ctx);
    });
    none()
}

fn context_release(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ctx = sid_arg(args, 0);
    with_state(lua, |s| {
        s.client_services.focused_contexts.retain(|c| *c != ctx)
    });
    none()
}

fn context_focused(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ctx = sid_arg(args, 0);
    let focused = with_state(lua, |s| {
        s.client_services.focused_contexts.last() == Some(&ctx)
    });
    ret(lua, focused)
}

fn no(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn ignore(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn identity_cframe(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, crate::types::LCFrame(rbx_math::CFrame::IDENTITY))
}

fn load_local_asset(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let path = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let url = if path.contains("://") {
        path.clone()
    } else {
        format!("rbxasset://{path}")
    };
    let data = {
        let eng = engine(lua);
        let e = eng.lock();
        crate::content::fetch(&e, &url)
    }
    .ok_or_else(|| rt(format!("Unable to load local asset {path}")))?;
    let roots = engine(lua)
        .lock()
        .dm
        .load_model(&data)
        .map_err(|e| rt(e.to_string()))?
        .0;
    match roots.first() {
        Some(&r) => ret(lua, push_instance(lua, r)?),
        None => ret(lua, Value::Nil),
    }
}

fn animator_tracks(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let owner = engine(lua).lock().dm.parent_of(id);
    match owner {
        Some(p) => super::animation::playing_tracks(lua, p, args),
        None => ret(lua, lua.create_table()?),
    }
}

fn user_agent(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "Roblox/WinInet")
}

fn request_internal(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(Value::Table(options)) = args.first() else {
        return Err(rt("Argument 1 missing or nil"));
    };
    let url: String = options.get::<Option<String>>("Url")?.unwrap_or_default();
    let method: String = options
        .get::<Option<String>>("Method")?
        .unwrap_or_else(|| "GET".into());
    let body: Option<String> = options.get("Body")?;
    let id = engine(lua)
        .lock()
        .dm
        .create_internal("HttpRequest")
        .map_err(|e| rt(e.to_string()))?;
    with_state(lua, |s| {
        s.client_services
            .requests
            .insert(id, RequestOptions { url, method, body })
    });
    ret(lua, push_instance(lua, id)?)
}

fn request_cancel(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    with_state(lua, |s| s.client_services.requests.remove(&id));
    none()
}

fn request_start(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(Value::Function(callback)) = args.first().cloned() else {
        return Err(rt("Argument 1 missing or nil"));
    };
    let Some(options) = with_state(lua, |s| s.client_services.requests.remove(&id)) else {
        return none();
    };
    let (resolved, local) = {
        let eng = engine(lua);
        let e = eng.lock();
        let resolved = crate::api::http::resolve(&e, &options.url);
        let base = e.base_url.trim_end_matches('/').to_ascii_lowercase();
        let local = resolved.to_ascii_lowercase().starts_with(&base);
        (resolved, local)
    };
    let outcome = if local {
        let body = if options.method.eq_ignore_ascii_case("GET") {
            None
        } else {
            Some((
                options.body.clone().unwrap_or_default(),
                "application/json".to_string(),
            ))
        };
        crate::api::http::request_blocking(resolved, body)
    } else {
        Err(format!(
            "HttpError: {} is not served by this client",
            options.url
        ))
    };
    let response = lua.create_table()?;
    let success = match outcome {
        Ok(bytes) => {
            response.set("StatusCode", 200)?;
            response.set("Body", lua.create_string(&bytes)?)?;
            true
        }
        Err(message) => {
            response.set("StatusCode", 0)?;
            response.set("Body", "")?;
            response.set("HttpError", message)?;
            false
        }
    };
    response.set("Headers", lua.create_table()?)?;
    spawn_function(
        lua,
        callback,
        MultiValue::from_vec(vec![Value::Boolean(success), Value::Table(response)]),
        current_info(lua),
    );
    none()
}
