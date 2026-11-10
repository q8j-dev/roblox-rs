use super::{Registry, Ret, none, ret};
use crate::datatypes::LTweenInfo;
use crate::instance::push_instance;
use crate::runtime::{engine, with_state};
use crate::types::rt;
use mlua::{FromLua, Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_math::{NumberRange, Rect2D, UDim, UDim2};
use std::collections::HashMap;
use std::f32::consts::PI;

const BEGIN: u32 = 0;
const DELAYED: u32 = 1;
const PLAYING: u32 = 2;
const PAUSED: u32 = 3;
const COMPLETED: u32 = 4;
const CANCELLED: u32 = 5;

pub struct TweenData {
    target: InstanceId,
    pub info: LTweenInfo,
    goals: Vec<(String, Variant)>,
    starts: Vec<Variant>,
    elapsed: f64,
    delay_left: f64,
    repeats_done: i32,
    reversing: bool,
    state: u32,
}

#[derive(Default)]
pub struct TweenServiceState {
    pub tweens: HashMap<InstanceId, TweenData>,
}

pub fn register(r: &mut Registry) {
    r.add("TweenService", &["Create"], create);
    r.add("TweenService", &["GetValue"], get_value);
    r.add("TweenBase", &["Play"], play);
    r.add("TweenBase", &["Pause"], pause);
    r.add("TweenBase", &["Cancel"], cancel);
}

fn ease_in(style: u32, t: f32) -> f32 {
    match style {
        0 => t,
        1 => 1.0 - (t * PI / 2.0).cos(),
        2 => {
            let s = 1.70158;
            t * t * ((s + 1.0) * t - s)
        }
        3 => t * t,
        4 => t * t * t * t,
        5 => t * t * t * t * t,
        6 => 1.0 - bounce_out(1.0 - t),
        7 => {
            if t <= 0.0 || t >= 1.0 {
                t
            } else {
                let p = 0.3;
                -(2f32.powf(10.0 * (t - 1.0))) * ((t - 1.0 - p / 4.0) * 2.0 * PI / p).sin()
            }
        }
        8 => {
            if t <= 0.0 {
                0.0
            } else {
                2f32.powf(10.0 * t - 10.0)
            }
        }
        9 => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
        10 => t * t * t,
        _ => t,
    }
}

fn bounce_out(t: f32) -> f32 {
    let n = 7.5625;
    let d = 2.75;
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

pub fn ease(style: u32, direction: u32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match direction {
        0 => ease_in(style, t),
        1 => 1.0 - ease_in(style, 1.0 - t),
        _ => {
            if t < 0.5 {
                ease_in(style, t * 2.0) / 2.0
            } else {
                1.0 - ease_in(style, 2.0 - t * 2.0) / 2.0
            }
        }
    }
}

fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_udim(a: UDim, b: UDim, t: f32) -> UDim {
    UDim::new(
        lerp_f(a.scale, b.scale, t),
        lerp_f(a.offset as f32, b.offset as f32, t).round() as i32,
    )
}

fn interpolate(a: &Variant, b: &Variant, t: f32) -> Variant {
    match (a, b) {
        (Variant::Float(x), Variant::Float(y)) => Variant::Float(lerp_f(*x, *y, t)),
        (Variant::Double(x), Variant::Double(y)) => Variant::Double(x + (y - x) * t as f64),
        (Variant::Int(x), Variant::Int(y)) => {
            Variant::Int(lerp_f(*x as f32, *y as f32, t).round() as i32)
        }
        (Variant::Int64(x), Variant::Int64(y)) => {
            Variant::Int64((*x as f64 + (*y - *x) as f64 * t as f64).round() as i64)
        }
        (Variant::Vector2(x), Variant::Vector2(y)) => Variant::Vector2(*x + (*y - *x) * t),
        (Variant::Vector3(x), Variant::Vector3(y)) => Variant::Vector3(*x + (*y - *x) * t),
        (Variant::UDim(x), Variant::UDim(y)) => Variant::UDim(lerp_udim(*x, *y, t)),
        (Variant::UDim2(x), Variant::UDim2(y)) => Variant::UDim2(UDim2 {
            x: lerp_udim(x.x, y.x, t),
            y: lerp_udim(x.y, y.y, t),
        }),
        (Variant::Color3(x), Variant::Color3(y)) => Variant::Color3(x.lerp(*y, t)),
        (Variant::CFrame(x), Variant::CFrame(y)) => Variant::CFrame(x.lerp(y, t)),
        (Variant::Rect2D(x), Variant::Rect2D(y)) => Variant::Rect2D(Rect2D::xyxy(
            lerp_f(x.min.x, y.min.x, t),
            lerp_f(x.min.y, y.min.y, t),
            lerp_f(x.max.x, y.max.x, t),
            lerp_f(x.max.y, y.max.y, t),
        )),
        (Variant::NumberRange(x), Variant::NumberRange(y)) => Variant::NumberRange(
            NumberRange::new(lerp_f(x.min, y.min, t), lerp_f(x.max, y.max, t)),
        ),
        _ => {
            if t >= 1.0 {
                b.clone()
            } else {
                a.clone()
            }
        }
    }
}

fn set_state(lua: &Lua, id: InstanceId, state: u32) {
    with_state(lua, |s| {
        if let Some(t) = s.tween_service.tweens.get_mut(&id) {
            t.state = state;
        }
    });
    engine(lua)
        .lock()
        .dm
        .set_unchecked(id, "PlaybackState", Variant::Enum(state));
}

fn create(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let target = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v))
        .ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let info = match args.get(1) {
        Some(v) => LTweenInfo::from_lua(v.clone(), lua)?,
        None => return Err(rt("Argument 2 missing or nil")),
    };
    let Some(Value::Table(props)) = args.get(2) else {
        return Err(rt("Argument 3 missing or nil"));
    };
    let mut goals = Vec::new();
    for pair in props.clone().pairs::<String, Value>() {
        let (name, value) = pair?;
        let desc = engine(lua)
            .lock()
            .dm
            .property_descriptor(target, &name)
            .ok_or_else(|| rt(format!("{name} is not a valid member")))?;
        let v = crate::convert::lua_to_variant(lua, value, &desc.ty)?;
        goals.push((name, v));
    }
    let tween = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let id =
            e.dm.create_internal("Tween")
                .map_err(|err| rt(err.to_string()))?;
        e.dm.set_unchecked(id, "Instance", Variant::Ref(Some(target)));
        e.dm.set_unchecked(id, "PlaybackState", Variant::Enum(BEGIN));
        id
    };
    with_state(lua, |s| {
        s.tween_service.tweens.insert(
            tween,
            TweenData {
                target,
                info,
                goals,
                starts: Vec::new(),
                elapsed: 0.0,
                delay_left: info.delay,
                repeats_done: 0,
                reversing: false,
                state: BEGIN,
            },
        )
    });
    ret(lua, push_instance(lua, tween)?)
}

fn get_value(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let alpha = crate::convert::arg_number(args.first()).unwrap_or(0.0) as f32;
    let style = args
        .get(1)
        .and_then(|v| crate::convert::enum_value(lua, v, "EasingStyle").ok())
        .unwrap_or(0);
    let direction = args
        .get(2)
        .and_then(|v| crate::convert::enum_value(lua, v, "EasingDirection").ok())
        .unwrap_or(0);
    ret(lua, ease(style, direction, alpha) as f64)
}

fn capture_starts(lua: &Lua, id: InstanceId) {
    let (target, names): (InstanceId, Vec<String>) = match with_state(lua, |s| {
        s.tween_service
            .tweens
            .get(&id)
            .map(|t| (t.target, t.goals.iter().map(|g| g.0.clone()).collect()))
    }) {
        Some(x) => x,
        None => return,
    };
    let starts: Vec<Variant> = {
        let eng = engine(lua);
        let e = eng.lock();
        names
            .iter()
            .map(|n| e.dm.get(target, n).unwrap_or(Variant::Nil))
            .collect()
    };
    with_state(lua, |s| {
        if let Some(t) = s.tween_service.tweens.get_mut(&id) {
            t.starts = starts;
            t.elapsed = 0.0;
            t.delay_left = t.info.delay;
            t.repeats_done = 0;
            t.reversing = false;
        }
    });
}

fn play(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let state = with_state(lua, |s| s.tween_service.tweens.get(&id).map(|t| t.state));
    let Some(state) = state else {
        return none();
    };
    if state == PLAYING || state == DELAYED {
        return none();
    }
    if state != PAUSED {
        capture_starts(lua, id);
    }
    let delayed = with_state(lua, |s| {
        s.tween_service
            .tweens
            .get(&id)
            .is_some_and(|t| t.delay_left > 0.0)
    });
    set_state(lua, id, if delayed { DELAYED } else { PLAYING });
    none()
}

fn pause(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let state = with_state(lua, |s| s.tween_service.tweens.get(&id).map(|t| t.state));
    if matches!(state, Some(PLAYING) | Some(DELAYED)) {
        set_state(lua, id, PAUSED);
    }
    none()
}

fn cancel(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let state = with_state(lua, |s| s.tween_service.tweens.get(&id).map(|t| t.state));
    if matches!(state, Some(PLAYING) | Some(DELAYED) | Some(PAUSED)) {
        set_state(lua, id, CANCELLED);
        engine(lua)
            .lock()
            .dm
            .fire_signal(id, "Completed", vec![Variant::Enum(CANCELLED)]);
    }
    none()
}

pub fn tween_info(lua: &Lua, id: InstanceId) -> Option<LTweenInfo> {
    with_state(lua, |s| s.tween_service.tweens.get(&id).map(|t| t.info))
}

pub fn step(lua: &Lua, dt: f64) {
    let mut writes: Vec<(InstanceId, Vec<(String, Variant)>)> = Vec::new();
    let mut finished = Vec::new();
    let mut started = Vec::new();
    with_state(lua, |s| {
        for (&id, t) in s.tween_service.tweens.iter_mut() {
            if t.state == DELAYED {
                t.delay_left -= dt;
                if t.delay_left > 0.0 {
                    continue;
                }
                t.state = PLAYING;
                started.push(id);
            }
            if t.state != PLAYING {
                continue;
            }
            t.elapsed += dt;
            let total = t.info.time.max(0.0);
            let mut alpha = if total <= 0.0 {
                1.0
            } else {
                (t.elapsed / total).min(1.0)
            };
            let done_leg = alpha >= 1.0;
            if t.reversing {
                alpha = 1.0 - alpha;
            }
            let e = ease(t.info.style, t.info.direction, alpha as f32);
            let values = t
                .goals
                .iter()
                .zip(t.starts.iter())
                .map(|((name, goal), start)| (name.clone(), interpolate(start, goal, e)))
                .collect();
            writes.push((t.target, values));
            if done_leg {
                if t.info.reverses && !t.reversing {
                    t.reversing = true;
                    t.elapsed = 0.0;
                } else if t.info.repeat_count < 0 || t.repeats_done < t.info.repeat_count {
                    t.repeats_done += 1;
                    t.reversing = false;
                    t.elapsed = 0.0;
                    t.delay_left = t.info.delay;
                    if t.delay_left > 0.0 {
                        t.state = DELAYED;
                    }
                } else {
                    t.state = COMPLETED;
                    finished.push(id);
                }
            }
        }
    });
    if writes.is_empty() && finished.is_empty() && started.is_empty() {
        return;
    }
    let eng = engine(lua);
    let mut e = eng.lock();
    for id in started {
        e.dm.set_unchecked(id, "PlaybackState", Variant::Enum(PLAYING));
    }
    for (target, values) in writes {
        if !e.dm.contains(target) {
            continue;
        }
        for (name, v) in values {
            let _ = e.dm.set(target, &name, v);
        }
    }
    for id in finished {
        e.dm.set_unchecked(id, "PlaybackState", Variant::Enum(COMPLETED));
        e.dm.fire_signal(id, "Completed", vec![Variant::Enum(COMPLETED)]);
    }
}

#[cfg(test)]
mod tests {
    use super::ease;

    #[test]
    fn easing_hits_endpoints() {
        for style in 0..=10 {
            for dir in 0..=2 {
                assert!(ease(style, dir, 0.0).abs() < 1e-4, "{style} {dir}");
                assert!((ease(style, dir, 1.0) - 1.0).abs() < 1e-4, "{style} {dir}");
            }
        }
        assert!((ease(3, 0, 0.5) - 0.25).abs() < 1e-6);
        assert!((ease(3, 1, 0.5) - 0.75).abs() < 1e-6);
    }
}
