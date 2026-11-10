use super::{Registry, Ret, ret};
use crate::convert::{arg_bool, arg_number, enum_value, variant_to_lua};
use crate::runtime::{ThreadInfo, engine, spawn_function, with_state};
use crate::types::{LUDim2, rt};
use mlua::{FromLua, Function, Lua, MultiValue, RegistryKey, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_math::{UDim, UDim2};
use rbx_reflection::ValueType;
use rustc_hash::FxHashMap as HashMap;

pub struct Tween {
    start: UDim2,
    end: UDim2,
    total: f32,
    elapsed: f32,
    direction: u32,
    style: u32,
    callback: Option<(RegistryKey, crate::security::Identity)>,
}

#[derive(Default)]
pub struct TweenSet {
    position: Option<Tween>,
    size: Option<Tween>,
}

#[derive(Default)]
pub struct TweenState {
    pub objects: HashMap<InstanceId, TweenSet>,
}

pub fn register(r: &mut Registry) {
    r.add(
        "GuiObject",
        &["TweenPosition", "TweenPositionInternal"],
        tween_position,
    );
    r.add("GuiObject", &["TweenSize", "TweenSizeInternal"], tween_size);
    r.add(
        "GuiObject",
        &["TweenSizeAndPosition", "TweenSizeAndPositionInternal"],
        tween_size_and_position,
    );
}

fn scale(u: UDim2, f: f32) -> UDim2 {
    UDim2 {
        x: UDim {
            scale: u.x.scale * f,
            offset: (u.x.offset as f32 * f) as i32,
        },
        y: UDim {
            scale: u.y.scale * f,
            offset: (u.y.offset as f32 * f) as i32,
        },
    }
}

fn add(a: UDim2, b: UDim2) -> UDim2 {
    UDim2 {
        x: UDim {
            scale: a.x.scale + b.x.scale,
            offset: a.x.offset + b.x.offset,
        },
        y: UDim {
            scale: a.y.scale + b.y.scale,
            offset: a.y.offset + b.y.offset,
        },
    }
}

fn sub(a: UDim2, b: UDim2) -> UDim2 {
    add(a, scale(b, -1.0))
}

fn elastic_period(t_total: f32) -> f32 {
    let two_pi = rbx_base::flag_value("ElasticEasingUseTwoPi").is_some_and(|v| v == "true");
    let k = if two_pi {
        std::f32::consts::TAU
    } else {
        std::f32::consts::FRAC_PI_2
    };
    k / t_total
}

pub fn interpolate(
    direction: u32,
    style: u32,
    elapsed: f32,
    total: f32,
    start: UDim2,
    end: UDim2,
) -> UDim2 {
    let delta = sub(end, start);
    let lerp = |f: f32| add(scale(delta, f), start);
    let t = elapsed / total;
    match style {
        0 => lerp(t),
        1 => lerp(match direction {
            0 => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
            1 => (t * std::f32::consts::FRAC_PI_2).sin(),
            _ => -0.5 * ((std::f32::consts::PI * t).cos() - 1.0),
        }),
        2 => {
            let s = 1.70158f32;
            lerp(match direction {
                0 => t * t * ((s + 1.0) * t - s),
                1 => {
                    let t = t - 1.0;
                    t * t * ((s + 1.0) * t + s) + 1.0
                }
                _ => {
                    let mut t = elapsed / (total * 0.5);
                    if t < 1.0 {
                        0.5 * t * t * (((s * 1.525) + 1.0) * t - (s * 1.525))
                    } else {
                        t -= 2.0;
                        0.5 * (t * t * (((s * 1.525) + 1.0) * t + (s * 0.1525)) + 2.0)
                    }
                }
            })
        }
        3 => lerp(match direction {
            0 => t * t,
            1 => -t * (t - 2.0),
            _ => {
                let t = elapsed / (total * 0.5);
                if t < 1.0 {
                    0.5 * t * t
                } else {
                    -0.5 * ((t - 1.0) * (t - 3.0) - 1.0)
                }
            }
        }),
        4 => lerp(match direction {
            0 => t.powi(4),
            1 => -((t - 1.0).powi(4) - 1.0),
            _ => {
                let t = elapsed / (total * 0.5);
                if t < 1.0 {
                    0.5 * t.powi(4)
                } else {
                    -0.5 * ((t - 2.0).powi(4) - 2.0)
                }
            }
        }),
        5 => lerp(match direction {
            0 => t.powi(5),
            1 => (t - 1.0).powi(5) + 1.0,
            _ => {
                let t = elapsed / (total * 0.5);
                if t < 1.0 {
                    0.5 * t.powi(5)
                } else {
                    0.5 * ((t - 2.0).powi(5) + 2.0)
                }
            }
        }),
        6 => match direction {
            0 => add(
                sub(
                    delta,
                    interpolate(1, 6, total - elapsed, total, UDim2::default(), delta),
                ),
                start,
            ),
            1 => {
                let p = t;
                let f = if p < 1.0 / 2.75 {
                    7.5625 * p * p
                } else if p < 2.0 / 2.75 {
                    7.5625 * (p - 1.5 / 2.75).powi(2) + 0.75
                } else if p < 2.5 / 2.75 {
                    7.5625 * (p - 2.25 / 2.75).powi(2) + 0.9375
                } else {
                    7.5625 * (p - 2.625 / 2.75).powi(2) + 0.984375
                };
                lerp(f)
            }
            _ => {
                if elapsed < total * 0.5 {
                    add(
                        scale(
                            interpolate(0, 6, elapsed * 2.0, total, UDim2::default(), delta),
                            0.5,
                        ),
                        start,
                    )
                } else {
                    add(
                        add(
                            scale(
                                interpolate(
                                    1,
                                    6,
                                    elapsed * 2.0 - total,
                                    total,
                                    UDim2::default(),
                                    delta,
                                ),
                                0.5,
                            ),
                            scale(delta, 0.5),
                        ),
                        start,
                    )
                }
            }
        },
        7 => {
            if elapsed == 0.0 {
                return start;
            }
            if t >= 1.0 {
                return end;
            }
            let w = elastic_period(1.0);
            lerp(match direction {
                0 => {
                    let p = total * 0.3;
                    let s = p / 4.0;
                    let t = t - 1.0;
                    -(2f32.powf(10.0 * t) * ((t * total - s) * w / p).sin())
                }
                1 => {
                    let p = total * 0.3;
                    let s = p / 4.0;
                    1.0 + 2f32.powf(-10.0 * t) * ((t * total - s) * w / p).sin()
                }
                _ => {
                    let t = elapsed / (total * 0.5);
                    let p = total * (0.3 * 1.5);
                    let s = p / 4.0;
                    let t = t - 1.0;
                    if t < 0.0 {
                        -0.5 * 2f32.powf(10.0 * t) * ((t * total - s) * w / p).sin()
                    } else {
                        1.0 + 0.5 * 2f32.powf(-10.0 * t) * ((t * total - s) * w / p).sin()
                    }
                }
            })
        }
        _ => end,
    }
}

fn finish(lua: &Lua, callback: Option<(RegistryKey, crate::security::Identity)>, status: u32) {
    let Some((key, identity)) = callback else {
        return;
    };
    let func = lua.registry_value::<Function>(&key).ok();
    let _ = lua.remove_registry_value(key);
    let Some(func) = func else {
        return;
    };
    let Ok(value) = variant_to_lua(
        lua,
        &Variant::Enum(status),
        Some(&ValueType::Enum("TweenStatus".into())),
    ) else {
        return;
    };
    spawn_function(
        lua,
        func,
        MultiValue::from_vec(vec![value]),
        ThreadInfo {
            script: None,
            identity,
        },
    );
}

struct Args {
    direction: u32,
    style: u32,
    time: f32,
    overwrite: bool,
    callback: Option<Function>,
}

fn parse(lua: &Lua, args: &[Value]) -> Result<Args> {
    let direction = match args.first() {
        Some(Value::Nil) | None => 1,
        Some(v) => enum_value(lua, v, "EasingDirection")?,
    };
    let style = match args.get(1) {
        Some(Value::Nil) | None => 3,
        Some(v) => enum_value(lua, v, "EasingStyle")?,
    };
    Ok(Args {
        direction,
        style,
        time: arg_number(args.get(2)).unwrap_or(1.0) as f32,
        overwrite: arg_bool(args.get(3)),
        callback: match args.get(4) {
            Some(Value::Function(f)) => Some(f.clone()),
            _ => None,
        },
    })
}

fn udim2_arg(lua: &Lua, v: Option<&Value>) -> Result<UDim2> {
    match v {
        Some(v) => LUDim2::from_lua(v.clone(), lua)
            .map(|u| u.0)
            .map_err(|_| rt("Unable to cast value to UDim2")),
        None => Err(rt("Argument 1 missing or nil")),
    }
}

fn start(
    lua: &Lua,
    id: InstanceId,
    property: &str,
    end: UDim2,
    a: &Args,
    callback: Option<Function>,
) -> Result<bool> {
    let current = {
        let eng = engine(lua);
        let e = eng.lock();
        if !e.dm.in_tree(id) {
            return Err(rt("Can only tween objects in the workspace"));
        }
        match e.dm.get(id, property) {
            Ok(Variant::UDim2(u)) => u,
            _ => UDim2::default(),
        }
    };
    let existing = with_state(lua, |s| {
        let set = s.tweens.objects.entry(id).or_default();
        let slot = if property == "Position" {
            &mut set.position
        } else {
            &mut set.size
        };
        slot.is_some()
    });
    if existing && !a.overwrite {
        return Ok(false);
    }
    let callback = match callback {
        Some(f) => Some((
            lua.create_registry_value(f)?,
            crate::runtime::current_identity(lua),
        )),
        None => None,
    };
    let cancelled = with_state(lua, |s| {
        let set = s.tweens.objects.entry(id).or_default();
        let slot = if property == "Position" {
            &mut set.position
        } else {
            &mut set.size
        };
        let old = slot.take().and_then(|t| t.callback);
        *slot = Some(Tween {
            start: current,
            end,
            total: a.time,
            elapsed: 0.0,
            direction: a.direction,
            style: a.style,
            callback,
        });
        old
    });
    finish(lua, cancelled, 0);
    Ok(true)
}

fn tween_position(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let end = udim2_arg(lua, args.first())?;
    let a = parse(lua, args.get(1..).unwrap_or(&[]))?;
    let cb = a.callback.clone();
    ret(lua, start(lua, id, "Position", end, &a, cb)?)
}

fn tween_size(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let end = udim2_arg(lua, args.first())?;
    let a = parse(lua, args.get(1..).unwrap_or(&[]))?;
    let cb = a.callback.clone();
    ret(lua, start(lua, id, "Size", end, &a, cb)?)
}

fn tween_size_and_position(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let end_size = udim2_arg(lua, args.first())?;
    let end_pos = udim2_arg(lua, args.get(1))?;
    let a = parse(lua, args.get(2..).unwrap_or(&[]))?;
    if !a.overwrite {
        let busy = with_state(lua, |s| {
            s.tweens
                .objects
                .get(&id)
                .is_some_and(|t| t.position.is_some() || t.size.is_some())
        });
        if busy {
            return ret(lua, false);
        }
    }
    let cb = a.callback.clone();
    let size = start(lua, id, "Size", end_size, &a, cb)?;
    let pos = start(lua, id, "Position", end_pos, &a, None)?;
    ret(lua, size && pos)
}

pub fn step(lua: &Lua, dt: f32) {
    let ids: Vec<InstanceId> = with_state(lua, |s| s.tweens.objects.keys().copied().collect());
    for id in ids {
        for property in ["Position", "Size"] {
            let update = with_state(lua, |s| {
                let set = s.tweens.objects.get_mut(&id)?;
                let slot = if property == "Position" {
                    &mut set.position
                } else {
                    &mut set.size
                };
                let tween = slot.as_mut()?;
                tween.elapsed += dt;
                if tween.elapsed >= tween.total {
                    let done = slot.take()?;
                    Some((done.end, true, done.callback))
                } else {
                    Some((
                        interpolate(
                            tween.direction,
                            tween.style,
                            tween.elapsed,
                            tween.total,
                            tween.start,
                            tween.end,
                        ),
                        false,
                        None,
                    ))
                }
            });
            let Some((value, done, callback)) = update else {
                continue;
            };
            let alive = {
                let eng = engine(lua);
                let mut e = eng.lock();
                if e.dm.contains(id) {
                    let _ = e.dm.set(id, property, Variant::UDim2(value));
                    true
                } else {
                    false
                }
            };
            if done && alive {
                finish(lua, callback, 1);
            } else if done && let Some((k, _)) = callback {
                let _ = lua.remove_registry_value(k);
            }
        }
        with_state(lua, |s| {
            if s.tweens
                .objects
                .get(&id)
                .is_some_and(|t| t.position.is_none() && t.size.is_none())
            {
                s.tweens.objects.remove(&id);
            }
        });
    }
}
