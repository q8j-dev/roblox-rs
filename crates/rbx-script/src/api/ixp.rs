use super::{Registry, Ret, none, ret};
use crate::convert::arg_string;
use crate::runtime::engine;
use crate::types::{LEnumItem, enum_item_value, rt};
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};

const INITIALIZED: u32 = 2;

pub fn register(r: &mut Registry) {
    r.add(
        "IXPService",
        &[
            "GetUserLayerLoadingStatus",
            "GetBrowserTrackerLayerLoadingStatus",
        ],
        loading_status,
    );
    r.add(
        "IXPService",
        &["GetUserStatusForLayer", "GetBrowserTrackerStatusForLayer"],
        layer_status,
    );
    r.add(
        "IXPService",
        &["GetUserLayerVariables", "GetBrowserTrackerLayerVariables"],
        layer_variables,
    );
    r.add("IXPService", &["RegisterUserLayers"], register_layers);
    r.add("IXPService", &["InitializeUserLayers"], initialize);
    r.add("IXPService", &["ClearUserLayers"], clear);
    r.add(
        "IXPService",
        &[
            "LogUserLayerExposure",
            "LogFlagLinkedUserLayerExposure",
            "LogBrowserTrackerLayerExposure",
        ],
        log_exposure,
    );
}

fn status(lua: &Lua) -> Result<Value> {
    match LEnumItem::lookup("IXPLoadingStatus", INITIALIZED) {
        Some(item) => enum_item_value(lua, item),
        None => Ok(Value::Number(INITIALIZED as f64)),
    }
}

fn loading_status(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, status(lua)?)
}

fn layer_status(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, status(lua)?)
}

fn layer_variables(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let layer = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let vars = engine(lua).lock().flags.ixp_layer(&layer);
    let text = serde_json::Value::Object(vars).to_string();
    ret(lua, crate::api::http::decode_json(lua, &text)?)
}

fn register_layers(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    if let Some(Value::Table(t)) = args.first() {
        let names: Vec<String> = t
            .clone()
            .sequence_values::<String>()
            .filter_map(|v| v.ok())
            .collect();
        let eng = engine(lua);
        let mut e = eng.lock();
        for n in names {
            e.flags.register_layer(&n);
        }
    }
    none()
}

fn initialize(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    engine(lua).lock().dm.fire_signal(
        id,
        "OnUserLayerLoadingStatusChanged",
        vec![Variant::Enum(INITIALIZED)],
    );
    none()
}

fn clear(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn log_exposure(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    if let Some(layer) = arg_string(args.first()) {
        engine(lua).lock().flags.register_layer(&layer);
    }
    none()
}
