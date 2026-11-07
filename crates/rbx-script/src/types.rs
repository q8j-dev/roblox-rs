use crate::format::float_str;
use mlua::{
    Error, FromLua, Lua, MetaMethod, MultiValue, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rbx_math::{
    Axes, BrickColor, CFrame, Color3, ColorSequence, ColorSequenceKeypoint, Faces, Mat3, NormalId,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Quat, Ray, Rect2D,
    Region3, Region3int16, UDim, UDim2, Vec2, Vec3, Vector2int16, Vector3int16,
    normal_id_to_vector,
};
use rbx_reflection::database;

pub fn rt(msg: impl Into<String>) -> Error {
    Error::RuntimeError(msg.into())
}

pub fn not_member(name: &str) -> Error {
    rt(format!("{name} is not a valid member"))
}

pub fn cannot_assign(name: &str) -> Error {
    rt(format!("{name} cannot be assigned to"))
}

macro_rules! from_lua_userdata {
    ($t:ty, $name:expr) => {
        impl FromLua for $t {
            fn from_lua(value: Value, _: &Lua) -> Result<Self> {
                match &value {
                    Value::UserData(ud) => match ud.borrow::<Self>() {
                        Ok(v) => Ok((*v).clone()),
                        Err(_) => Err(Error::FromLuaConversionError {
                            from: "userdata",
                            to: $name.to_string(),
                            message: None,
                        }),
                    },
                    other => Err(Error::FromLuaConversionError {
                        from: other.type_name(),
                        to: $name.to_string(),
                        message: Some(format!("{} expected, got {}", $name, other.type_name())),
                    }),
                }
            }
        }
    };
}

pub fn to_f32(v: &Value) -> f32 {
    match v {
        Value::Number(n) => *n as f32,
        Value::Integer(i) => *i as f32,
        Value::String(s) => s
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0) as f32,
        _ => 0.0,
    }
}

pub fn to_i64(v: &Value) -> i64 {
    match v {
        Value::Number(n) => *n as i64,
        Value::Integer(i) => *i,
        Value::String(s) => s
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0) as i64,
        _ => 0,
    }
}

pub fn is_number(v: &Value) -> bool {
    matches!(v, Value::Number(_) | Value::Integer(_))
        || matches!(v, Value::String(s) if s.to_str().ok().and_then(|s| s.trim().parse::<f64>().ok()).is_some())
}

fn args_f32(args: &MultiValue, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| args.get(i).map(to_f32).unwrap_or(0.0))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LVector3(pub Vec3);
from_lua_userdata!(LVector3, "Vector3");

pub fn vec3_str(v: Vec3) -> String {
    format!("{}, {}, {}", float_str(v.x), float_str(v.y), float_str(v.z))
}

fn fuzzy_eq_vec(a: Vec3, b: Vec3, eps: f32) -> bool {
    (a - b).length_squared() <= eps * eps * (1.0f32).max(a.length_squared()).max(b.length_squared())
}

impl UserData for LVector3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "x"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.x));
        }
        for n in ["Y", "y"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.y));
        }
        for n in ["Z", "z"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.z));
        }
        for n in ["unit", "Unit"] {
            f.add_field_method_get(n, |_, t| Ok(LVector3(rbx_math::g3d_direction(t.0))));
        }
        for n in ["magnitude", "Magnitude"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.length()));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        for n in ["lerp", "Lerp"] {
            m.add_method(n, |_, t, (o, a): (LVector3, f32)| {
                Ok(LVector3(t.0 + (o.0 - t.0) * a))
            });
        }
        m.add_method("Cross", |_, t, o: LVector3| Ok(LVector3(t.0.cross(o.0))));
        m.add_method("Dot", |_, t, o: LVector3| Ok(t.0.dot(o.0)));
        m.add_method("isClose", |_, t, (o, eps): (LVector3, Option<f32>)| {
            Ok(fuzzy_eq_vec(t.0, o.0, eps.map(f32::abs).unwrap_or(1e-5)))
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(vec3_str(t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LVector3>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| Ok(LVector3(-t.0)));
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LVector3, LVector3)| {
            Ok(LVector3(a.0 + b.0))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LVector3, LVector3)| {
            Ok(LVector3(a.0 - b.0))
        });
        m.add_meta_function(MetaMethod::Mul, |lua, (a, b): (Value, Value)| {
            vec3_binop(lua, a, b, "multiply", |x, y| x * y)
        });
        m.add_meta_function(MetaMethod::Div, |lua, (a, b): (Value, Value)| {
            vec3_binop(lua, a, b, "divide", |x, y| x / y)
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

fn vec3_binop(
    lua: &Lua,
    a: Value,
    b: Value,
    verb: &str,
    op: impl Fn(Vec3, Vec3) -> Vec3,
) -> Result<LVector3> {
    let av = LVector3::from_lua(a.clone(), lua).ok();
    let bv = LVector3::from_lua(b.clone(), lua).ok();
    match (av, bv) {
        (Some(x), Some(y)) => Ok(LVector3(op(x.0, y.0))),
        (Some(x), None) if is_number(&b) => Ok(LVector3(op(x.0, Vec3::splat(to_f32(&b))))),
        (None, Some(y)) if is_number(&a) => Ok(LVector3(op(Vec3::splat(to_f32(&a)), y.0))),
        _ => Err(rt(format!(
            "attempt to {verb} a Vector3 with an incompatible value type or nil"
        ))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LVector2(pub Vec2);
from_lua_userdata!(LVector2, "Vector2");

impl UserData for LVector2 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "x"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.x));
        }
        for n in ["Y", "y"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.y));
        }
        f.add_field_method_get("unit", |_, t| {
            let inv = 1.0 / t.0.length_squared().sqrt();
            Ok(LVector2(t.0 * inv))
        });
        f.add_field_method_get("magnitude", |_, t| Ok(t.0.length()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("lerp", |_, t, (o, a): (LVector2, f32)| {
            Ok(LVector2(t.0 + (o.0 - t.0) * a))
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("{}, {}", float_str(t.0.x), float_str(t.0.y)))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LVector2>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| Ok(LVector2(-t.0)));
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LVector2, LVector2)| {
            Ok(LVector2(a.0 + b.0))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LVector2, LVector2)| {
            Ok(LVector2(a.0 - b.0))
        });
        m.add_meta_function(MetaMethod::Mul, |lua, (a, b): (Value, Value)| {
            vec2_binop(lua, a, b, "multiply", |x, y| x * y)
        });
        m.add_meta_function(MetaMethod::Div, |lua, (a, b): (Value, Value)| {
            vec2_binop(lua, a, b, "divide", |x, y| x / y)
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

fn vec2_binop(
    lua: &Lua,
    a: Value,
    b: Value,
    verb: &str,
    op: impl Fn(Vec2, Vec2) -> Vec2,
) -> Result<LVector2> {
    let av = LVector2::from_lua(a.clone(), lua).ok();
    let bv = LVector2::from_lua(b.clone(), lua).ok();
    match (av, bv) {
        (Some(x), Some(y)) => Ok(LVector2(op(x.0, y.0))),
        (Some(x), None) if is_number(&b) => Ok(LVector2(op(x.0, Vec2::splat(to_f32(&b))))),
        (None, Some(y)) if is_number(&a) => Ok(LVector2(op(Vec2::splat(to_f32(&a)), y.0))),
        _ => Err(rt(format!(
            "attempt to {verb} a Vector2 with an incompatible value type or nil"
        ))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LVector3int16(pub Vector3int16);
from_lua_userdata!(LVector3int16, "Vector3int16");

impl UserData for LVector3int16 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "x"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.x as i32));
        }
        for n in ["Y", "y"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.y as i32));
        }
        for n in ["Z", "z"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.z as i32));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("{}, {}, {}", t.0.x, t.0.y, t.0.z))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LVector3int16>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_function(
            MetaMethod::Add,
            |_, (a, b): (LVector3int16, LVector3int16)| {
                Ok(LVector3int16(Vector3int16::new(
                    a.0.x.wrapping_add(b.0.x),
                    a.0.y.wrapping_add(b.0.y),
                    a.0.z.wrapping_add(b.0.z),
                )))
            },
        );
        m.add_meta_function(
            MetaMethod::Sub,
            |_, (a, b): (LVector3int16, LVector3int16)| {
                Ok(LVector3int16(Vector3int16::new(
                    a.0.x.wrapping_sub(b.0.x),
                    a.0.y.wrapping_sub(b.0.y),
                    a.0.z.wrapping_sub(b.0.z),
                )))
            },
        );
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| {
            Ok(LVector3int16(Vector3int16::new(
                t.0.x.wrapping_neg(),
                t.0.y.wrapping_neg(),
                t.0.z.wrapping_neg(),
            )))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LVector2int16(pub Vector2int16);
from_lua_userdata!(LVector2int16, "Vector2int16");

impl UserData for LVector2int16 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "x"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.x as i32));
        }
        for n in ["Y", "y"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.y as i32));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("{}, {}", t.0.x, t.0.y))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LVector2int16>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LCFrame(pub CFrame);
from_lua_userdata!(LCFrame, "CFrame");

pub fn cframe_str(c: &CFrame) -> String {
    let mut s = vec3_str(c.translation);
    for v in c.rows() {
        s.push_str(", ");
        s.push_str(&float_str(v));
    }
    s
}

fn many_vec3(lua: &Lua, args: MultiValue, f: impl Fn(Vec3) -> Vec3) -> Result<MultiValue> {
    if args.is_empty() {
        let v = Value::UserData(lua.create_userdata(LVector3(f(Vec3::ZERO)))?);
        return Ok(MultiValue::from_vec(vec![v]));
    }
    let mut out = Vec::new();
    for a in args {
        let v = LVector3::from_lua(a, lua)?;
        out.push(Value::UserData(lua.create_userdata(LVector3(f(v.0)))?));
    }
    Ok(MultiValue::from_vec(out))
}

impl UserData for LCFrame {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("p", |_, t| Ok(LVector3(t.0.translation)));
        f.add_field_method_get("lookVector", |_, t| Ok(LVector3(t.0.look_vector())));
        for n in ["X", "x"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.translation.x));
        }
        for n in ["Y", "y"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.translation.y));
        }
        for n in ["Z", "z"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.translation.z));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("inverse", |_, t, ()| Ok(LCFrame(t.0.inverse())));
        m.add_method("lerp", |_, t, (o, a): (LCFrame, f32)| {
            Ok(LCFrame(if a == 1.0 {
                o.0
            } else if a == 0.0 {
                t.0
            } else {
                t.0.lerp(&o.0, a)
            }))
        });
        m.add_method("toWorldSpace", |lua, t, args: MultiValue| {
            if args.is_empty() {
                return Ok(MultiValue::from_vec(vec![Value::UserData(
                    lua.create_userdata(*t)?,
                )]));
            }
            let mut out = Vec::new();
            for a in args {
                let b = LCFrame::from_lua(a, lua)?;
                out.push(Value::UserData(lua.create_userdata(LCFrame(t.0 * b.0))?));
            }
            Ok(MultiValue::from_vec(out))
        });
        m.add_method("toObjectSpace", |lua, t, args: MultiValue| {
            if args.is_empty() {
                return Ok(MultiValue::from_vec(vec![Value::UserData(
                    lua.create_userdata(LCFrame(t.0.inverse()))?,
                )]));
            }
            let mut out = Vec::new();
            for a in args {
                let b = LCFrame::from_lua(a, lua)?;
                out.push(Value::UserData(
                    lua.create_userdata(LCFrame(t.0.to_object_space(&b.0)))?,
                ));
            }
            Ok(MultiValue::from_vec(out))
        });
        m.add_method("pointToWorldSpace", |lua, t, args: MultiValue| {
            let c = t.0;
            many_vec3(lua, args, |v| c.point_to_world_space(v))
        });
        m.add_method("pointToObjectSpace", |lua, t, args: MultiValue| {
            let c = t.0;
            many_vec3(lua, args, |v| c.point_to_object_space(v))
        });
        m.add_method("vectorToWorldSpace", |lua, t, args: MultiValue| {
            let c = t.0;
            many_vec3(lua, args, |v| c.vector_to_world_space(v))
        });
        m.add_method("vectorToObjectSpace", |lua, t, args: MultiValue| {
            let c = t.0;
            many_vec3(lua, args, |v| c.vector_to_object_space(v))
        });
        m.add_method("toEulerAnglesXYZ", |_, t, ()| {
            let (x, y, z) = t.0.to_euler_xyz();
            Ok((x, y, z))
        });
        m.add_method("components", |_, t, ()| {
            let r = t.0.rows();
            let p = t.0.translation;
            Ok((
                p.x, p.y, p.z, r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8],
            ))
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(cframe_str(&t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LCFrame>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LCFrame, LVector3)| {
            Ok(LCFrame(CFrame::new(a.0.rotation, a.0.translation + b.0)))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LCFrame, LVector3)| {
            Ok(LCFrame(CFrame::new(a.0.rotation, a.0.translation - b.0)))
        });
        m.add_meta_function(
            MetaMethod::Mul,
            |lua, (a, b): (LCFrame, Value)| -> Result<Value> {
                if let Ok(c) = LCFrame::from_lua(b.clone(), lua) {
                    return Ok(Value::UserData(lua.create_userdata(LCFrame(a.0 * c.0))?));
                }
                let v = LVector3::from_lua(b, lua)?;
                Ok(Value::UserData(lua.create_userdata(LVector3(
                    a.0.point_to_world_space(v.0),
                ))?))
            },
        );
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LColor3(pub Color3);
from_lua_userdata!(LColor3, "Color3");

pub fn color3_str(c: Color3) -> String {
    format!(
        "{}, {}, {}",
        crate::format::format_g(c.r as f64, 6),
        crate::format::format_g(c.g as f64, 6),
        crate::format::format_g(c.b as f64, 6)
    )
}

impl UserData for LColor3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("r", |_, t| Ok(t.0.r));
        f.add_field_method_get("g", |_, t| Ok(t.0.g));
        f.add_field_method_get("b", |_, t| Ok(t.0.b));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(color3_str(t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LColor3>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LBrickColor(pub BrickColor);
from_lua_userdata!(LBrickColor, "BrickColor");

impl UserData for LBrickColor {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["number", "Number"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.number() as i32));
        }
        f.add_field_method_get("Color", |_, t| Ok(LColor3(t.0.color3())));
        f.add_field_method_get("r", |_, t| Ok(t.0.color3().r));
        f.add_field_method_get("g", |_, t| Ok(t.0.color3().g));
        f.add_field_method_get("b", |_, t| Ok(t.0.color3().b));
        for n in ["name", "Name"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.name()));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(t.0.name()));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LBrickColor>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LUDim(pub UDim);
from_lua_userdata!(LUDim, "UDim");

fn udim_str(u: UDim) -> String {
    format!("{}, {}", float_str(u.scale), u.offset)
}

impl UserData for LUDim {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Scale", |_, t| Ok(t.0.scale));
        f.add_field_method_get("Offset", |_, t| Ok(t.0.offset));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(udim_str(t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LUDim>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LUDim, LUDim)| {
            Ok(LUDim(UDim::new(
                a.0.scale + b.0.scale,
                a.0.offset.wrapping_add(b.0.offset),
            )))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LUDim, LUDim)| {
            Ok(LUDim(UDim::new(
                a.0.scale - b.0.scale,
                a.0.offset.wrapping_sub(b.0.offset),
            )))
        });
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| {
            Ok(LUDim(UDim::new(-t.0.scale, t.0.offset.wrapping_neg())))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LUDim2(pub UDim2);
from_lua_userdata!(LUDim2, "UDim2");

impl UserData for LUDim2 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "Width"] {
            f.add_field_method_get(n, |_, t| Ok(LUDim(t.0.x)));
        }
        for n in ["Y", "Height"] {
            f.add_field_method_get(n, |_, t| Ok(LUDim(t.0.y)));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("{{{}}}, {{{}}}", udim_str(t.0.x), udim_str(t.0.y)))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LUDim2>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LUDim2, LUDim2)| {
            Ok(LUDim2(UDim2 {
                x: UDim::new(
                    a.0.x.scale + b.0.x.scale,
                    a.0.x.offset.wrapping_add(b.0.x.offset),
                ),
                y: UDim::new(
                    a.0.y.scale + b.0.y.scale,
                    a.0.y.offset.wrapping_add(b.0.y.offset),
                ),
            }))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LUDim2, LUDim2)| {
            Ok(LUDim2(UDim2 {
                x: UDim::new(
                    a.0.x.scale - b.0.x.scale,
                    a.0.x.offset.wrapping_sub(b.0.x.offset),
                ),
                y: UDim::new(
                    a.0.y.scale - b.0.y.scale,
                    a.0.y.offset.wrapping_sub(b.0.y.offset),
                ),
            }))
        });
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| {
            Ok(LUDim2(UDim2 {
                x: UDim::new(-t.0.x.scale, t.0.x.offset.wrapping_neg()),
                y: UDim::new(-t.0.y.scale, t.0.y.offset.wrapping_neg()),
            }))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRay(pub Ray);
from_lua_userdata!(LRay, "Ray");

impl UserData for LRay {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Origin", |_, t| Ok(LVector3(t.0.origin)));
        f.add_field_method_get("Direction", |_, t| Ok(LVector3(t.0.direction)));
        f.add_field_method_get("Unit", |_, t| {
            Ok(LRay(Ray::new(
                t.0.origin,
                rbx_math::g3d_direction(t.0.direction),
            )))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("ClosestPoint", |_, t, p: LVector3| {
            Ok(LVector3(t.0.closest_point(p.0)))
        });
        m.add_method("Distance", |_, t, p: LVector3| Ok(t.0.distance(p.0)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{{{}}}, {{{}}}",
                vec3_str(t.0.origin),
                vec3_str(t.0.direction)
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LRay>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRegion3(pub Region3);
from_lua_userdata!(LRegion3, "Region3");

impl UserData for LRegion3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("CFrame", |_, t| {
            Ok(LCFrame(CFrame::from_translation(t.0.center())))
        });
        f.add_field_method_get("Size", |_, t| Ok(LVector3(t.0.size())));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("ExpandToGrid", |_, t, res: f32| {
            if res <= 0.0 {
                return Err(rt("Resolution must be greater than 0"));
            }
            Ok(LRegion3(t.0.expand_to_grid(res)))
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{}; {}",
                cframe_str(&CFrame::from_translation(t.0.center())),
                vec3_str(t.0.size())
            ))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRegion3int16(pub Region3int16);
from_lua_userdata!(LRegion3int16, "Region3int16");

impl UserData for LRegion3int16 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Min", |_, t| Ok(LVector3int16(t.0.min)));
        f.add_field_method_get("Max", |_, t| Ok(LVector3int16(t.0.max)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{}, {}, {}; {}, {}, {}",
                t.0.min.x, t.0.min.y, t.0.min.z, t.0.max.x, t.0.max.y, t.0.max.z
            ))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRect2D(pub Rect2D);
from_lua_userdata!(LRect2D, "Rect2D");

impl UserData for LRect2D {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Min", |_, t| Ok(LVector2(t.0.min)));
        f.add_field_method_get("Max", |_, t| Ok(LVector2(t.0.max)));
        f.add_field_method_get("Width", |_, t| Ok(t.0.width()));
        f.add_field_method_get("Height", |_, t| Ok(t.0.height()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{}, {}, {}, {}",
                float_str(t.0.min.x),
                float_str(t.0.min.y),
                float_str(t.0.max.x),
                float_str(t.0.max.y)
            ))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LFaces(pub Faces);
from_lua_userdata!(LFaces, "Faces");

pub fn faces_str(f: Faces) -> String {
    let names = [
        (NormalId::Right, "Right"),
        (NormalId::Top, "Top"),
        (NormalId::Back, "Back"),
        (NormalId::Left, "Left"),
        (NormalId::Bottom, "Bottom"),
        (NormalId::Front, "Front"),
    ];
    names
        .iter()
        .filter(|(n, _)| f.has(*n))
        .map(|(_, s)| *s)
        .collect::<Vec<_>>()
        .join(", ")
}

impl UserData for LFaces {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Top", |_, t| Ok(t.0.has(NormalId::Top)));
        f.add_field_method_get("Bottom", |_, t| Ok(t.0.has(NormalId::Bottom)));
        f.add_field_method_get("Back", |_, t| Ok(t.0.has(NormalId::Back)));
        f.add_field_method_get("Front", |_, t| Ok(t.0.has(NormalId::Front)));
        f.add_field_method_get("Right", |_, t| Ok(t.0.has(NormalId::Right)));
        f.add_field_method_get("Left", |_, t| Ok(t.0.has(NormalId::Left)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(faces_str(t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LFaces>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(rt(format!(
                "{k} is not a valid member. Valid members are Top,Bottom,Left,Right,Back,Front"
            )))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LAxes(pub Axes);
from_lua_userdata!(LAxes, "Axes");

pub fn axes_str(a: Axes) -> String {
    let mut parts = Vec::new();
    if a.x() {
        parts.push("X");
    }
    if a.y() {
        parts.push("Y");
    }
    if a.z() {
        parts.push("Z");
    }
    parts.join(", ")
}

impl UserData for LAxes {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for n in ["X", "Right", "Left"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.x()));
        }
        for n in ["Y", "Top", "Bottom"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.y()));
        }
        for n in ["Z", "Back", "Front"] {
            f.add_field_method_get(n, |_, t| Ok(t.0.z()));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(axes_str(t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LAxes>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(rt(format!(
                "{k} is not a valid member, valid members are X,Y,Z,Top,Bottom,Left,Right,Front,Back"
            )))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LNumberRange(pub NumberRange);
from_lua_userdata!(LNumberRange, "NumberRange");

impl UserData for LNumberRange {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Min", |_, t| Ok(t.0.min));
        f.add_field_method_get("Max", |_, t| Ok(t.0.max));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{} {} ",
                crate::format::format_g(t.0.min as f64, 6),
                crate::format::format_g(t.0.max as f64, 6)
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LNumberRange>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LNumberSequenceKeypoint(pub NumberSequenceKeypoint);
from_lua_userdata!(LNumberSequenceKeypoint, "NumberSequenceKeypoint");

impl UserData for LNumberSequenceKeypoint {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Time", |_, t| Ok(t.0.time));
        f.add_field_method_get("Value", |_, t| Ok(t.0.value));
        f.add_field_method_get("Envelope", |_, t| Ok(t.0.envelope));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{} {} {} ",
                crate::format::format_g(t.0.time as f64, 6),
                crate::format::format_g(t.0.value as f64, 6),
                crate::format::format_g(t.0.envelope as f64, 6)
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LNumberSequenceKeypoint>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LNumberSequence(pub NumberSequence);
from_lua_userdata!(LNumberSequence, "NumberSequence");

impl UserData for LNumberSequence {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Keypoints", |lua, t| {
            let tbl = lua.create_table()?;
            for (i, k) in t.0.keypoints.iter().enumerate() {
                tbl.raw_set(i + 1, LNumberSequenceKeypoint(*k))?;
            }
            Ok(tbl)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            let mut s = String::new();
            for k in &t.0.keypoints {
                s.push_str(&format!(
                    "{} {} {} ",
                    crate::format::format_g(k.time as f64, 6),
                    crate::format::format_g(k.value as f64, 6),
                    crate::format::format_g(k.envelope as f64, 6)
                ));
            }
            Ok(s)
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LNumberSequence>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LColorSequenceKeypoint(pub ColorSequenceKeypoint);
from_lua_userdata!(LColorSequenceKeypoint, "ColorSequenceKeypoint");

impl UserData for LColorSequenceKeypoint {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Time", |_, t| Ok(t.0.time));
        f.add_field_method_get("Value", |_, t| Ok(LColor3(t.0.value)));
        f.add_field_method_get("Envelope", |_, t| Ok(t.0.envelope));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "{} {} {} {} {} ",
                crate::format::format_g(t.0.time as f64, 6),
                crate::format::format_g(t.0.value.r as f64, 6),
                crate::format::format_g(t.0.value.g as f64, 6),
                crate::format::format_g(t.0.value.b as f64, 6),
                crate::format::format_g(t.0.envelope as f64, 6)
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LColorSequenceKeypoint>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LColorSequence(pub ColorSequence);
from_lua_userdata!(LColorSequence, "ColorSequence");

impl UserData for LColorSequence {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Keypoints", |lua, t| {
            let tbl = lua.create_table()?;
            for (i, k) in t.0.keypoints.iter().enumerate() {
                tbl.raw_set(i + 1, LColorSequenceKeypoint(*k))?;
            }
            Ok(tbl)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            let mut s = String::new();
            for k in &t.0.keypoints {
                s.push_str(&format!(
                    "{} {} {} {} {} ",
                    crate::format::format_g(k.time as f64, 6),
                    crate::format::format_g(k.value.r as f64, 6),
                    crate::format::format_g(k.value.g as f64, 6),
                    crate::format::format_g(k.value.b as f64, 6),
                    crate::format::format_g(k.envelope as f64, 6)
                ));
            }
            Ok(s)
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LColorSequence>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LPhysicalProperties(pub PhysicalProperties);
from_lua_userdata!(LPhysicalProperties, "PhysicalProperties");

impl UserData for LPhysicalProperties {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Density", |_, t| {
            Ok(t.0.custom.map(|c| c.density).unwrap_or(0.0))
        });
        f.add_field_method_get("Friction", |_, t| {
            Ok(t.0.custom.map(|c| c.friction).unwrap_or(0.0))
        });
        f.add_field_method_get("Elasticity", |_, t| {
            Ok(t.0.custom.map(|c| c.elasticity).unwrap_or(0.0))
        });
        f.add_field_method_get("FrictionWeight", |_, t| {
            Ok(t.0.custom.map(|c| c.friction_weight).unwrap_or(0.0))
        });
        f.add_field_method_get("ElasticityWeight", |_, t| {
            Ok(t.0.custom.map(|c| c.elasticity_weight).unwrap_or(0.0))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(match t.0.custom {
                Some(c) => format!(
                    "{}, {}, {}, {}, {}",
                    float_str(c.density),
                    float_str(c.friction),
                    float_str(c.elasticity),
                    float_str(c.friction_weight),
                    float_str(c.elasticity_weight)
                ),
                None => "default".to_string(),
            })
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LPhysicalProperties>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(rt(format!(
                "{k} is not a valid member of PhysicalProperties"
            )))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LEnumItem {
    pub enum_name: &'static str,
    pub name: &'static str,
    pub value: u32,
}
from_lua_userdata!(LEnumItem, "EnumItem");

impl LEnumItem {
    pub fn lookup(enum_name: &str, value: u32) -> Option<LEnumItem> {
        let e = database().enum_desc(enum_name)?;
        let item = e.items.iter().find(|i| i.value == value)?;
        Some(LEnumItem {
            enum_name: e.name.as_str(),
            name: item.name.as_str(),
            value,
        })
    }

    pub fn by_name(enum_name: &str, name: &str) -> Option<LEnumItem> {
        let e = database().enum_desc(enum_name)?;
        let value = e.from_name(name)?;
        LEnumItem::lookup(enum_name, value)
    }
}

impl UserData for LEnumItem {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Name", |_, t| Ok(t.name));
        f.add_field_method_get("Value", |_, t| Ok(t.value));
        f.add_field_method_get("EnumType", |_, t| Ok(LEnum(t.enum_name)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("Enum.{}.{}", t.enum_name, t.name))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LEnumItem>().map(|v| *v == *t).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LEnum(pub &'static str);
from_lua_userdata!(LEnum, "Enum");

impl UserData for LEnum {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetEnumItems", |lua, t, ()| {
            let tbl = lua.create_table()?;
            if let Some(e) = database().enum_desc(t.0) {
                for (i, item) in e.items.iter().enumerate() {
                    tbl.raw_set(
                        i + 1,
                        LEnumItem {
                            enum_name: e.name.as_str(),
                            name: item.name.as_str(),
                            value: item.value,
                        },
                    )?;
                }
            }
            Ok(tbl)
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(t.0.to_string()));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LEnum>().map(|v| *v == *t).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |lua, t, k: Value| -> Result<Value> {
            let name = match &k {
                Value::String(s) => s.to_str()?.to_string(),
                _ => return Err(rt(format!("{} is not a valid EnumItem", k.type_name()))),
            };
            let e = database()
                .enum_desc(t.0)
                .ok_or_else(|| rt(format!("{name} is not a valid EnumItem")))?;
            let item = e
                .items
                .iter()
                .chain(e.legacy.iter())
                .find(|i| i.name == name)
                .ok_or_else(|| rt(format!("{name} is not a valid EnumItem")))?;
            let real = LEnumItem::lookup(t.0, item.value)
                .ok_or_else(|| rt(format!("{name} is not a valid EnumItem")))?;
            Ok(Value::UserData(lua.create_userdata(real)?))
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LEnums;

impl UserData for LEnums {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetEnums", |lua, _, ()| {
            let tbl = lua.create_table()?;
            let mut names: Vec<&'static str> =
                database().enums().map(|e| e.name.as_str()).collect();
            names.sort_unstable();
            for (i, n) in names.into_iter().enumerate() {
                tbl.raw_set(i + 1, LEnum(n))?;
            }
            Ok(tbl)
        });
        m.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Enums"));
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<LEnum> {
            database()
                .enum_desc(&k)
                .map(|e| LEnum(e.name.as_str()))
                .ok_or_else(|| rt(format!("{k} is not a valid member")))
        });
    }
}

fn enum_item_from(lua: &Lua, v: &Value, expected: &str) -> Option<u32> {
    let item = LEnumItem::from_lua(v.clone(), lua).ok()?;
    (item.enum_name == expected).then_some(item.value)
}

pub fn normal_id_arg(lua: &Lua, v: &Value) -> Option<NormalId> {
    if let Some(n) = enum_item_from(lua, v, "NormalId") {
        return NormalId::from_index(n as u8);
    }
    if is_number(v) {
        return NormalId::from_index(to_i64(v) as u8);
    }
    if let Value::String(s) = v {
        let s = s.to_str().ok()?;
        return LEnumItem::by_name("NormalId", &s)
            .and_then(|i| NormalId::from_index(i.value as u8));
    }
    None
}

pub fn register_constructors(lua: &Lua, g: &Table) -> Result<()> {
    let make = |name: &str| -> Result<Table> {
        let t = lua.create_table()?;
        g.set(name, t.clone())?;
        Ok(t)
    };

    let v3 = make("Vector3")?;
    v3.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let a = args_f32(&args, 3);
            Ok(LVector3(Vec3::new(a[0], a[1], a[2])))
        })?,
    )?;
    v3.set(
        "FromNormalId",
        lua.create_function(|lua, v: Value| {
            let id = enum_item_from(lua, &v, "NormalId")
                .and_then(|n| NormalId::from_index(n as u8))
                .ok_or_else(|| rt("Vector3.FromNormalId expects Enum.NormalId input"))?;
            Ok(LVector3(normal_id_to_vector(id)))
        })?,
    )?;
    v3.set(
        "FromAxis",
        lua.create_function(|lua, v: Value| {
            let axis = enum_item_from(lua, &v, "Axis")
                .ok_or_else(|| rt("Vector3.FromAxis expects Enum.Axis input"))?;
            Ok(LVector3(match axis {
                0 => Vec3::X,
                1 => Vec3::Y,
                _ => Vec3::Z,
            }))
        })?,
    )?;

    let v2 = make("Vector2")?;
    v2.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let a = args_f32(&args, 2);
            Ok(LVector2(Vec2::new(a[0], a[1])))
        })?,
    )?;

    let v3i = make("Vector3int16")?;
    v3i.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let g = |i: usize| args.get(i).map(to_i64).unwrap_or(0) as i16;
            Ok(LVector3int16(Vector3int16::new(g(0), g(1), g(2))))
        })?,
    )?;
    let v2i = make("Vector2int16")?;
    v2i.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let g = |i: usize| args.get(i).map(to_i64).unwrap_or(0) as i16;
            Ok(LVector2int16(Vector2int16 { x: g(0), y: g(1) }))
        })?,
    )?;

    let cf = make("CFrame")?;
    cf.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let n = args.len();
            let c = match n {
                0 => CFrame::IDENTITY,
                1 => CFrame::from_translation(LVector3::from_lua(args[0].clone(), lua)?.0),
                2 => {
                    let p = LVector3::from_lua(args[0].clone(), lua)?.0;
                    let at = LVector3::from_lua(args[1].clone(), lua)?.0;
                    CFrame::look_at(p, at, Vec3::Y)
                }
                3 => {
                    let a = args_f32(&args, 3);
                    CFrame::from_translation(Vec3::new(a[0], a[1], a[2]))
                }
                7 => {
                    let a = args_f32(&args, 7);
                    CFrame::from_quaternion(
                        Quat::from_xyzw(a[3], a[4], a[5], a[6]),
                        Vec3::new(a[0], a[1], a[2]),
                    )
                }
                12 => {
                    let a = args_f32(&args, 12);
                    CFrame::from_rows(
                        [a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11]],
                        Vec3::new(a[0], a[1], a[2]),
                    )
                }
                _ => return Err(rt(format!("Invalid number of arguments: {n}"))),
            };
            Ok(LCFrame(c))
        })?,
    )?;
    let angles = lua.create_function(|_, (x, y, z): (f64, f64, f64)| {
        Ok(LCFrame(CFrame::from_euler_xyz(
            x as f32, y as f32, z as f32,
        )))
    })?;
    cf.set("fromEulerAnglesXYZ", angles.clone())?;
    cf.set("Angles", angles)?;
    cf.set(
        "fromAxisAngle",
        lua.create_function(|_, (axis, angle): (LVector3, f64)| {
            Ok(LCFrame(CFrame::new(
                Mat3::from_axis_angle(rbx_math::g3d_direction(axis.0), angle as f32),
                Vec3::ZERO,
            )))
        })?,
    )?;

    let c3 = make("Color3")?;
    c3.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let a = args_f32(&args, 3);
            Ok(LColor3(Color3::new(a[0], a[1], a[2])))
        })?,
    )?;
    c3.set(
        "fromRGB",
        lua.create_function(|_, args: MultiValue| {
            let a = args_f32(&args, 3);
            Ok(LColor3(Color3::new(
                a[0] / 255.0,
                a[1] / 255.0,
                a[2] / 255.0,
            )))
        })?,
    )?;

    let bc = make("BrickColor")?;
    let new_bc = lua.create_function(|lua, args: MultiValue| {
        let count = args.len().min(3);
        Ok(LBrickColor(match count {
            0 => BrickColor::DEFAULT,
            1 => {
                let v = &args[0];
                if matches!(v, Value::Number(_) | Value::Integer(_)) {
                    BrickColor::new(to_i64(v))
                } else if let Value::String(s) = v {
                    match s.to_str()?.trim().parse::<f64>() {
                        Ok(n) => BrickColor::new(n as i64),
                        Err(_) => BrickColor::parse(&s.to_str()?),
                    }
                } else {
                    BrickColor::closest(LColor3::from_lua(v.clone(), lua)?.0)
                }
            }
            _ => {
                let a = args_f32(&args, count);
                BrickColor::closest(Color3::new(
                    a[0],
                    *a.get(1).unwrap_or(&0.0),
                    *a.get(2).unwrap_or(&0.0),
                ))
            }
        }))
    })?;
    bc.set("new", new_bc.clone())?;
    bc.set("New", new_bc)?;
    let random = lua.create_function(|_, ()| {
        Ok(LBrickColor(BrickColor::random(
            crate::runtime::random_seed(),
        )))
    })?;
    bc.set("random", random.clone())?;
    bc.set("Random", random)?;
    bc.set(
        "palette",
        lua.create_function(|_, i: Option<Value>| {
            let index = i.as_ref().map(to_i64).unwrap_or(0);
            if !(0..128).contains(&index) {
                return Err(rt(format!("palette index out of bounds ({index})")));
            }
            Ok(LBrickColor(BrickColor::palette_index(index as usize)))
        })?,
    )?;
    for (name, color) in [
        ("White", BrickColor::WHITE),
        ("Gray", BrickColor::GRAY),
        ("DarkGray", BrickColor::DARK_GRAY),
        ("Black", BrickColor::BLACK),
        ("Red", BrickColor::RED),
        ("Yellow", BrickColor::YELLOW),
        ("Green", BrickColor::GREEN),
        ("Blue", BrickColor::BLUE),
    ] {
        bc.set(
            name,
            lua.create_function(move |_, ()| Ok(LBrickColor(color)))?,
        )?;
    }

    let ud = make("UDim")?;
    ud.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            Ok(LUDim(UDim::new(
                args.front().map(to_f32).unwrap_or(0.0),
                args.get(1).map(to_i64).unwrap_or(0) as i32,
            )))
        })?,
    )?;
    let ud2 = make("UDim2")?;
    ud2.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            Ok(LUDim2(UDim2::new(
                args.front().map(to_f32).unwrap_or(0.0),
                args.get(1).map(to_i64).unwrap_or(0) as i32,
                args.get(2).map(to_f32).unwrap_or(0.0),
                args.get(3).map(to_i64).unwrap_or(0) as i32,
            )))
        })?,
    )?;

    let ray = make("Ray")?;
    ray.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let o = args
                .front()
                .cloned()
                .map(|v| LVector3::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or(Vec3::ZERO);
            let d = args
                .get(1)
                .cloned()
                .map(|v| LVector3::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or(Vec3::ZERO);
            Ok(LRay(Ray::new(o, d)))
        })?,
    )?;

    let r3 = make("Region3")?;
    r3.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let a = args
                .front()
                .cloned()
                .map(|v| LVector3::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or(Vec3::ZERO);
            let b = args
                .get(1)
                .cloned()
                .map(|v| LVector3::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or(Vec3::ZERO);
            Ok(LRegion3(Region3 { min: a, max: b }))
        })?,
    )?;
    let r3i = make("Region3int16")?;
    r3i.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let a = args
                .front()
                .cloned()
                .map(|v| LVector3int16::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or_default();
            let b = args
                .get(1)
                .cloned()
                .map(|v| LVector3int16::from_lua(v, lua))
                .transpose()?
                .map(|v| v.0)
                .unwrap_or_default();
            Ok(LRegion3int16(Region3int16 { min: a, max: b }))
        })?,
    )?;

    let rect = make("Rect2D")?;
    rect.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            if args.len() == 2 {
                let a = LVector2::from_lua(args[0].clone(), lua)?.0;
                let b = LVector2::from_lua(args[1].clone(), lua)?.0;
                return Ok(LRect2D(Rect2D { min: a, max: b }));
            }
            let a = args_f32(&args, 4);
            Ok(LRect2D(Rect2D::xyxy(a[0], a[1], a[2], a[3])))
        })?,
    )?;

    let faces = make("Faces")?;
    faces.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let mut f = Faces(0);
            for a in args.iter() {
                if let Some(n) = normal_id_arg(lua, a) {
                    f = f.with(n, true);
                } else {
                    return Err(rt("Faces.new expects Enum.NormalId values"));
                }
            }
            Ok(LFaces(f))
        })?,
    )?;
    let axes = make("Axes")?;
    axes.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            let mut mask = 0u8;
            for a in args.iter() {
                if let Some(v) = enum_item_from(lua, a, "Axis") {
                    mask |= 1 << v.min(2);
                } else if let Some(n) = normal_id_arg(lua, a) {
                    mask |= 1 << ((n as u8) % 3);
                } else {
                    return Err(rt("Axes.new expects Enum.Axis or Enum.NormalId values"));
                }
            }
            Ok(LAxes(Axes(mask)))
        })?,
    )?;

    let nr = make("NumberRange")?;
    nr.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let min = args.front().map(to_f32).unwrap_or(0.0);
            let max = if args.len() > 1 {
                to_f32(&args[1])
            } else {
                min
            };
            if max < min {
                return Err(rt("NumberRange: invalid range"));
            }
            Ok(LNumberRange(NumberRange::new(min, max)))
        })?,
    )?;

    let nsk = make("NumberSequenceKeypoint")?;
    nsk.set(
        "new",
        lua.create_function(|_, args: MultiValue| {
            let a = args_f32(&args, 3);
            Ok(LNumberSequenceKeypoint(NumberSequenceKeypoint {
                time: a[0],
                value: a[1],
                envelope: a[2],
            }))
        })?,
    )?;
    let ns = make("NumberSequence")?;
    ns.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            if let Some(Value::Table(t)) = args.front() {
                let mut keypoints = Vec::new();
                for v in t.clone().sequence_values::<Value>() {
                    keypoints.push(LNumberSequenceKeypoint::from_lua(v?, lua)?.0);
                }
                NumberSequence::validate(&keypoints).map_err(rt)?;
                return Ok(LNumberSequence(NumberSequence { keypoints }));
            }
            let a = args.front().map(to_f32).unwrap_or(0.0);
            let b = if args.len() > 1 { to_f32(&args[1]) } else { a };
            Ok(LNumberSequence(NumberSequence::range(a, b)))
        })?,
    )?;
    let csk = make("ColorSequenceKeypoint")?;
    csk.set(
        "new",
        lua.create_function(|lua, (t, c): (f32, LColor3)| {
            let _ = lua;
            Ok(LColorSequenceKeypoint(ColorSequenceKeypoint {
                time: t,
                value: c.0,
                envelope: 0.0,
            }))
        })?,
    )?;
    let cs = make("ColorSequence")?;
    cs.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            if let Some(Value::Table(t)) = args.front() {
                let mut keypoints = Vec::new();
                for v in t.clone().sequence_values::<Value>() {
                    keypoints.push(LColorSequenceKeypoint::from_lua(v?, lua)?.0);
                }
                ColorSequence::validate(&keypoints).map_err(rt)?;
                return Ok(LColorSequence(ColorSequence { keypoints }));
            }
            let a = args
                .front()
                .cloned()
                .map(|v| LColor3::from_lua(v, lua))
                .transpose()?
                .map(|c| c.0)
                .unwrap_or(Color3::WHITE);
            let b = args
                .get(1)
                .cloned()
                .map(|v| LColor3::from_lua(v, lua))
                .transpose()?
                .map(|c| c.0)
                .unwrap_or(a);
            Ok(LColorSequence(ColorSequence::range(a, b)))
        })?,
    )?;

    let pp = make("PhysicalProperties")?;
    pp.set(
        "new",
        lua.create_function(|lua, args: MultiValue| {
            if let Some(v) = args.front()
                && let Some(m) = enum_item_from(lua, v, "Material")
            {
                let (density, friction, elasticity, fw, ew) = crate::materials::material_physics(m);
                return Ok(LPhysicalProperties(PhysicalProperties::custom(
                    density, friction, elasticity, fw, ew,
                )));
            }
            let a = args_f32(&args, 5);
            let fw = if args.len() > 3 { a[3] } else { 1.0 };
            let ew = if args.len() > 4 { a[4] } else { 1.0 };
            Ok(LPhysicalProperties(PhysicalProperties::custom(
                a[0], a[1], a[2], fw, ew,
            )))
        })?,
    )?;

    g.set("Enum", LEnums)?;
    Ok(())
}
