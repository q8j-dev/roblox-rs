use super::camera::CameraView;
use super::{Registry, Ret, none, ret};
use crate::convert::variant_to_lua;
use crate::runtime::{Engine, engine, fire_lua};
use crate::types::{LCFrame, rt};
use mlua::{Lua, MultiValue, Result, Value};
use rbx_audio::{PlayParams, VoiceId, VoiceState, inverse_rolloff};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::CFrame;
use rbx_reflection::ValueType;
use rustc_hash::FxHashMap as HashMap;

#[derive(Clone, Debug, Default)]
pub struct SoundState {
    pub voice: Option<VoiceId>,
    pub play_count: i32,
    pub req: i32,
    pub position: f64,
    pub loops: u32,
    pub last_position: f64,
    pub detached: bool,
    pub part: Option<InstanceId>,
    pub was_in_world: bool,
}

#[derive(Clone, Debug, Default)]
pub enum Listener {
    #[default]
    Camera,
    CFrame(CFrame),
    ObjectPosition(InstanceId),
    ObjectCFrame(InstanceId),
}

#[derive(Default)]
pub struct AudioState {
    pub sounds: HashMap<InstanceId, SoundState>,
    pub listener: Listener,
    pub reverb: u32,
    pub master: f32,
}

fn sound_id(dm: &DataModel, id: InstanceId) -> String {
    match dm.get(id, "SoundId") {
        Ok(Variant::Content(s)) | Ok(Variant::String(s)) => s,
        _ => String::new(),
    }
}

fn f32_prop(dm: &DataModel, id: InstanceId, name: &str, default: f32) -> f32 {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_f32())
        .unwrap_or(default)
}

fn heard_globally(dm: &DataModel, id: InstanceId) -> bool {
    dm.find_service("Workspace")
        .is_some_and(|ws| dm.is_descendant_of(id, ws))
}

fn heard_locally(dm: &DataModel, id: InstanceId) -> bool {
    let player = dm.local_player;
    let mut p = dm.parent_of(id);
    while let Some(cur) = p {
        if matches!(
            dm.class_of(cur),
            Some("SoundService" | "Workspace" | "CoreGui")
        ) || Some(cur) == player
        {
            return true;
        }
        p = dm.parent_of(cur);
    }
    false
}

fn new_state(dm: &DataModel, id: InstanceId) -> SoundState {
    let req = dm
        .get(id, "PlayCount")
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0) as i32;
    SoundState {
        play_count: req,
        req,
        part: dm.parent_of(id).filter(|p| dm.is_a(*p, "BasePart")),
        was_in_world: heard_globally(dm, id),
        ..Default::default()
    }
}

fn clip_length(e: &mut Engine, id: InstanceId) -> f64 {
    let url = sound_id(&e.dm, id);
    if url.trim().is_empty() {
        return 0.0;
    }
    if let Some(d) = e.audio.cached_duration(&url) {
        return d;
    }
    let bytes = crate::content::fetch(e, &url);
    e.audio
        .clip(&url, || bytes)
        .map(|c| c.duration)
        .unwrap_or(0.0)
}

fn listener_cframe(e: &Engine) -> Option<CFrame> {
    let dm = &e.dm;
    let camera = CameraView::current(dm).map(|v| v.cframe);
    let location = |id: InstanceId| -> Option<CFrame> {
        if dm.is_a(id, "BasePart") {
            dm.get_raw(id, "CFrame").and_then(|v| v.as_cframe())
        } else if dm.is_a(id, "Model") {
            let primary = dm.get_ref(id, "PrimaryPart")?;
            dm.get_raw(primary, "CFrame").and_then(|v| v.as_cframe())
        } else {
            None
        }
    };
    match &e.audio_state.listener {
        Listener::Camera => camera,
        Listener::CFrame(cf) => Some(*cf),
        Listener::ObjectPosition(id) => {
            let mut cf = camera.unwrap_or_default();
            if let Some(l) = location(*id) {
                cf.translation = l.translation;
            }
            Some(cf)
        }
        Listener::ObjectCFrame(id) => location(*id).or(camera),
    }
}

fn spatial(e: &Engine, id: InstanceId, state: &SoundState) -> (f32, f32) {
    let Some(part) = state.part.filter(|p| e.dm.contains(*p)) else {
        return (1.0, 0.0);
    };
    let Some(listener) = listener_cframe(e) else {
        return (1.0, 0.0);
    };
    let Some(pos) =
        e.dm.get_raw(part, "CFrame")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
    else {
        return (1.0, 0.0);
    };
    let rolloff =
        e.dm.find_service("SoundService")
            .map(|s| f32_prop(&e.dm, s, "RolloffScale", 1.0))
            .unwrap_or(1.0);
    let to = pos - listener.translation;
    let distance = to.length();
    let gain = inverse_rolloff(
        distance,
        f32_prop(&e.dm, id, "MinDistance", 10.0),
        f32_prop(&e.dm, id, "MaxDistance", 100000.0),
        rolloff,
    );
    let pan = if distance > 1e-3 {
        (to / distance).dot(listener.right_vector()) * 0.8
    } else {
        0.0
    };
    (gain, pan)
}

fn start_voice(e: &mut Engine, id: InstanceId, start: f64) {
    if !e.audio.enabled() || !heard_locally(&e.dm, id) {
        return;
    }
    let url = sound_id(&e.dm, id);
    if url.trim().is_empty() {
        return;
    }
    let bytes = if e.audio.cached_duration(&url).is_some() {
        None
    } else {
        crate::content::fetch(e, &url)
    };
    let Some(clip) = e.audio.clip(&url, || bytes) else {
        return;
    };
    let mut state = e
        .audio_state
        .sounds
        .remove(&id)
        .unwrap_or_else(|| new_state(&e.dm, id));
    state.part = e.dm.parent_of(id).filter(|p| e.dm.is_a(*p, "BasePart"));
    let (gain, pan) = spatial(e, id, &state);
    let params = PlayParams {
        volume: f32_prop(&e.dm, id, "Volume", 0.5).clamp(0.0, 1.0) * gain,
        pitch: f32_prop(&e.dm, id, "Pitch", 1.0),
        pan,
        looped: e.dm.get_bool(id, "Looped"),
        start,
    };
    if let Some(old) = state.voice.take() {
        e.audio.stop(old);
    }
    state.voice = e.audio.play(&clip, params);
    state.last_position = start;
    e.audio_state.sounds.insert(id, state);
}

fn set_play_count(e: &mut Engine, id: InstanceId, value: i32) {
    let state = e
        .audio_state
        .sounds
        .entry(id)
        .or_insert_with(|| SoundState {
            play_count: -1,
            req: -1,
            ..Default::default()
        });
    state.req = value;
    state.play_count = value;
    e.dm.store_silent(id, "PlayCount", Variant::Int(value));
}

fn emit(lua: &Lua, id: InstanceId, event: &'static str, extra: Option<i64>) {
    let url = sound_id(&engine(lua).lock().dm, id);
    let Ok(s) = lua.create_string(&url) else {
        return;
    };
    let mut args = vec![Value::String(s)];
    if let Some(n) = extra {
        args.push(Value::Integer(n as mlua::Integer));
    }
    fire_lua(lua, id, event, MultiValue::from_vec(args));
}

pub fn play(lua: &Lua, id: InstanceId) {
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        let req = e
            .audio_state
            .sounds
            .get(&id)
            .map(|s| s.req)
            .unwrap_or_else(|| new_state(&e.dm, id).req);
        let start = e
            .audio_state
            .sounds
            .get(&id)
            .filter(|s| s.voice.is_none())
            .map(|s| s.position)
            .unwrap_or(0.0);
        let keep = looped_voice_playing(&e, id);
        if !keep {
            start_voice(&mut e, id, start);
        }
        set_play_count(&mut e, id, (req + 1).max(1));
        if !keep && let Some(s) = e.audio_state.sounds.get_mut(&id) {
            s.position = start;
            s.loops = 0;
        }
    }
    emit(lua, id, "Played", None);
}

fn pause(lua: &Lua, id: InstanceId) {
    let fire = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let voice = e.audio_state.sounds.get(&id).and_then(|s| s.voice);
        let mut fire = false;
        if let Some(v) = voice {
            if e.audio.state(v) == VoiceState::Playing {
                fire = true;
            }
            let pos = e.audio.position(v);
            e.audio.pause(v);
            if let Some(s) = e.audio_state.sounds.get_mut(&id) {
                s.position = pos;
            }
        }
        if e.audio_state.sounds.get(&id).map(|s| s.req) != Some(0) {
            set_play_count(&mut e, id, 0);
        }
        fire
    };
    if fire {
        emit(lua, id, "Paused", None);
    }
}

fn is_paused(e: &Engine, id: InstanceId) -> bool {
    let Some(s) = e.audio_state.sounds.get(&id) else {
        return true;
    };
    match s.voice {
        Some(v) => e.audio.state(v) != VoiceState::Playing,
        None => s.play_count <= 0,
    }
}

fn looped_voice_playing(e: &Engine, id: InstanceId) -> bool {
    e.dm.get_bool(id, "Looped")
        && e.audio_state
            .sounds
            .get(&id)
            .and_then(|s| s.voice)
            .is_some_and(|v| e.audio.state(v) == VoiceState::Playing)
}

fn is_playing(e: &Engine, id: InstanceId) -> bool {
    let Some(s) = e.audio_state.sounds.get(&id) else {
        return false;
    };
    match s.voice {
        Some(v) => e.audio.state(v) == VoiceState::Playing,
        None => s.play_count > 0,
    }
}

fn resume(lua: &Lua, id: InstanceId) {
    let eng = engine(lua);
    let mut e = eng.lock();
    if !is_paused(&e, id) {
        return;
    }
    let state = e.audio_state.sounds.get(&id).cloned().unwrap_or_default();
    match state.voice {
        Some(v) if e.audio.state(v) == VoiceState::Paused => e.audio.resume(v),
        _ => start_voice(&mut e, id, state.position),
    }
    set_play_count(&mut e, id, (state.req + 1).max(1));
}

pub fn stop(lua: &Lua, id: InstanceId) {
    let fire = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let voice = e
            .audio_state
            .sounds
            .get_mut(&id)
            .and_then(|s| s.voice.take());
        let mut fire = false;
        if let Some(v) = voice {
            fire = e.audio.state(v) == VoiceState::Playing;
            e.audio.stop(v);
        }
        if let Some(s) = e.audio_state.sounds.get_mut(&id) {
            s.position = 0.0;
            s.loops = 0;
        }
        if e.audio_state.sounds.get(&id).map(|s| s.req) != Some(-1) {
            set_play_count(&mut e, id, -1);
        }
        fire
    };
    if fire {
        emit(lua, id, "Stopped", None);
    }
}

fn native_play(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    play(lua, id);
    none()
}

fn native_pause(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    pause(lua, id);
    none()
}

fn native_resume(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    resume(lua, id);
    none()
}

fn native_stop(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    stop(lua, id);
    none()
}

pub fn get_sound_property(lua: &Lua, id: InstanceId, name: &str) -> Result<Option<Value>> {
    let eng = engine(lua);
    let mut e = eng.lock();
    Ok(Some(match name {
        "IsPlaying" | "isPlaying" => Value::Boolean(is_playing(&e, id)),
        "IsPaused" => Value::Boolean(is_paused(&e, id)),
        "TimeLength" => Value::Number(clip_length(&mut e, id)),
        "TimePosition" => {
            let s = e.audio_state.sounds.get(&id);
            let p = match s.and_then(|s| s.voice) {
                Some(v) => e.audio.position(v),
                None => s.map(|s| s.position).unwrap_or(0.0),
            };
            Value::Number(p)
        }
        _ => return Ok(None),
    }))
}

pub fn set_time_position(lua: &Lua, id: InstanceId, value: f64) {
    let eng = engine(lua);
    let mut e = eng.lock();
    let length = clip_length(&mut e, id);
    let value = if length > 0.0 {
        value.clamp(0.0, length)
    } else {
        value.max(0.0)
    };
    let state = e.audio_state.sounds.entry(id).or_default();
    state.position = value;
    state.last_position = value;
    if let Some(v) = state.voice {
        e.audio.seek(v, value);
    }
}

pub fn on_play_count_changed(lua: &Lua, id: InstanceId) {
    let (value, known) = {
        let eng = engine(lua);
        let e = eng.lock();
        let value =
            e.dm.get(id, "PlayCount")
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(-1.0) as i32;
        (value, e.audio_state.sounds.get(&id).map(|s| s.req))
    };
    if known == Some(value) {
        return;
    }
    match value {
        -1 => stop(lua, id),
        0 => pause(lua, id),
        v if known.is_none_or(|k| v > k) => {
            {
                let eng = engine(lua);
                let mut e = eng.lock();
                if !looped_voice_playing(&e, id) {
                    start_voice(&mut e, id, 0.0);
                }
                set_play_count(&mut e, id, v);
            }
            emit(lua, id, "Played", None);
        }
        _ => {}
    }
}

pub fn on_ancestry_changed(lua: &Lua, id: InstanceId) {
    let should_play = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let in_world = e.dm.in_tree(id) && heard_globally(&e.dm, id);
        let part = e.dm.parent_of(id).filter(|p| e.dm.is_a(*p, "BasePart"));
        let has_parent = e.dm.parent_of(id).is_some();
        let looped = e.dm.get_bool(id, "Looped");
        let entry = e
            .audio_state
            .sounds
            .entry(id)
            .or_insert_with(|| SoundState {
                play_count: -1,
                req: -1,
                was_in_world: in_world,
                ..Default::default()
            });
        let was = entry.was_in_world;
        entry.was_in_world = in_world;
        if has_parent {
            entry.part = part;
        }
        let left = was && !in_world;
        let mut stop_voice = None;
        if left && looped {
            stop_voice = entry.voice.take();
            entry.play_count = -1;
            entry.req = -1;
        }
        if let Some(v) = stop_voice {
            e.audio.stop(v);
        }
        left && !looped && e.dm.get_bool(id, "PlayOnRemove")
    };
    if should_play {
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.audio.enabled() && !sound_id(&e.dm, id).trim().is_empty() {
            let url = sound_id(&e.dm, id);
            let bytes = if e.audio.cached_duration(&url).is_some() {
                None
            } else {
                crate::content::fetch(&e, &url)
            };
            if let Some(clip) = e.audio.clip(&url, || bytes) {
                let params = PlayParams {
                    volume: f32_prop(&e.dm, id, "Volume", 0.5).clamp(0.0, 1.0),
                    pitch: f32_prop(&e.dm, id, "Pitch", 1.0),
                    pan: 0.0,
                    looped: false,
                    start: 0.0,
                };
                if let Some(v) = e.audio.play(&clip, params) {
                    let s = e.audio_state.sounds.entry(id).or_default();
                    s.voice = Some(v);
                    s.detached = true;
                }
            }
        }
    }
}

pub fn step(lua: &Lua, dt: f64) {
    let mut events: Vec<(InstanceId, &'static str, Option<i64>)> = Vec::new();
    {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        let master =
            e.dm.find_service("UserGameSettings")
                .map(|s| f32_prop(&e.dm, s, "MasterVolume", 1.0))
                .unwrap_or(1.0);
        if (master - e.audio_state.master).abs() > 1e-4 {
            e.audio_state.master = master;
            e.audio.set_master_volume(master);
        }
        let reverb =
            e.dm.find_service("SoundService")
                .and_then(|s| e.dm.get(s, "AmbientReverb").ok())
                .and_then(|v| v.as_enum())
                .unwrap_or(0);
        if reverb != e.audio_state.reverb {
            e.audio_state.reverb = reverb;
            e.audio.set_reverb(reverb);
        }
        let ids: Vec<InstanceId> = e.audio_state.sounds.keys().copied().collect();
        for id in ids {
            let alive = e.dm.contains(id);
            let Some(mut state) = e.audio_state.sounds.remove(&id) else {
                continue;
            };
            if let Some(v) = state.voice {
                match e.audio.state(v) {
                    VoiceState::Stopped => {
                        e.audio.release(v);
                        state.voice = None;
                        if alive && !state.detached && state.play_count > 0 {
                            events.push((id, "Ended", None));
                            state.play_count = -1;
                            state.req = -1;
                            state.position = 0.0;
                            e.dm.store_silent(id, "PlayCount", Variant::Int(-1));
                        }
                        state.detached = false;
                    }
                    voice_state => {
                        if alive && !state.detached {
                            let (gain, pan) = spatial(e, id, &state);
                            let volume = f32_prop(&e.dm, id, "Volume", 0.5).clamp(0.0, 1.0);
                            e.audio.set_volume(v, volume * gain);
                            e.audio.set_pan(v, pan);
                            e.audio.set_pitch(v, f32_prop(&e.dm, id, "Pitch", 1.0));
                            let looped = e.dm.get_bool(id, "Looped");
                            e.audio.set_looped(v, looped);
                            let pos = e.audio.position(v);
                            if looped
                                && voice_state == VoiceState::Playing
                                && pos + 1e-3 < state.last_position
                            {
                                state.loops += 1;
                                events.push((id, "DidLoop", Some(state.loops as i64)));
                            }
                            state.last_position = pos;
                            state.position = pos;
                        }
                    }
                }
            } else if alive && state.play_count > 0 {
                let length = clip_length(e, id);
                let pitch = f32_prop(&e.dm, id, "Pitch", 1.0) as f64;
                state.position += dt * pitch;
                if length > 0.0 && state.position > length {
                    if e.dm.get_bool(id, "Looped") {
                        state.loops += 1;
                        state.position %= length;
                        events.push((id, "DidLoop", Some(state.loops as i64)));
                    } else {
                        state.position = 0.0;
                        state.play_count = -1;
                        state.req = -1;
                        e.dm.store_silent(id, "PlayCount", Variant::Int(-1));
                        events.push((id, "Ended", None));
                    }
                }
            }
            if alive || state.voice.is_some() {
                e.audio_state.sounds.insert(id, state);
            }
        }
    }
    for (id, event, extra) in events {
        emit(lua, id, event, extra);
    }
}

fn set_listener(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kind = match args.first() {
        Some(v) => crate::convert::enum_value(lua, v, "ListenerType")?,
        None => {
            return Err(rt(
                "SoundService:SetListener called with an incorrect argument.",
            ));
        }
    };
    let listener = match kind {
        0 => Listener::Camera,
        1 => match args.get(1) {
            Some(v) => match mlua::FromLua::from_lua(v.clone(), lua) {
                Ok(LCFrame(cf)) => Listener::CFrame(cf),
                Err(_) => {
                    return Err(rt(
                        "SoundService:SetListener value given is not a valid CFrame when given Enum.ListenerType.CFrame",
                    ));
                }
            },
            None => {
                return Err(rt(
                    "SoundService:SetListener called with an incorrect argument.",
                ));
            }
        },
        2 | 3 => {
            let target = match args.get(1) {
                Some(v) => crate::runtime::instance_from_value(lua, v),
                None => None,
            };
            let Some(target) = target else {
                return Err(rt(
                    "SoundService:SetListener value given is not a valid instance",
                ));
            };
            let has_location = {
                let eng = engine(lua);
                let e = eng.lock();
                e.dm.is_a(target, "BasePart") || e.dm.is_a(target, "Model")
            };
            if !has_location {
                return Err(rt(
                    "SoundService:SetListener value given does not have a location",
                ));
            }
            if kind == 2 {
                Listener::ObjectPosition(target)
            } else {
                Listener::ObjectCFrame(target)
            }
        }
        _ => {
            return Err(rt(
                "SoundService:SetListener called with an incorrect argument.",
            ));
        }
    };
    engine(lua).lock().audio_state.listener = listener;
    none()
}

fn get_listener(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let listener = engine(lua).lock().audio_state.listener.clone();
    let (kind, value) = match listener {
        Listener::Camera => (0, Value::Nil),
        Listener::CFrame(cf) => (1, mlua::IntoLua::into_lua(LCFrame(cf), lua)?),
        Listener::ObjectPosition(id) => (2, crate::instance::push_instance(lua, id)?),
        Listener::ObjectCFrame(id) => (3, crate::instance::push_instance(lua, id)?),
    };
    let kind = variant_to_lua(
        lua,
        &Variant::Enum(kind),
        Some(&ValueType::Enum("ListenerType".into())),
    )?;
    ret(lua, (kind, value))
}

fn play_stock_sound(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

pub fn register(r: &mut Registry) {
    r.add("Sound", &["Play", "play"], native_play);
    r.add("Sound", &["Pause", "pause"], native_pause);
    r.add("Sound", &["Resume"], native_resume);
    r.add("Sound", &["Stop", "stop"], native_stop);
    r.add("SoundService", &["SetListener"], set_listener);
    r.add("SoundService", &["GetListener"], get_listener);
    r.add("SoundService", &["PlayStockSound"], play_stock_sound);
}
