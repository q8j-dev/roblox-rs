use super::{Registry, Ret, none, ret};
use crate::convert::{arg_number, arg_string, variant_to_lua};
use crate::instance::push_instance;
use crate::runtime::{Engine, MessageType, engine, expect_instance, fire_lua};
use crate::types::rt;
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{Mat3, Vec3};
use rbx_reflection::ValueType;
use rustc_hash::FxHashMap as HashMap;

pub const PRIORITY_CORE: u32 = 1000;
const AUTO_STOP_FADE: f64 = 0.3;

#[derive(Clone, Debug)]
pub struct TrackState {
    pub animator: InstanceId,
    pub animation: InstanceId,
    pub kfs: Option<InstanceId>,
    start_time: f64,
    fade_start_time: f64,
    fade_start_weight: f64,
    fade_end_time: f64,
    fade_end_weight: f64,
    speed: f64,
    phase: f64,
    last_keyframe_time: f64,
    pre_keyframe: i32,
    pub is_playing: bool,
    priority: Option<u32>,
}

impl TrackState {
    fn new(animator: InstanceId, animation: InstanceId, kfs: Option<InstanceId>) -> TrackState {
        TrackState {
            animator,
            animation,
            kfs,
            start_time: 0.0,
            fade_start_time: 0.0,
            fade_start_weight: 0.0,
            fade_end_time: 0.0,
            fade_end_weight: 0.0,
            speed: 1.0,
            phase: 0.0,
            last_keyframe_time: 0.0,
            pre_keyframe: -1,
            is_playing: false,
            priority: None,
        }
    }

    fn weight_at(&self, t: f64) -> f64 {
        if t >= self.fade_end_time {
            self.fade_end_weight
        } else if t <= self.fade_start_time {
            self.fade_start_weight
        } else {
            let i = self.fade_end_time - self.fade_start_time;
            ((t - self.fade_start_time) / i) * self.fade_end_weight
                + ((self.fade_end_time - t) / i) * self.fade_start_weight
        }
    }

    fn reverse(&self) -> bool {
        self.speed < 0.0
    }

    fn keyframe_at(&self, t: f64, duration: f64) -> f64 {
        if !self.reverse() {
            (t - self.start_time) * self.speed + self.phase
        } else {
            (duration - (t - self.start_time) * -self.speed) + self.phase
        }
    }

    fn is_stopped(&self, t: f64) -> bool {
        t >= self.fade_end_time && self.fade_end_weight.abs() < 1e-5
    }
}

#[derive(Clone, Copy, Debug)]
struct CachedPose {
    weight: f32,
    mask_weight: f32,
    translation: Vec3,
    rot: Vec3,
}

impl Default for CachedPose {
    fn default() -> Self {
        CachedPose {
            weight: 0.0,
            mask_weight: 1.0,
            translation: Vec3::ZERO,
            rot: Vec3::ZERO,
        }
    }
}

fn axis_angle(m: &Mat3) -> Vec3 {
    let q = rbx_math::quat_from_matrix(m).normalize();
    let (axis, angle) = q.to_axis_angle();
    let angle = if angle > std::f32::consts::PI {
        angle - std::f32::consts::TAU
    } else {
        angle
    };
    axis * angle
}

fn flip_axis_angle(r: Vec3, lr: f32) -> Vec3 {
    r * ((lr - std::f32::consts::TAU) / lr)
}

fn lerp_axis_angle(r0: Vec3, r1: Vec3, w0: f32, w1: f32) -> Vec3 {
    let dot = r0.dot(r1);
    if dot < 0.0 {
        let l0 = r0.length();
        let l1 = r1.length();
        if l0 >= l1 {
            let len_r1_on_r0 = -dot / l0;
            if l0 + len_r1_on_r0 > std::f32::consts::PI {
                let r = flip_axis_angle(r0, l0) * w0 + r1 * w1;
                let lr_sq = r.length_squared();
                if lr_sq > std::f32::consts::PI * std::f32::consts::PI {
                    return flip_axis_angle(r, lr_sq.sqrt());
                }
                return r;
            }
        } else if l1.abs() < 1e-5 {
            return Vec3::ZERO;
        } else {
            return lerp_axis_angle(r1, r0, w1, w0);
        }
    }
    r0 * w0 + r1 * w1
}

fn interpolate(p0: &CachedPose, p1: &CachedPose, w0: f32, w1: f32) -> CachedPose {
    if w0 <= 0.0 {
        return *p1;
    }
    if w1 <= 0.0 {
        return *p0;
    }
    let n0 = w0 / (w0 + w1);
    let n1 = w1 / (w0 + w1);
    CachedPose {
        weight: n0 * p0.weight + n1 * p1.weight,
        mask_weight: n0 * p0.mask_weight + n1 * p1.mask_weight,
        translation: p0.translation * n0 + p1.translation * n1,
        rot: lerp_axis_angle(p0.rot, p1.rot, n0, n1),
    }
}

fn blend(p0: &CachedPose, p1: &CachedPose) -> CachedPose {
    let masked = p0.weight.min(p1.mask_weight);
    CachedPose {
        weight: masked + p1.weight,
        mask_weight: p0.mask_weight.min(p1.mask_weight),
        translation: p0.translation * masked + p1.translation * p1.weight,
        rot: lerp_axis_angle(p0.rot, p1.rot, masked, p1.weight),
    }
}

struct Keyframe {
    time: f32,
    poses: Vec<Option<CachedPose>>,
}

struct SequenceCache {
    duration: f32,
    loop_: bool,
    priority: u32,
    joints: Vec<(String, String)>,
    keyframes: Vec<Keyframe>,
    keyframe_names: Vec<(f32, String)>,
}

fn build_cache(dm: &DataModel, kfs: InstanceId) -> SequenceCache {
    let mut joints: Vec<(String, String)> = Vec::new();
    let mut duration = 0.0f32;
    let kf_ids: Vec<InstanceId> = dm
        .children_of(kfs)
        .iter()
        .copied()
        .filter(|c| dm.class_of(*c) == Some("Keyframe"))
        .collect();
    let pose_key = |p: InstanceId| -> (String, String) {
        let parent = dm
            .parent_of(p)
            .filter(|pp| dm.class_of(*pp) == Some("Pose"));
        let parent_name = match parent {
            Some(pp) => dm.name_of(pp).to_string(),
            None => "__Root".to_string(),
        };
        (parent_name, dm.name_of(p).to_string())
    };
    for &kf in &kf_ids {
        duration = duration.max(
            dm.get(kf, "Time")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0),
        );
        for d in dm.descendants(kf) {
            if dm.class_of(d) == Some("Pose") {
                let key = pose_key(d);
                if !joints.contains(&key) {
                    joints.push(key);
                }
            }
        }
    }
    let mut keyframes = Vec::new();
    let mut keyframe_names = Vec::new();
    for &kf in &kf_ids {
        let time = dm
            .get(kf, "Time")
            .ok()
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0);
        keyframe_names.push((time, dm.name_of(kf).to_string()));
        let mut poses = vec![None; joints.len()];
        for d in dm.descendants(kf) {
            if dm.class_of(d) == Some("Pose") {
                let key = pose_key(d);
                if let Some(i) = joints.iter().position(|j| *j == key) {
                    let cf = dm
                        .get(d, "CFrame")
                        .ok()
                        .and_then(|v| v.as_cframe())
                        .unwrap_or_default();
                    poses[i] = Some(CachedPose {
                        weight: dm
                            .get(d, "Weight")
                            .ok()
                            .and_then(|v| v.as_f32())
                            .unwrap_or(1.0),
                        mask_weight: dm
                            .get(d, "MaskWeight")
                            .ok()
                            .and_then(|v| v.as_f32())
                            .unwrap_or(0.0),
                        translation: cf.translation,
                        rot: axis_angle(&cf.rotation),
                    });
                }
            }
        }
        keyframes.push(Keyframe { time, poses });
    }
    keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
    SequenceCache {
        duration,
        loop_: dm.get_bool(kfs, "Loop"),
        priority: dm
            .get(kfs, "Priority")
            .ok()
            .and_then(|v| v.as_enum())
            .unwrap_or(2),
        joints,
        keyframes,
        keyframe_names,
    }
}

fn clamp_time(t: f64, duration: f64) -> f64 {
    if duration > 0.0 {
        if t < 0.0 {
            let off = (t.abs() / duration) as i64 + 1;
            return t + off as f64 * duration;
        } else if t > duration {
            let off = (t / duration) as i64;
            return t - off as f64 * duration;
        }
    }
    t
}

fn apply_sequence(
    cache: &SequenceCache,
    joints: &[(InstanceId, String, String)],
    poses: &mut [CachedPose],
    keyframe_time: f64,
    track_weight: f32,
) {
    if track_weight <= 0.0 || cache.keyframes.is_empty() {
        return;
    }
    let mut t = keyframe_time;
    if cache.loop_ && cache.duration > 0.0 && (t > cache.duration as f64 || t < 0.0) {
        t = clamp_time(t, cache.duration as f64);
    }
    for (i, (pname, cname)) in cache.joints.iter().enumerate() {
        for (j, (_, jp, jc)) in joints.iter().enumerate() {
            if jp != pname || jc != cname {
                continue;
            }
            let mut before = 0usize;
            let mut after = cache.keyframes.len() - 1;
            for (k, kf) in cache.keyframes.iter().enumerate() {
                if (kf.time as f64) <= t && kf.poses[i].is_some() {
                    before = k;
                } else if kf.poses[i].is_some() {
                    after = k;
                    break;
                }
            }
            if cache.keyframes[after].poses[i].is_none() {
                after = before;
            }
            let (Some(bp), Some(ap)) = (
                cache.keyframes[before].poses[i],
                cache.keyframes[after].poses[i],
            ) else {
                continue;
            };
            let mut afterw = t - cache.keyframes[before].time as f64;
            let beforew = cache.keyframes[after].time as f64 - t;
            if afterw + beforew <= 0.0 {
                afterw = 1.0;
            }
            let mut ip = interpolate(&bp, &ap, beforew as f32, afterw as f32);
            ip.weight *= track_weight;
            ip.mask_weight = 1.0 + (ip.mask_weight - 1.0) * track_weight;
            poses[j] = blend(&poses[j], &ip);
        }
    }
}

fn kfs_for(e: &mut Engine, animation: InstanceId) -> Option<InstanceId> {
    let id =
        e.dm.get(animation, "AnimationId")
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
    let id = crate::content::normalize_asset_id(&id);
    if id.is_empty() {
        return None;
    }
    if let Some(k) = e.active_kfs.get(&id).copied().filter(|k| e.dm.contains(*k)) {
        return Some(k);
    }
    if let Some(cached) = e.kfs_cache.get(&id) {
        return cached.filter(|k| e.dm.contains(*k));
    }
    let roots = crate::content::load_instances(e, &id);
    let found = roots
        .iter()
        .copied()
        .find(|r| e.dm.class_of(*r) == Some("KeyframeSequence"));
    for r in &roots {
        if Some(*r) != found {
            let _ = e.dm.destroy(*r);
        }
    }
    if let Some(k) = found {
        e.dm.retain(k);
    }
    e.kfs_cache.insert(id, found);
    found
}

pub fn find_or_create_animator(e: &mut Engine, owner: InstanceId) -> Result<InstanceId> {
    if let Some(a) = e.dm.find_first_child_of_class(owner, "Animator") {
        return Ok(a);
    }
    let a = e.dm.create("Animator").map_err(|x| rt(x.to_string()))?;
    let _ = e.dm.set(a, "Archivable", Variant::Bool(false));
    e.dm.set_parent(a, Some(owner))
        .map_err(|x| rt(x.to_string()))?;
    Ok(a)
}

fn load_animation_into(lua: &Lua, animator: InstanceId, args: &[Value]) -> Result<Ret> {
    let animation = match args.first() {
        Some(Value::Nil) | None => return Err(rt("Argument error: must be an Animation object")),
        Some(v) => expect_instance(lua, v, "Animation")?,
    };
    let track = {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        if e.dm.class_of(animation) != Some("Animation") {
            return Err(rt("Argument error: must be an Animation object"));
        }
        let kfs = kfs_for(e, animation);
        if kfs.is_none() {
            e.print(
                MessageType::Warning,
                "Object must be in Workspace before loading animation.",
            );
        }
        let track =
            e.dm.create_internal("AnimationTrack")
                .map_err(|x| rt(x.to_string()))?;
        let name = e.dm.name_of(animation).to_string();
        e.dm.set_name(track, &name);
        e.dm.lock_parent(track, true);
        e.dm.retain(track);
        e.tracks
            .insert(track, TrackState::new(animator, animation, kfs));
        track
    };
    ret(lua, push_instance(lua, track)?)
}

fn animator_load(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_animation_into(lua, id, args)
}

fn owner_load(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let animator = {
        let eng = engine(lua);
        let mut e = eng.lock();
        find_or_create_animator(&mut e, id)?
    };
    load_animation_into(lua, animator, args)
}

pub(crate) fn playing_tracks(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let ids: Vec<InstanceId> = {
        let e = engine(lua);
        let e = e.lock();
        match e.dm.find_first_child_of_class(id, "Animator") {
            Some(a) => e.active_tracks.get(&a).cloned().unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let t = lua.create_table()?;
    for (i, tr) in ids.iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, *tr)?)?;
    }
    ret(lua, t)
}

fn reload(e: &mut Engine, track: InstanceId) {
    let Some(st) = e.tracks.get(&track) else {
        return;
    };
    if st.kfs.is_none() {
        return;
    }
    let list = e.active_tracks.entry(st.animator).or_default();
    if !list.contains(&track) {
        list.push(track);
    }
}

fn sequence_duration(e: &Engine, kfs: Option<InstanceId>) -> f64 {
    let Some(k) = kfs else {
        return 0.0;
    };
    e.dm.children_of(k)
        .iter()
        .filter(|c| e.dm.class_of(**c) == Some("Keyframe"))
        .map(|c| {
            e.dm.get(*c, "Time")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
        })
        .fold(0.0f32, f32::max) as f64
}

fn track_play(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let fade = arg_number(args.first()).unwrap_or(0.1);
    let weight = arg_number(args.get(1)).unwrap_or(1.0);
    let speed = arg_number(args.get(2)).unwrap_or(1.0);
    let (owner, reversed_first) = {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        reload(e, id);
        let now = e.game_time;
        let first_name = e
            .tracks
            .get(&id)
            .and_then(|s| s.kfs)
            .and_then(|k| e.dm.children_of(k).last().copied())
            .map(|c| e.dm.name_of(c).to_string());
        let Some(st) = e.tracks.get_mut(&id) else {
            return none();
        };
        let mut reversed = None;
        if now >= st.start_time {
            st.phase = 0.0;
            st.start_time = now;
            st.fade_start_time = now;
            st.fade_start_weight = 0.0;
            st.fade_end_time = now + fade;
            st.fade_end_weight = weight;
            st.speed = speed;
            st.last_keyframe_time = 0.0;
            st.pre_keyframe = -1;
            st.is_playing = true;
            if st.reverse() {
                reversed = first_name;
            }
        }
        let owner = e.dm.parent_of(st.animator);
        (owner, reversed)
    };
    if let Some(name) = reversed_first {
        fire_keyframe(lua, id, &name);
    }
    if let Some(o) = owner
        && let Ok(v) = push_instance(lua, id)
    {
        fire_lua(lua, o, "AnimationPlayed", MultiValue::from_vec(vec![v]));
    }
    none()
}

fn fire_keyframe(lua: &Lua, track: InstanceId, name: &str) {
    if let Ok(s) = lua.create_string(name) {
        fire_lua(
            lua,
            track,
            "KeyframeReached",
            MultiValue::from_vec(vec![Value::String(s)]),
        );
    }
}

fn stop_state(st: &mut TrackState, now: f64, fade: f64) {
    st.fade_start_weight = st.weight_at(now);
    st.fade_start_time = now;
    st.fade_end_time = now + fade;
    st.fade_end_weight = 0.0;
    st.is_playing = false;
}

fn track_stop(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let fade = arg_number(args.first()).unwrap_or(0.1);
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        let now = e.game_time;
        if let Some(st) = e.tracks.get_mut(&id) {
            stop_state(st, now, fade);
        }
    }
    fire_lua(lua, id, "Stopped", MultiValue::new());
    none()
}

fn track_adjust_weight(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let weight = arg_number(args.first()).unwrap_or(1.0);
    let fade = arg_number(args.get(1)).unwrap_or(0.1);
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    reload(e, id);
    let now = e.game_time;
    if let Some(st) = e.tracks.get_mut(&id) {
        st.fade_start_weight = st.weight_at(now);
        st.fade_start_time = now;
        st.fade_end_time = now + fade;
        st.fade_end_weight = weight;
    }
    none()
}

fn track_adjust_speed(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let speed = arg_number(args.first()).unwrap_or(1.0);
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    reload(e, id);
    let now = e.game_time;
    let duration = sequence_duration(e, e.tracks.get(&id).and_then(|s| s.kfs));
    if let Some(st) = e.tracks.get_mut(&id)
        && speed != st.speed
    {
        if (st.speed >= 0.0) != (speed >= 0.0) {
            st.pre_keyframe = -1;
        }
        let kt = st.keyframe_at(now, duration);
        st.speed = speed;
        let delta = kt - st.keyframe_at(now, duration);
        st.phase += delta;
    }
    none()
}

fn track_time_of_keyframe(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = arg_string(args.first()).unwrap_or_default();
    let e = engine(lua);
    let e = e.lock();
    let kfs = e.tracks.get(&id).and_then(|s| s.kfs);
    if let Some(k) = kfs {
        for c in e.dm.children_of(k) {
            if e.dm.class_of(*c) == Some("Keyframe") && e.dm.name_of(*c) == name {
                let t =
                    e.dm.get(*c, "Time")
                        .ok()
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0);
                return ret(lua, t as f64);
            }
        }
    }
    Err(rt(format!(
        "Could not find a keyframe by that name: {name}"
    )))
}

pub fn get_track_property(lua: &Lua, id: InstanceId, name: &str) -> Result<Option<Value>> {
    let eng = engine(lua);
    let e = eng.lock();
    let Some(st) = e.tracks.get(&id) else {
        return Ok(None);
    };
    let duration = sequence_duration(&e, st.kfs);
    Ok(Some(match name {
        "Length" => Value::Number(duration),
        "IsPlaying" => Value::Boolean(st.is_playing),
        "Animation" => {
            let a = st.animation;
            drop(e);
            return Ok(Some(push_instance(lua, a)?));
        }
        "TimePosition" => {
            let t = st.keyframe_at(e.game_time, duration);
            Value::Number(
                if st.kfs.is_some() && e.dm.get_bool(st.kfs.unwrap_or(id), "Loop") {
                    clamp_time(t, duration)
                } else {
                    t.clamp(0.0, duration.max(0.0))
                },
            )
        }
        "Priority" => {
            let p = st.priority.unwrap_or_else(|| {
                st.kfs
                    .and_then(|k| e.dm.get(k, "Priority").ok().and_then(|v| v.as_enum()))
                    .unwrap_or(PRIORITY_CORE)
            });
            drop(e);
            return Ok(Some(variant_to_lua(
                lua,
                &Variant::Enum(p),
                Some(&ValueType::Enum("AnimationPriority".into())),
            )?));
        }
        _ => return Ok(None),
    }))
}

pub fn set_track_property(lua: &Lua, id: InstanceId, name: &str, value: &Variant) -> Result<bool> {
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    let now = e.game_time;
    let duration = sequence_duration(e, e.tracks.get(&id).and_then(|s| s.kfs));
    let Some(st) = e.tracks.get_mut(&id) else {
        return Ok(false);
    };
    match name {
        "TimePosition" => {
            let t = value.as_f64().unwrap_or(0.0);
            let delta = t - st.keyframe_at(now, duration);
            st.phase += delta;
            st.last_keyframe_time = t;
            st.pre_keyframe = -1;
            Ok(true)
        }
        "Priority" => {
            st.priority = value.as_enum();
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub fn step(lua: &Lua) {
    let mut keyframe_events: Vec<(InstanceId, String)> = Vec::new();
    let mut stopped: Vec<InstanceId> = Vec::new();
    {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        if e.active_tracks.is_empty() {
            return;
        }
        let now = e.game_time;
        let animators: Vec<InstanceId> = e.active_tracks.keys().copied().collect();
        let mut pose_updates: Vec<(InstanceId, CachedPose)> = Vec::new();
        for animator in animators {
            let Some(list) = e.active_tracks.get(&animator).cloned() else {
                continue;
            };
            let list: Vec<InstanceId> = list
                .into_iter()
                .filter(|t| {
                    e.dm.contains(*t)
                        && e.tracks.get(t).is_some_and(|s| {
                            s.kfs.is_some_and(|k| e.dm.contains(k)) && !s.is_stopped(now)
                        })
                })
                .collect();
            if list.is_empty() || !e.dm.contains(animator) {
                e.active_tracks.remove(&animator);
                continue;
            }
            e.active_tracks.insert(animator, list.clone());
            let Some(root) = e.dm.parent_of(animator).and_then(|p| e.dm.parent_of(p)) else {
                continue;
            };
            let joints: Vec<(InstanceId, String, String)> =
                e.dm.descendants(root)
                    .into_iter()
                    .filter(|d| e.dm.is_a(*d, "Motor"))
                    .map(|d| {
                        let p0 =
                            e.dm.get_ref(d, "Part0")
                                .map(|p| e.dm.name_of(p).to_string())
                                .unwrap_or_default();
                        let p1 =
                            e.dm.get_ref(d, "Part1")
                                .map(|p| e.dm.name_of(p).to_string())
                                .unwrap_or_default();
                        (d, p0, p1)
                    })
                    .collect();
            if joints.is_empty() {
                continue;
            }
            let mut caches: HashMap<InstanceId, SequenceCache> = HashMap::default();
            let mut layers: Vec<Vec<CachedPose>> = Vec::new();
            for prio in [PRIORITY_CORE, 0, 1, 2] {
                let mut poses: Vec<CachedPose> = Vec::new();
                for t in &list {
                    let (kfs, track_prio) = {
                        let st = &e.tracks[t];
                        let kfs = st.kfs.unwrap_or(*t);
                        let cache = caches.entry(kfs).or_insert_with(|| build_cache(&e.dm, kfs));
                        (kfs, st.priority.unwrap_or(cache.priority))
                    };
                    if track_prio != prio {
                        continue;
                    }
                    if poses.is_empty() {
                        poses = vec![CachedPose::default(); joints.len()];
                    }
                    let cache = &caches[&kfs];
                    let duration = cache.duration as f64;
                    let st = e.tracks.get_mut(t).expect("track");
                    let kt = st.keyframe_at(now, duration);
                    let tw = st.weight_at(now) as f32;
                    apply_sequence(cache, &joints, &mut poses, kt, tw);
                    let mut norm = kt;
                    if !cache.loop_ {
                        let done = if !st.reverse() {
                            st.last_keyframe_time < duration && kt >= duration
                        } else {
                            st.last_keyframe_time > 0.0 && kt <= 0.0
                        } && duration > 0.0;
                        if done {
                            let name = if !st.reverse() {
                                cache.keyframe_names.iter().last()
                            } else {
                                cache.keyframe_names.first()
                            };
                            if let Some((_, n)) = name {
                                keyframe_events.push((*t, n.clone()));
                            }
                            stop_state(st, now, AUTO_STOP_FADE);
                            stopped.push(*t);
                        }
                    } else {
                        norm = clamp_time(kt, duration);
                    }
                    let last = clamp_time(st.last_keyframe_time, duration);
                    detect_keyframes(st, cache, norm, last, &mut keyframe_events, *t);
                    st.last_keyframe_time = kt;
                }
                layers.push(poses);
            }
            let mut idle = layers[1].clone();
            let core = &layers[0];
            if idle.is_empty() {
                idle = core.clone();
            } else {
                for i in 0..core.len() {
                    let mut c = core[i];
                    c.weight *= idle[i].mask_weight;
                    idle[i] = blend(&c, &idle[i]);
                }
            }
            for layer in [&layers[2], &layers[3]] {
                if idle.is_empty() {
                    idle = layer.clone();
                } else {
                    for i in 0..layer.len() {
                        idle[i].weight *= layer[i].mask_weight;
                        idle[i] = blend(&idle[i], &layer[i]);
                    }
                }
            }
            for (i, (j, _, _)) in joints.iter().enumerate() {
                let pose = idle.get(i).copied().unwrap_or_default();
                pose_updates.push((*j, pose));
            }
        }
        if !pose_updates.is_empty() {
            let world = e.world.clone();
            let mut w = world.lock();
            w.sync(&mut e.dm);
            for (j, pose) in pose_updates {
                w.apply_motor_pose(j, pose.translation, pose.rot, pose.weight, pose.mask_weight);
            }
        }
    }
    for (t, name) in keyframe_events {
        fire_keyframe(lua, t, &name);
    }
    for t in stopped {
        fire_lua(lua, t, "Stopped", MultiValue::new());
    }
}

fn detect_keyframes(
    st: &mut TrackState,
    cache: &SequenceCache,
    now: f64,
    last: f64,
    out: &mut Vec<(InstanceId, String)>,
    track: InstanceId,
) {
    if !st.is_playing || cache.keyframe_names.is_empty() {
        return;
    }
    let names = &cache.keyframe_names;
    let n = names.len();
    if cache.loop_ {
        if !st.reverse() {
            if last > now {
                out.push((track, names[n - 1].1.clone()));
                out.push((track, names[0].1.clone()));
            }
        } else if now > last {
            out.push((track, names[0].1.clone()));
            out.push((track, names[n - 1].1.clone()));
        }
    }
    for (index, (time, name)) in names.iter().enumerate() {
        let t = *time as f64;
        let passed = if !st.reverse() {
            last <= t && t < now
        } else {
            last >= t && t > now
        };
        if passed {
            if st.pre_keyframe == index as i32 {
                continue;
            }
            out.push((track, name.clone()));
            st.pre_keyframe = index as i32;
            break;
        }
    }
}

fn register_active(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kfs = match args.first() {
        Some(v) => expect_instance(lua, v, "KeyframeSequence")?,
        None => return Err(rt("Argument must be a 'KeyframeSequence' object")),
    };
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.class_of(kfs) != Some("KeyframeSequence") {
        return Err(rt("Argument must be a 'KeyframeSequence' object"));
    }
    if e.dm.find_service("NetworkClient").is_some() || e.dm.find_service("NetworkServer").is_some()
    {
        return Err(rt(
            "Usage error: RegisterActiveKeyframeSequence can only be used in Solo mode.",
        ));
    }
    let id = format!("active://{}", e.active_kfs.len());
    e.active_kfs.insert(id.clone(), kfs);
    e.dm.retain(kfs);
    drop(e);
    ret(lua, id)
}

fn register_kfs(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let kfs = match args.first() {
        Some(v) => expect_instance(lua, v, "KeyframeSequence")?,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    if e.dm.find_service("NetworkServer").is_some() {
        return Err(rt(
            "Usage error: RegisterKeyframeSequence can only be used in Solo mode.",
        ));
    }
    let tree = e.dm.save_tree(&[kfs], rbx_dom::SaveOptions::default());
    let data = rbx_format::write_binary(&tree, rbx_format::BinaryWriteOptions::default())
        .map_err(|x| rt(x.to_string()))?;
    let id = crate::content::register(e, data);
    ret(lua, id)
}

fn get_kfs(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let clone = {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        let roots = crate::content::load_instances(e, &url);
        let found = roots
            .iter()
            .copied()
            .find(|r| e.dm.class_of(*r) == Some("KeyframeSequence"));
        for r in &roots {
            if Some(*r) != found {
                let _ = e.dm.destroy(*r);
            }
        }
        found
    };
    match clone {
        Some(k) => ret(lua, push_instance(lua, k)?),
        None => ret(lua, Value::Nil),
    }
}

fn get_kfs_by_id(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let asset = arg_number(args.first()).unwrap_or(0.0) as i64;
    if asset <= 0 {
        return Err(rt(
            "GetKeyframeSequenceById assetId is not a positive number!",
        ));
    }
    let url = format!("rbxassetid://{asset}");
    let s = lua.create_string(&url)?;
    get_kfs(lua, id, &[Value::String(s)])
}

fn kfs_children(lua: &Lua, id: InstanceId, class: &str) -> Result<Ret> {
    let ids: Vec<InstanceId> = {
        let e = engine(lua);
        let e = e.lock();
        e.dm.children_of(id)
            .iter()
            .copied()
            .filter(|c| e.dm.class_of(*c) == Some(class))
            .collect()
    };
    let t = lua.create_table()?;
    for (i, c) in ids.iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, *c)?)?;
    }
    ret(lua, t)
}

fn add_child_of(lua: &Lua, id: InstanceId, args: &[Value], class: &str) -> Result<Ret> {
    let child = match args.first() {
        Some(v) => expect_instance(lua, v, class)?,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.class_of(child) != Some(class) {
        return Err(rt(format!("Argument must be a '{class}' object")));
    }
    e.dm.set_parent(child, Some(id))
        .map_err(|x| rt(x.to_string()))?;
    none()
}

fn remove_child_of(lua: &Lua, id: InstanceId, args: &[Value], class: &str) -> Result<Ret> {
    let child = match args.first() {
        Some(v) => expect_instance(lua, v, class)?,
        None => return Err(rt("Argument 1 missing or nil")),
    };
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.parent_of(child) == Some(id) {
        e.dm.set_parent(child, None)
            .map_err(|x| rt(x.to_string()))?;
    }
    none()
}

pub fn register(r: &mut Registry) {
    r.add("Animator", &["LoadAnimation"], animator_load);
    r.add("Humanoid", &["LoadAnimation", "loadAnimation"], owner_load);
    r.add("AnimationController", &["LoadAnimation"], owner_load);
    r.add("Humanoid", &["GetPlayingAnimationTracks"], playing_tracks);
    r.add(
        "AnimationController",
        &["GetPlayingAnimationTracks"],
        playing_tracks,
    );
    r.add("AnimationTrack", &["Play"], track_play);
    r.add("AnimationTrack", &["Stop"], track_stop);
    r.add("AnimationTrack", &["AdjustWeight"], track_adjust_weight);
    r.add("AnimationTrack", &["AdjustSpeed"], track_adjust_speed);
    r.add(
        "AnimationTrack",
        &["GetTimeOfKeyframe"],
        track_time_of_keyframe,
    );
    r.add(
        "KeyframeSequenceProvider",
        &["RegisterActiveKeyframeSequence"],
        register_active,
    );
    r.add(
        "KeyframeSequenceProvider",
        &["RegisterKeyframeSequence"],
        register_kfs,
    );
    r.add(
        "KeyframeSequenceProvider",
        &["GetKeyframeSequence"],
        get_kfs,
    );
    r.add(
        "KeyframeSequenceProvider",
        &["GetKeyframeSequenceById"],
        get_kfs_by_id,
    );
    r.add("KeyframeSequence", &["GetKeyframes"], |lua, id, _| {
        kfs_children(lua, id, "Keyframe")
    });
    r.add("KeyframeSequence", &["AddKeyframe"], |lua, id, a| {
        add_child_of(lua, id, a, "Keyframe")
    });
    r.add("KeyframeSequence", &["RemoveKeyframe"], |lua, id, a| {
        remove_child_of(lua, id, a, "Keyframe")
    });
    r.add("Keyframe", &["GetPoses"], |lua, id, _| {
        kfs_children(lua, id, "Pose")
    });
    r.add("Keyframe", &["AddPose"], |lua, id, a| {
        add_child_of(lua, id, a, "Pose")
    });
    r.add("Keyframe", &["RemovePose"], |lua, id, a| {
        remove_child_of(lua, id, a, "Pose")
    });
    r.add("Pose", &["GetSubPoses"], |lua, id, _| {
        kfs_children(lua, id, "Pose")
    });
    r.add("Pose", &["AddSubPose"], |lua, id, a| {
        add_child_of(lua, id, a, "Pose")
    });
    r.add("Pose", &["RemoveSubPose"], |lua, id, a| {
        remove_child_of(lua, id, a, "Pose")
    });
}
