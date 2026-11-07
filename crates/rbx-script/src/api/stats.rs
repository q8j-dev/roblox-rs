use super::{Registry, Ret, none, ret};
use crate::convert::arg_number;
use crate::runtime::{Engine, engine, with_state};
use crate::types::rt;
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

#[derive(Clone, Debug, PartialEq)]
pub enum StatSource {
    Group,
    Fps,
    Primitives,
    Joints,
    Contacts,
    LuaThreads,
    LuaThreadsThrottled,
    Instances,
    Net(String, NetStat),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetStat {
    Group,
    SentPhysics,
    SentData,
    ReceivedPhysics,
    ReceivedData,
    SentKbps,
    ReceivedKbps,
}

const NET_ITEMS: &[(&str, NetStat)] = &[
    ("Sent Physics Packets", NetStat::SentPhysics),
    ("Sent Data Packets", NetStat::SentData),
    ("Received Physics Packets", NetStat::ReceivedPhysics),
    ("Received Data Packets", NetStat::ReceivedData),
    ("Sent kBytes/sec", NetStat::SentKbps),
    ("Received kBytes/sec", NetStat::ReceivedKbps),
];

fn item(e: &mut Engine, parent: InstanceId, name: &str, source: StatSource) -> Option<InstanceId> {
    if let Some(existing) = e.dm.find_first_child(parent, name, false) {
        return Some(existing);
    }
    let id = e.dm.create_internal("StatsItem").ok()?;
    e.dm.set_name(id, name);
    let _ = e.dm.set_parent_ignore_lock(id, Some(parent));
    e.dm.lock_parent(id, true);
    e.stats_sources.insert(id, source);
    Some(id)
}

pub fn ensure_tree(e: &mut Engine) -> Option<InstanceId> {
    let stats = e.dm.get_service("Stats").ok()?;
    if let Some(ws) = item(e, stats, "Workspace", StatSource::Group) {
        item(e, ws, "FPS", StatSource::Fps);
        if let Some(world) = item(e, ws, "World", StatSource::Group) {
            item(e, world, "Primitives", StatSource::Primitives);
            item(e, world, "Joints", StatSource::Joints);
            item(e, world, "Contacts", StatSource::Contacts);
        }
    }
    if let Some(l) = item(e, stats, "Lua", StatSource::Group) {
        item(e, l, "Threads", StatSource::LuaThreads);
        item(e, l, "ThreadsThrottled", StatSource::LuaThreadsThrottled);
    }
    if let Some(dm) = item(e, stats, "DataModel", StatSource::Group) {
        item(e, dm, "Instances", StatSource::Instances);
    }
    if let Some(net) = item(e, stats, "Network", StatSource::Group) {
        item(e, net, "Packets Thread", StatSource::Group);
    }
    sync_network(e);
    Some(stats)
}

pub fn sync_network(e: &mut Engine) {
    let Some(stats) = e.dm.find_service("Stats") else {
        return;
    };
    let Some(net) = e.dm.find_first_child(stats, "Network", false) else {
        return;
    };
    let wanted: Vec<String> = e.net_stats.iter().map(|p| p.name.clone()).collect();
    for child in e.dm.children_of(net).to_vec() {
        if let Some(StatSource::Net(name, NetStat::Group)) = e.stats_sources.get(&child)
            && !wanted.contains(name)
        {
            for d in e.dm.descendants(child) {
                e.stats_sources.remove(&d);
            }
            e.stats_sources.remove(&child);
            e.dm.lock_parent(child, false);
            let _ = e.dm.destroy(child);
        }
    }
    for name in wanted {
        let Some(peer) = item(e, net, &name, StatSource::Net(name.clone(), NetStat::Group)) else {
            continue;
        };
        for (label, stat) in NET_ITEMS {
            if let Some(it) = item(e, peer, label, StatSource::Net(name.clone(), *stat))
                && matches!(stat, NetStat::SentPhysics | NetStat::SentData)
            {
                item(e, it, "Smoothed", StatSource::Net(name.clone(), *stat));
            }
        }
    }
}

fn value_of(lua: &Lua, id: InstanceId) -> f64 {
    let source = engine(lua).lock().stats_sources.get(&id).cloned();
    match source {
        None | Some(StatSource::Group) => 0.0,
        Some(StatSource::LuaThreads) => with_state(lua, |s| s.threads.len()) as f64,
        Some(StatSource::LuaThreadsThrottled) => 0.0,
        Some(other) => {
            let eng = engine(lua);
            let e = eng.lock();
            match other {
                StatSource::Fps => e.fps,
                StatSource::Instances => e.dm.len() as f64,
                StatSource::Primitives | StatSource::Joints | StatSource::Contacts => {
                    let w = e.world.lock();
                    match other {
                        StatSource::Primitives => w.parts.len() as f64,
                        StatSource::Joints => w.joints.len() as f64,
                        _ => w
                            .contacts
                            .values()
                            .filter(|c| !c.manifolds.is_empty())
                            .count() as f64,
                    }
                }
                StatSource::Net(name, stat) => e
                    .net_stats
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| match stat {
                        NetStat::Group => 0.0,
                        NetStat::SentPhysics => p.sent_physics,
                        NetStat::SentData => p.sent_data,
                        NetStat::ReceivedPhysics => p.received_physics,
                        NetStat::ReceivedData => p.received_data,
                        NetStat::SentKbps => p.sent_kbps,
                        NetStat::ReceivedKbps => p.received_kbps,
                    })
                    .unwrap_or(0.0),
                _ => 0.0,
            }
        }
    }
}

fn get_value(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, value_of(lua, id))
}

fn get_value_string(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let v = value_of(lua, id);
    let text = if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    };
    ret(lua, text)
}

pub fn job_rows(e: &Engine) -> Vec<(String, f64, f64, f64, f64, bool)> {
    let frame = if e.fps > 0.0 { 1.0 / e.fps } else { 0.0 };
    let mut rows = vec![
        ("Heartbeat".to_string(), 0.0, e.fps, frame, 0.0, true),
        (
            "Physics".to_string(),
            0.0,
            if e.running { e.fps } else { 0.0 },
            frame,
            0.0,
            e.running,
        ),
        ("Lua wait".to_string(), 0.0, e.fps, frame, 0.0, true),
    ];
    for p in &e.net_stats {
        let physics_rate = e.fps / crate::net_physics::SEND_INTERVAL as f64;
        rows.push((
            format!("Net SendPhysics {}", p.name),
            0.0,
            physics_rate,
            0.0,
            0.0,
            true,
        ));
        rows.push((
            format!("Net SendData {}", p.name),
            0.0,
            e.fps,
            0.0,
            0.0,
            true,
        ));
    }
    rows
}

fn jobs_info(lua: &Lua, _: InstanceId, _: &[Value]) -> Result<Ret> {
    let rows = job_rows(&engine(lua).lock());
    let out = lua.create_table()?;
    for (i, (name, duty, steps, step_time, error, running)) in rows.into_iter().enumerate() {
        let row = lua.create_table()?;
        row.raw_set(1, name)?;
        row.raw_set(2, duty)?;
        row.raw_set(3, steps)?;
        row.raw_set(4, step_time)?;
        row.raw_set(5, error)?;
        row.raw_set(6, running)?;
        out.raw_set(i + 1, row)?;
    }
    ret(lua, out)
}

fn set_window(lua: &Lua, _: InstanceId, args: &[Value]) -> Result<Ret> {
    let seconds = arg_number(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    engine(lua).lock().jobs_stats_window = seconds.max(0.0);
    none()
}

pub fn register(r: &mut Registry) {
    r.add("StatsItem", &["GetValue"], get_value);
    r.add("StatsItem", &["GetValueString"], get_value_string);
    r.add(
        "DataModel",
        &["GetJobsInfo", "GetJobsExtendedStats"],
        jobs_info,
    );
    r.add(
        "DataModel",
        &["SetJobsExtendedStatsWindow", "setJobsExtendedStatsWindow"],
        set_window,
    );
}
