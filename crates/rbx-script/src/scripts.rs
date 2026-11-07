use crate::runtime::{
    MessageType, ThreadInfo, engine, make_ready, new_environment, resume_thread, thread_key,
    with_state,
};
use crate::security::Identity;
use mlua::{Lua, MultiValue};
use rbx_dom::{DataModel, InstanceId, Variant};

fn is_local_script(dm: &DataModel, id: InstanceId) -> bool {
    dm.class_of(id) == Some("LocalScript")
}

fn local_character(dm: &DataModel, local_player: Option<InstanceId>) -> Option<InstanceId> {
    local_player.and_then(|p| dm.get_ref(p, "Character"))
}

pub fn should_run(
    dm: &DataModel,
    local_player: Option<InstanceId>,
    id: InstanceId,
) -> Option<Identity> {
    let class = dm.class_of(id)?;
    if !dm.is_a(id, "BaseScript") {
        return None;
    }
    if dm.get_bool(id, "Disabled") {
        return None;
    }
    if !dm.in_tree(id) {
        return None;
    }
    let backend = dm.find_service("NetworkClient").is_none();
    let frontend = dm.find_service("NetworkServer").is_none();
    let local = is_local_script(dm, id);
    let mut cur = dm.parent_of(id);
    while let Some(p) = cur {
        let pclass = dm.class_of(p).unwrap_or("");
        let verdict = match pclass {
            "ScriptContext" => Some(class == "CoreScript"),
            "Workspace" => Some(if local {
                local_character(dm, local_player)
                    .is_some_and(|c| c == id || dm.is_descendant_of(id, c))
            } else {
                backend
            }),
            "ServerScriptService" => Some(backend && class == "Script"),
            "Backpack" => {
                let is_local_backpack = local_player.is_some() && dm.parent_of(p) == local_player;
                if (is_local_backpack && local) || (!local && backend) {
                    Some(true)
                } else {
                    None
                }
            }
            "PlayerGui" => Some(if local {
                local_player.is_some() && dm.parent_of(p) == local_player
            } else {
                backend
            }),
            "PlayerScripts" => {
                Some(local && local_player.is_some() && dm.parent_of(p) == local_player)
            }
            "ReplicatedFirst" => Some(local && frontend),
            _ => None,
        };
        if verdict == Some(true) {
            return Some(match class {
                "CoreScript" => Identity::RobloxGameScript,
                _ => Identity::GameScript,
            });
        }
        cur = dm.parent_of(p);
    }
    None
}

pub fn start_script(lua: &Lua, id: InstanceId, identity: Identity) {
    let already = with_state(lua, |s| {
        s.running_scripts.get(&id).copied().unwrap_or(false)
    });
    if already {
        return;
    }
    let (source, name) = {
        let eng = engine(lua);
        let e = eng.lock();
        let src =
            e.dm.get(id, "Source")
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
        (src, e.dm.full_name(id))
    };
    with_state(lua, |s| {
        s.running_scripts.insert(id, true);
    });
    let env = match new_environment(lua, Some(id)) {
        Ok(e) => e,
        Err(e) => {
            crate::runtime::report_error(lua, &e);
            return;
        }
    };
    let func = match lua
        .load(&source)
        .set_name(format!("={name}"))
        .set_environment(env)
        .into_function()
    {
        Ok(f) => f,
        Err(e) => {
            let text = crate::runtime::format_error(&e);
            engine(lua).lock().print(MessageType::Error, text);
            return;
        }
    };
    let thread = match lua.create_thread(func) {
        Ok(t) => t,
        Err(e) => {
            crate::runtime::report_error(lua, &e);
            return;
        }
    };
    with_state(lua, |s| {
        s.threads.insert(
            thread_key(&thread),
            ThreadInfo {
                script: Some(id),
                identity,
            },
        );
        s.script_threads.entry(id).or_default().push(thread.clone());
    });
    resume_thread(lua, &thread, MultiValue::new());
}

pub fn stop_script(lua: &Lua, id: InstanceId) {
    let was = with_state(lua, |s| s.running_scripts.remove(&id).unwrap_or(false));
    if !was {
        return;
    }
    with_state(lua, |s| {
        let conn_ids: Vec<u64> = s
            .conns
            .iter()
            .filter(|(_, c)| c.script == Some(id))
            .map(|(k, _)| *k)
            .collect();
        for cid in conn_ids {
            if let Some(c) = s.conns.remove(&cid)
                && let Some(list) = s.by_event.get_mut(&(c.target, c.event))
            {
                list.retain(|x| *x != cid);
            }
        }
        if let Some(threads) = s.script_threads.remove(&id) {
            for t in threads {
                s.threads.remove(&thread_key(&t));
                s.sleepers
                    .retain(|sl| thread_key(&sl.thread) != thread_key(&t));
                s.ready.retain(|(r, _)| thread_key(r) != thread_key(&t));
                for waiters in s.waiters.values_mut() {
                    waiters.retain(|w| thread_key(w) != thread_key(&t));
                }
                s.child_waiters
                    .retain(|w| thread_key(&w.thread) != thread_key(&t));
            }
        }
    });
}

pub fn reevaluate(lua: &Lua, id: InstanceId) {
    let verdict = {
        let eng = engine(lua);
        let e = eng.lock();
        if !e.dm.contains(id) || !e.dm.is_a(id, "BaseScript") {
            return;
        }
        should_run(&e.dm, e.local_player, id)
    };
    let running = with_state(lua, |s| {
        s.running_scripts.get(&id).copied().unwrap_or(false)
    });
    match (verdict, running) {
        (Some(identity), false) => start_script(lua, id, identity),
        (None, true) => stop_script(lua, id),
        _ => {}
    }
}

pub fn scan_all(lua: &Lua) {
    let scripts: Vec<InstanceId> = {
        let eng = engine(lua);
        let e = eng.lock();
        let root = e.dm.root();
        e.dm.descendants(root)
            .into_iter()
            .filter(|&d| e.dm.is_a(d, "BaseScript"))
            .collect()
    };
    for s in scripts {
        reevaluate(lua, s);
    }
}

pub fn on_ancestry_changed(lua: &Lua, id: InstanceId) {
    reevaluate(lua, id);
}

pub fn on_property_changed(lua: &Lua, id: InstanceId, property: &str) {
    if property == "Disabled" {
        let is_script = engine(lua).lock().dm.is_a(id, "BaseScript");
        if is_script {
            stop_script(lua, id);
            reevaluate(lua, id);
        }
    }
    if property == "Character" {
        let local = engine(lua).lock().local_player;
        if local == Some(id) {
            scan_all(lua);
        }
    }
}

pub fn on_destroyed(lua: &Lua, id: InstanceId) {
    stop_script(lua, id);
}

fn resolve_child_waiters(lua: &Lua, parent: InstanceId, child: InstanceId) {
    let name = engine(lua).lock().dm.name_of(child).to_string();
    let ready = with_state(lua, |s| {
        let mut ready = Vec::new();
        let mut i = 0;
        while i < s.child_waiters.len() {
            if s.child_waiters[i].parent == parent && s.child_waiters[i].name == name {
                ready.push(s.child_waiters.swap_remove(i).thread);
            } else {
                i += 1;
            }
        }
        ready
    });
    for t in ready {
        make_ready(lua, t, vec![Variant::Ref(Some(child))]);
    }
}

pub fn on_child_added(lua: &Lua, parent: InstanceId, child: InstanceId) {
    resolve_child_waiters(lua, parent, child);
}

pub fn on_name_changed(lua: &Lua, id: InstanceId) {
    let parent = engine(lua).lock().dm.parent_of(id);
    if let Some(p) = parent {
        resolve_child_waiters(lua, p, id);
    }
}
