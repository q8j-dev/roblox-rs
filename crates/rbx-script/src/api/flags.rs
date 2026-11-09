use super::{Registry, Ret, ret};
use crate::convert::arg_string;
use crate::flags::{FlagKind, FlagValue};
use crate::runtime::engine;
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

pub fn register(r: &mut Registry) {
    r.add("DataModel", &["DefineFastFlag"], define_flag);
    r.add("DataModel", &["DefineFastInt"], define_int);
    r.add("DataModel", &["DefineFastString"], define_string);
    r.add("DataModel", &["GetFastFlag"], get_flag);
    r.add("DataModel", &["GetFastInt"], get_int);
    r.add("DataModel", &["GetFastString"], get_string);
    r.add("DataModel", &["SetFastFlagForTesting"], set_flag);
    r.add("DataModel", &["SetFastIntForTesting"], set_int);
    r.add("DataModel", &["GetEngineFeature"], engine_feature);
}

fn name_arg(args: &[Value]) -> Result<String> {
    arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))
}

fn value_arg(kind: FlagKind, v: Option<&Value>) -> FlagValue {
    match kind {
        FlagKind::Flag => FlagValue::Bool(matches!(v, Some(Value::Boolean(true)))),
        FlagKind::Int => FlagValue::Int(crate::convert::arg_number(v).unwrap_or(0.0) as i64),
        FlagKind::String => FlagValue::String(arg_string(v).unwrap_or_default()),
    }
}

fn push(lua: &Lua, v: FlagValue) -> Result<Ret> {
    match v {
        FlagValue::Bool(b) => ret(lua, b),
        FlagValue::Int(i) => ret(lua, i as f64),
        FlagValue::String(s) => ret(lua, s),
    }
}

fn define(lua: &Lua, kind: FlagKind, args: &[Value]) -> Result<Ret> {
    let name = name_arg(args)?;
    let default = value_arg(kind, args.get(1));
    let v = engine(lua).lock().flags.define(kind, &name, default);
    push(lua, v)
}

fn get(lua: &Lua, kind: FlagKind, args: &[Value]) -> Result<Ret> {
    let name = name_arg(args)?;
    let v = engine(lua).lock().flags.get(kind, &name);
    push(lua, v)
}

fn set(lua: &Lua, kind: FlagKind, args: &[Value]) -> Result<Ret> {
    let name = name_arg(args)?;
    let value = value_arg(kind, args.get(1));
    let v = engine(lua).lock().flags.set(kind, &name, value);
    push(lua, v)
}

fn define_flag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    define(lua, FlagKind::Flag, args)
}

fn define_int(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    define(lua, FlagKind::Int, args)
}

fn define_string(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    define(lua, FlagKind::String, args)
}

fn get_flag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    get(lua, FlagKind::Flag, args)
}

fn get_int(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    get(lua, FlagKind::Int, args)
}

fn get_string(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    get(lua, FlagKind::String, args)
}

fn set_flag(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    set(lua, FlagKind::Flag, args)
}

fn set_int(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    set(lua, FlagKind::Int, args)
}

fn engine_feature(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let name = name_arg(args)?;
    let on = engine(lua).lock().flags.engine_feature(&name);
    ret(lua, on)
}
