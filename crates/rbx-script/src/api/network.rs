use super::{Registry, Ret, none, ret};
use crate::convert::{arg_number, arg_string};
use crate::runtime::{NetworkRequest, engine};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::{InstanceId, Variant};

fn port_arg(v: Option<&Value>, default: u16) -> Result<u16> {
    match arg_number(v) {
        None => Ok(default),
        Some(p) if (0.0..=65535.0).contains(&p) => Ok(p as u16),
        Some(p) => Err(rt(format!("{p} is not a valid port"))),
    }
}

fn server_start(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let port = port_arg(args.first(), 53640)?;
    let eng = engine(lua);
    let mut e = eng.lock();
    e.dm.set_unchecked(id, "Port", Variant::Int(port as i32));
    e.network_requests
        .push(NetworkRequest::StartServer { port });
    none()
}

fn server_stop(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    engine(lua)
        .lock()
        .network_requests
        .push(NetworkRequest::StopServer);
    none()
}

fn client_count(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let n = engine(lua).lock().client_count;
    ret(lua, n as f64)
}

fn player_connect(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let user_id = arg_number(args.first()).unwrap_or(0.0) as i64;
    let server = arg_string(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))?;
    let port = port_arg(args.get(2), 53640)?;
    let eng = engine(lua);
    let mut e = eng.lock();
    let ticket =
        e.dm.get(id, "Ticket")
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
    e.network_requests.push(NetworkRequest::Connect {
        user_id,
        server,
        port,
        ticket,
    });
    none()
}

fn client_disconnect(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    engine(lua)
        .lock()
        .network_requests
        .push(NetworkRequest::Disconnect);
    none()
}

pub fn register(r: &mut Registry) {
    r.add("NetworkServer", &["Start"], server_start);
    r.add("NetworkServer", &["Stop"], server_stop);
    r.add("NetworkServer", &["GetClientCount"], client_count);
    r.add("NetworkClient", &["PlayerConnect"], player_connect);
    r.add("NetworkClient", &["Disconnect"], client_disconnect);
}
