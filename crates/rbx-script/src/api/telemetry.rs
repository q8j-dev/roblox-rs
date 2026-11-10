use super::{Registry, Ret, none, ret};
use crate::convert::arg_string;
use crate::runtime::{engine, with_state};
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

#[derive(Default)]
pub struct TelemetryState {
    session: Option<String>,
    registered_urls: Vec<String>,
}

pub fn register(r: &mut Registry) {
    for class in ["RbxAnalyticsService", "EventIngestService"] {
        r.add(
            class,
            &[
                "AddGlobalPointsField",
                "ReleaseRBXEventStream",
                "ReportCounter",
                "ReportInfluxSeries",
                "ReportStats",
                "ReportToDiagByCountryCode",
                "SendEventDeferred",
                "SendEventImmediately",
                "SetRBXEvent",
                "SetRBXEventStream",
                "TrackEvent",
                "UpdateHeartbeatObject",
            ],
            drop_event,
        );
    }
    r.add(
        "RbxAnalyticsService",
        &["GetPlaySessionId", "GetSessionId", "GetClientId"],
        session_id,
    );
    r.add(
        "TelemetryService",
        &["LogCounter", "LogEvent", "LogStat"],
        drop_event,
    );
    r.add("LinkingService", &["RegisterLuaUrl"], register_url);
    r.add("LinkingService", &["IsUrlRegistered"], is_url_registered);
    r.add(
        "LinkingService",
        &[
            "GetLastLuaUrl",
            "StartLuaUrlDelivery",
            "GetAndClearLastPendingUrl",
        ],
        no_pending_url,
    );
    r.add(
        "LinkingService",
        &["StopLuaUrlDelivery", "DetectUrl", "DetectDlic"],
        drop_event,
    );
    r.add(
        "LinkingService",
        &["SupportsSwitchToSettingsApp"],
        unsupported,
    );
}

fn drop_event(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn unsupported(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn session_id(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let existing = with_state(lua, |s| s.telemetry.session.clone());
    let id = match existing {
        Some(id) => id,
        None => {
            let seed = engine(lua).lock().next_random();
            let id = format!(
                "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
                (seed >> 32) as u32,
                (seed >> 16) as u16,
                (seed >> 4) as u16 & 0xfff,
                seed as u16 & 0xfff,
                seed.rotate_left(17) & 0xffff_ffff_ffff
            );
            with_state(lua, |s| s.telemetry.session = Some(id.clone()));
            id
        }
    };
    ret(lua, id)
}

fn register_url(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    if let Some(url) = arg_string(args.first()) {
        with_state(lua, |s| s.telemetry.registered_urls.push(url));
    }
    none()
}

fn is_url_registered(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let url = arg_string(args.first()).unwrap_or_default();
    let found = with_state(lua, |s| s.telemetry.registered_urls.contains(&url));
    ret(lua, found)
}

fn no_pending_url(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}
