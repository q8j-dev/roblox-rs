use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, enum_value, variant_to_lua};
use crate::instance::push_instance;
use crate::runtime::{MessageType, engine, expect_instance, fire_lua};
use crate::types::{LVector3, rt};
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant};
use rbx_physics::humanoid::{HState, HumanoidEvent, NUM_STATES};
use rbx_reflection::ValueType;

fn state_value(lua: &Lua, s: HState) -> Result<Value> {
    variant_to_lua(
        lua,
        &Variant::Enum(s as u32),
        Some(&ValueType::Enum("HumanoidStateType".into())),
    )
}

fn state_arg(lua: &Lua, v: Option<&Value>) -> Result<u32> {
    match v {
        Some(v) => enum_value(lua, v, "HumanoidStateType"),
        None => Err(rt("Argument 1 missing or nil")),
    }
}

fn with_sim<R>(
    lua: &Lua,
    id: InstanceId,
    f: impl FnOnce(&mut rbx_physics::humanoid::HumanoidSim) -> R,
) -> Option<R> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let world = e.world.clone();
    let mut w = world.lock();
    w.sync(&mut e.dm);
    w.humanoid_mut(id).map(f)
}

fn vec3_arg(v: Option<&Value>) -> Result<rbx_math::Vec3> {
    match v {
        Some(Value::UserData(ud)) => ud
            .borrow::<LVector3>()
            .map(|x| x.0)
            .map_err(|_| rt("Unable to cast value to Vector3")),
        None | Some(Value::Nil) => Err(rt("Argument 1 missing or nil")),
        _ => Err(rt("Unable to cast value to Vector3")),
    }
}

fn move_to(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let pos = vec3_arg(args.first())?;
    let part = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(v) => Some(expect_instance(lua, v, "BasePart")?),
    };
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        let local = match part {
            Some(p) => {
                e.dm.get(p, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default()
                    .point_to_object_space(pos)
            }
            None => pos,
        };
        let _ = e.dm.set(id, "WalkToPart", Variant::Ref(part));
        let _ = e.dm.set(
            id,
            "WalkToPoint",
            Variant::Vector3(local + rbx_math::Vec3::X),
        );
        let _ = e.dm.set(id, "WalkToPoint", Variant::Vector3(local));
        let _ = e.dm.set(
            id,
            "MoveDirectionInternal",
            Variant::Vector3(rbx_math::Vec3::ZERO),
        );
    }
    with_sim(lua, id, |h| {
        h.set_walk_mode(true);
        h.move_direction = rbx_math::Vec3::ZERO;
    });
    none()
}

fn move_(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let mut v = vec3_arg(args.first())?;
    let relative = arg_bool(args.get(1));
    let dir = if v == rbx_math::Vec3::ZERO {
        rbx_math::Vec3::ZERO
    } else {
        if v.length() > 1.0 {
            v = v.normalize();
        }
        let percent = v.length();
        with_sim(lua, id, |h| h.percent_walk_speed = percent.clamp(0.0, 1.0));
        let unit = v.normalize_or_zero();
        let swimming = drop_and_state(lua, id) == Some(HState::Swimming);
        if relative {
            let eng = engine(lua);
            let e = eng.lock();
            let camera =
                e.dm.find_service("Workspace")
                    .and_then(|ws| e.dm.get_ref(ws, "CurrentCamera"));
            match camera {
                Some(cam) => {
                    let cf =
                        e.dm.get(cam, "CFrame")
                            .ok()
                            .and_then(|v| v.as_cframe())
                            .unwrap_or_default();
                    if swimming {
                        cf.vector_to_world_space(unit)
                    } else {
                        let look = cf.look_vector() * rbx_math::Vec3::new(1.0, 0.0, 1.0);
                        let flat = rbx_math::CFrame::look_at(
                            cf.translation,
                            cf.translation + look,
                            rbx_math::Vec3::Y,
                        );
                        let mut d = flat.vector_to_world_space(unit);
                        d.y = 0.0;
                        d.normalize_or_zero()
                    }
                }
                None => return none(),
            }
        } else {
            unit
        }
    };
    set_move_direction(lua, id, dir);
    none()
}

fn drop_and_state(lua: &Lua, id: InstanceId) -> Option<HState> {
    let world = engine(lua).lock().world.clone();
    let w = world.try_lock()?;
    w.humanoid(id).map(|h| h.state)
}

fn set_move_direction(lua: &Lua, id: InstanceId, dir: rbx_math::Vec3) {
    let dir = if dir.length() > 1.0 {
        dir.normalize()
    } else {
        dir
    };
    let changed = with_sim(lua, id, |h| {
        let changed = h.move_direction != dir;
        h.move_direction = dir;
        if dir != rbx_math::Vec3::ZERO {
            h.set_walk_mode(false);
        }
        changed
    })
    .unwrap_or(true);
    if changed {
        let _ = engine(lua)
            .lock()
            .dm
            .set(id, "MoveDirectionInternal", Variant::Vector3(dir));
    }
}

fn change_state(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let s = state_arg(lua, args.first())?;
    if let Some(state) = HState::from_u32(s) {
        with_sim(lua, id, |h| h.lua_state = Some(state));
    }
    none()
}

fn get_state(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    match with_sim(lua, id, |h| h.state) {
        Some(s) => ret(lua, state_value(lua, s)?),
        None => ret(
            lua,
            variant_to_lua(
                lua,
                &Variant::Enum(18),
                Some(&ValueType::Enum("HumanoidStateType".into())),
            )?,
        ),
    }
}

fn set_state_enabled(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let s = state_arg(lua, args.first())?;
    let enabled = arg_bool(args.get(1));
    if (s as usize) >= NUM_STATES {
        engine(lua).lock().print(
            MessageType::Error,
            "Invalid state passed to SetStateEnabled.",
        );
        return none();
    }
    with_sim(lua, id, |h| h.enabled[s as usize] = enabled);
    if let Some(state) = HState::from_u32(s) {
        let args = MultiValue::from_vec(vec![state_value(lua, state)?, Value::Boolean(enabled)]);
        fire_lua(lua, id, "StateEnabledChanged", args);
    }
    none()
}

fn get_state_enabled(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let s = state_arg(lua, args.first())?;
    if (s as usize) >= NUM_STATES {
        engine(lua).lock().print(
            MessageType::Error,
            "Invalid state passed to GetStateEnabled.",
        );
        return ret(lua, true);
    }
    let v = with_sim(lua, id, |h| h.enabled[s as usize]).unwrap_or(true);
    ret(lua, v)
}

fn take_damage(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let amount = arg_number(args.first()).unwrap_or(0.0) as f32;
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(character) = e.dm.parent_of(id) else {
        return none();
    };
    let torso =
        e.dm.find_first_child(character, "HumanoidRootPart", false)
            .or_else(|| e.dm.find_first_child(character, "Torso", false))
            .or_else(|| e.dm.find_first_child(character, "UpperTorso", false));
    let Some(torso) = torso else {
        return none();
    };
    let shielded =
        e.dm.parent_of(torso)
            .is_some_and(|p| e.dm.find_first_child_of_class(p, "ForceField").is_some());
    if !shielded {
        let h =
            e.dm.get(id, "Health_XML")
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0);
        let _ = e.dm.set(id, "Health_XML", Variant::Float(h - amount));
    }
    none()
}

fn set_click_to_walk_enabled(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let enabled = arg_bool(args.first());
    with_sim(lua, id, |h| h.click_to_walk = enabled);
    none()
}

pub fn fire_events(lua: &Lua, events: Vec<(InstanceId, HumanoidEvent)>) {
    for (id, ev) in events {
        let alive = engine(lua).lock().dm.contains(id);
        if !alive {
            continue;
        }
        let (name, args): (&'static str, Vec<Value>) = match ev {
            HumanoidEvent::StateChanged(a, b) => match (state_value(lua, a), state_value(lua, b)) {
                (Ok(x), Ok(y)) => ("StateChanged", vec![x, y]),
                _ => continue,
            },
            HumanoidEvent::Running(s) => ("Running", vec![Value::Number(s as f64)]),
            HumanoidEvent::Climbing(s) => ("Climbing", vec![Value::Number(s as f64)]),
            HumanoidEvent::Swimming(s) => ("Swimming", vec![Value::Number(s as f64)]),
            HumanoidEvent::Strafing(b) => ("Strafing", vec![Value::Boolean(b)]),
            HumanoidEvent::FallingDown(b) => ("FallingDown", vec![Value::Boolean(b)]),
            HumanoidEvent::Ragdoll(b) => ("Ragdoll", vec![Value::Boolean(b)]),
            HumanoidEvent::GettingUp(b) => ("GettingUp", vec![Value::Boolean(b)]),
            HumanoidEvent::Jumping(b) => ("Jumping", vec![Value::Boolean(b)]),
            HumanoidEvent::FreeFalling(b) => ("FreeFalling", vec![Value::Boolean(b)]),
            HumanoidEvent::PlatformStanding(b) => ("PlatformStanding", vec![Value::Boolean(b)]),
            HumanoidEvent::Seated(b, seat) => {
                let s = match seat {
                    Some(p) => push_instance(lua, p).unwrap_or(Value::Nil),
                    None => Value::Nil,
                };
                ("Seated", vec![Value::Boolean(b), s])
            }
            HumanoidEvent::Died => ("Died", vec![]),
            HumanoidEvent::MoveToFinished(b) => ("MoveToFinished", vec![Value::Boolean(b)]),
        };
        fire_lua(lua, id, name, MultiValue::from_vec(args));
    }
}

fn equip_tool(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tool = match args.first() {
        Some(Value::Nil) | None => return none(),
        Some(v) => expect_instance(lua, v, "Instance")?,
    };
    let warning = engine(lua).lock().dm.humanoid_equip_tool(id, tool);
    if let Some(w) = warning {
        engine(lua).lock().print(MessageType::Warning, w);
    }
    none()
}

fn unequip_tools(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let warning = engine(lua).lock().dm.humanoid_unequip_tools(id).err();
    if let Some(w) = warning {
        engine(lua).lock().print(MessageType::Warning, w);
    }
    none()
}

fn tool_activate(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let activated = engine(lua).lock().dm.tool_activate(id);
    if !activated {
        engine(lua).lock().print(
            MessageType::Warning,
            "Tool:Activate() called from script when tool is not equipped. Tool will not be activated.",
        );
    }
    none()
}

pub fn register(r: &mut Registry) {
    r.add("Humanoid", &["EquipTool"], equip_tool);
    r.add("Humanoid", &["UnequipTools"], unequip_tools);
    r.add("Tool", &["Activate"], tool_activate);
    r.add("Humanoid", &["MoveTo", "moveTo"], move_to);
    r.add("Humanoid", &["Move"], move_);
    r.add("Humanoid", &["ChangeState"], change_state);
    r.add("Humanoid", &["GetState"], get_state);
    r.add("Humanoid", &["SetStateEnabled"], set_state_enabled);
    r.add("Humanoid", &["GetStateEnabled"], get_state_enabled);
    r.add("Humanoid", &["TakeDamage", "takeDamage"], take_damage);
    r.add(
        "Humanoid",
        &["SetClickToWalkEnabled"],
        set_click_to_walk_enabled,
    );
}
