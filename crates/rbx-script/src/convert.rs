use crate::instance::push_instance;
use crate::runtime::instance_from_value;
use crate::types::*;
use mlua::{FromLua, Lua, Result, Value};
use rbx_dom::{InstanceId, Variant, VariantType, database};
use rbx_math::BrickColor;
use rbx_reflection::ValueType;

pub fn variant_to_lua(lua: &Lua, v: &Variant, ty: Option<&ValueType>) -> Result<Value> {
    Ok(match v {
        Variant::Nil => Value::Nil,
        Variant::Bool(b) => Value::Boolean(*b),
        Variant::Int(i) => Value::Number(*i as f64),
        Variant::Int64(i) => Value::Number(*i as f64),
        Variant::Float(f) => Value::Number(*f as f64),
        Variant::Double(f) => Value::Number(*f),
        Variant::String(s)
        | Variant::ProtectedString(s)
        | Variant::Content(s)
        | Variant::SystemAddress(s) => Value::String(lua.create_string(s)?),
        Variant::BinaryString(b) => Value::String(lua.create_string(b)?),
        Variant::Vector2(x) => Value::UserData(lua.create_userdata(LVector2(*x))?),
        Variant::Vector3(x) => Value::UserData(lua.create_userdata(LVector3(*x))?),
        Variant::Vector2int16(x) => Value::UserData(lua.create_userdata(LVector2int16(*x))?),
        Variant::Vector3int16(x) => Value::UserData(lua.create_userdata(LVector3int16(*x))?),
        Variant::Vector3int32(x) => Value::UserData(lua.create_userdata(LVector3int16(
            rbx_math::Vector3int16::new(x.x as i16, x.y as i16, x.z as i16),
        ))?),
        Variant::CFrame(x) => Value::UserData(lua.create_userdata(LCFrame(*x))?),
        Variant::Color3(x) => Value::UserData(lua.create_userdata(LColor3(*x))?),
        Variant::BrickColor(x) => Value::UserData(lua.create_userdata(LBrickColor(*x))?),
        Variant::UDim(x) => Value::UserData(lua.create_userdata(LUDim(*x))?),
        Variant::UDim2(x) => Value::UserData(lua.create_userdata(LUDim2(*x))?),
        Variant::Ray(x) => Value::UserData(lua.create_userdata(LRay(*x))?),
        Variant::Rect2D(x) => Value::UserData(lua.create_userdata(LRect2D(*x))?),
        Variant::Region3(x) => Value::UserData(lua.create_userdata(LRegion3(*x))?),
        Variant::Region3int16(x) => Value::UserData(lua.create_userdata(LRegion3int16(*x))?),
        Variant::Faces(x) => Value::UserData(lua.create_userdata(LFaces(*x))?),
        Variant::Axes(x) => Value::UserData(lua.create_userdata(LAxes(*x))?),
        Variant::PhysicalProperties(x) => {
            Value::UserData(lua.create_userdata(LPhysicalProperties(*x))?)
        }
        Variant::NumberSequence(x) => {
            Value::UserData(lua.create_userdata(LNumberSequence(x.clone()))?)
        }
        Variant::ColorSequence(x) => {
            Value::UserData(lua.create_userdata(LColorSequence(x.clone()))?)
        }
        Variant::NumberRange(x) => Value::UserData(lua.create_userdata(LNumberRange(*x))?),
        Variant::Enum(e) => match ty {
            Some(ValueType::Enum(name)) => match LEnumItem::lookup(name, *e) {
                Some(item) => Value::UserData(lua.create_userdata(item)?),
                None => Value::Number(*e as f64),
            },
            _ => Value::Number(*e as f64),
        },
        Variant::Ref(r) => match r {
            Some(id) => push_instance(lua, *id)?,
            None => Value::Nil,
        },
    })
}

fn type_label(ty: &ValueType) -> String {
    match ty {
        ValueType::Value(t) => match t {
            VariantType::Bool => "bool".into(),
            VariantType::Int | VariantType::Int64 => "int".into(),
            VariantType::Float => "float".into(),
            VariantType::Double => "double".into(),
            VariantType::String | VariantType::ProtectedString | VariantType::BinaryString => {
                "string".into()
            }
            VariantType::Content => "Content".into(),
            VariantType::CFrame => "CoordinateFrame".into(),
            other => format!("{other:?}"),
        },
        ValueType::Enum(n) => n.clone(),
        ValueType::Ref(c) => c.clone(),
        other => format!("{other:?}"),
    }
}

pub fn lua_type_name(v: &Value) -> String {
    match v {
        Value::UserData(ud) => {
            if ud.borrow::<crate::instance::LInstance>().is_ok() {
                return "Object".into();
            }
            ud.type_name()
                .ok()
                .map(|s| s.to_string_lossy())
                .unwrap_or_else(|| "userdata".into())
        }
        other => other.type_name().to_string(),
    }
}

fn bad(ty: &ValueType, v: &Value) -> mlua::Error {
    rt(format!(
        "{} expected, got {}",
        type_label(ty),
        lua_type_name(v)
    ))
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Integer(i) => Some(*i as f64),
        Value::String(s) => s.to_str().ok().and_then(|s| s.trim().parse::<f64>().ok()),
        _ => None,
    }
}

fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(String::from_utf8_lossy(&s.as_bytes()).into_owned()),
        Value::Number(n) => Some(crate::format::lua_number_str(*n)),
        Value::Integer(i) => Some(i.to_string()),
        _ => None,
    }
}

pub fn lua_to_variant(lua: &Lua, v: Value, ty: &ValueType) -> Result<Variant> {
    Ok(match ty {
        ValueType::Value(t) => match t {
            VariantType::Bool => Variant::Bool(!matches!(v, Value::Nil | Value::Boolean(false))),
            VariantType::Int => Variant::Int(number(&v).ok_or_else(|| bad(ty, &v))? as i32),
            VariantType::Int64 => Variant::Int64(number(&v).ok_or_else(|| bad(ty, &v))? as i64),
            VariantType::Float => Variant::Float(number(&v).ok_or_else(|| bad(ty, &v))? as f32),
            VariantType::Double => Variant::Double(number(&v).ok_or_else(|| bad(ty, &v))?),
            VariantType::String => Variant::String(string_of(&v).ok_or_else(|| bad(ty, &v))?),
            VariantType::ProtectedString => {
                Variant::ProtectedString(string_of(&v).ok_or_else(|| bad(ty, &v))?)
            }
            VariantType::Content => Variant::Content(string_of(&v).ok_or_else(|| bad(ty, &v))?),
            VariantType::BinaryString => match &v {
                Value::String(s) => Variant::BinaryString(s.as_bytes().to_vec()),
                _ => return Err(bad(ty, &v)),
            },
            VariantType::SystemAddress => Variant::SystemAddress(string_of(&v).unwrap_or_default()),
            VariantType::Vector3 => Variant::Vector3(
                LVector3::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Vector2 => Variant::Vector2(
                LVector2::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Vector3int16 => Variant::Vector3int16(
                LVector3int16::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Vector2int16 => Variant::Vector2int16(
                LVector2int16::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::CFrame => Variant::CFrame(
                LCFrame::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Color3 => Variant::Color3(
                LColor3::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::BrickColor => match LBrickColor::from_lua(v.clone(), lua) {
                Ok(b) => Variant::BrickColor(b.0),
                Err(_) => match number(&v) {
                    Some(n) => Variant::BrickColor(BrickColor::new(n as i64)),
                    None => return Err(bad(ty, &v)),
                },
            },
            VariantType::UDim => {
                Variant::UDim(LUDim::from_lua(v.clone(), lua).map_err(|_| bad(ty, &v))?.0)
            }
            VariantType::UDim2 => {
                Variant::UDim2(LUDim2::from_lua(v.clone(), lua).map_err(|_| bad(ty, &v))?.0)
            }
            VariantType::Ray => {
                Variant::Ray(LRay::from_lua(v.clone(), lua).map_err(|_| bad(ty, &v))?.0)
            }
            VariantType::Rect2D => Variant::Rect2D(
                LRect2D::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Region3 => Variant::Region3(
                LRegion3::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Region3int16 => Variant::Region3int16(
                LRegion3int16::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0,
            ),
            VariantType::Faces => {
                Variant::Faces(LFaces::from_lua(v.clone(), lua).map_err(|_| bad(ty, &v))?.0)
            }
            VariantType::Axes => {
                Variant::Axes(LAxes::from_lua(v.clone(), lua).map_err(|_| bad(ty, &v))?.0)
            }
            VariantType::PhysicalProperties => match &v {
                Value::Nil => Variant::PhysicalProperties(rbx_math::PhysicalProperties::default()),
                _ => Variant::PhysicalProperties(
                    LPhysicalProperties::from_lua(v.clone(), lua)
                        .map_err(|_| bad(ty, &v))?
                        .0,
                ),
            },
            VariantType::NumberSequence => match number(&v) {
                Some(n) => Variant::NumberSequence(rbx_math::NumberSequence::constant(n as f32)),
                None => Variant::NumberSequence(
                    LNumberSequence::from_lua(v.clone(), lua)
                        .map_err(|_| bad(ty, &v))?
                        .0,
                ),
            },
            VariantType::ColorSequence => match LColor3::from_lua(v.clone(), lua) {
                Ok(c) => Variant::ColorSequence(rbx_math::ColorSequence::constant(c.0)),
                Err(_) => Variant::ColorSequence(
                    LColorSequence::from_lua(v.clone(), lua)
                        .map_err(|_| bad(ty, &v))?
                        .0,
                ),
            },
            VariantType::NumberRange => match number(&v) {
                Some(n) => Variant::NumberRange(rbx_math::NumberRange::new(n as f32, n as f32)),
                None => Variant::NumberRange(
                    LNumberRange::from_lua(v.clone(), lua)
                        .map_err(|_| bad(ty, &v))?
                        .0,
                ),
            },
            VariantType::Vector3int32 => {
                let x = LVector3int16::from_lua(v.clone(), lua)
                    .map_err(|_| bad(ty, &v))?
                    .0;
                Variant::Vector3int32(rbx_math::Vector3int32::new(
                    x.x as i32, x.y as i32, x.z as i32,
                ))
            }
            VariantType::Nil => Variant::Nil,
            VariantType::Enum => Variant::Enum(number(&v).unwrap_or(0.0) as u32),
            VariantType::Ref => Variant::Ref(instance_from_value(lua, &v)),
        },
        ValueType::Enum(name) => Variant::Enum(enum_value(lua, &v, name)?),
        ValueType::Ref(class) => match &v {
            Value::Nil => Variant::Ref(None),
            _ => {
                let id = instance_from_value(lua, &v).ok_or_else(|| bad(ty, &v))?;
                let ok = {
                    let eng = crate::runtime::engine(lua);
                    let e = eng.lock();
                    class == "Instance" || e.dm.is_a(id, class) || database().class(class).is_none()
                };
                if !ok {
                    return Err(bad(ty, &v));
                }
                Variant::Ref(Some(id))
            }
        },
        _ => return Err(rt("unsupported property type")),
    })
}

pub fn enum_value(lua: &Lua, v: &Value, name: &str) -> Result<u32> {
    let desc = database()
        .enum_desc(name)
        .ok_or_else(|| rt(format!("unknown enum {name}")))?;
    if let Ok(item) = LEnumItem::from_lua(v.clone(), lua) {
        if item.enum_name != desc.name {
            return Err(rt(format!(
                "Invalid value for enum {name}: {}",
                format_args!("Enum.{}.{}", item.enum_name, item.name)
            )));
        }
        return Ok(item.value);
    }
    if let Some(n) = number(v) {
        let n = n as u32;
        if desc.is_valid(n) {
            return Ok(n);
        }
        return Err(rt(format!("Invalid value for enum {name}")));
    }
    if let Value::String(s) = v {
        let s = s.to_str()?;
        if let Some(val) = desc.from_name(&s) {
            return Ok(val);
        }
        return Err(rt(format!("Invalid value for enum {name}")));
    }
    Err(rt(format!("Invalid value for enum {name}")))
}

pub fn arg_string(v: Option<&Value>) -> Option<String> {
    v.and_then(string_of)
}

pub fn arg_number(v: Option<&Value>) -> Option<f64> {
    v.and_then(number)
}

pub fn arg_bool(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Nil) | Some(Value::Boolean(false)))
}

pub fn arg_instance(lua: &Lua, v: Option<&Value>) -> Option<InstanceId> {
    v.and_then(|v| instance_from_value(lua, v))
}
