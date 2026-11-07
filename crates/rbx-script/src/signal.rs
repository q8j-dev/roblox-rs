use crate::runtime::{current_info, with_state, yield_sentinel};
use crate::types::{not_member, rt};
use mlua::{Function, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::InstanceId;

#[derive(Clone, Copy, Debug)]
pub struct LSignal {
    pub target: InstanceId,
    pub event: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct LConnection {
    pub id: u64,
}

pub fn connect(
    lua: &Lua,
    target: InstanceId,
    event: &'static str,
    func: Function,
) -> Result<LConnection> {
    let info = current_info(lua);
    let since = crate::runtime::engine(lua).lock().dm.event_position();
    let id = with_state(lua, |s| {
        let id = s.next_conn;
        s.next_conn += 1;
        s.conns.insert(
            id,
            crate::runtime::Conn {
                target,
                event,
                func,
                script: info.script,
                identity: info.identity,
                since,
            },
        );
        s.by_event.entry((target, event)).or_default().push(id);
        id
    });
    Ok(LConnection { id })
}

pub fn disconnect(lua: &Lua, id: u64) {
    with_state(lua, |s| {
        if let Some(c) = s.conns.remove(&id)
            && let Some(list) = s.by_event.get_mut(&(c.target, c.event))
        {
            list.retain(|x| *x != id);
        }
    });
}

pub fn register(lua: &Lua) -> Result<()> {
    let native = lua.create_function(|lua, sig: mlua::AnyUserData| {
        let sig = *sig.borrow::<LSignal>()?;
        let t = lua.current_thread();
        let since = crate::runtime::engine(lua).lock().dm.event_position();
        with_state(lua, |s| {
            s.waiters
                .entry((sig.target, sig.event))
                .or_default()
                .push((t, since))
        });
        yield_sentinel(lua)
    })?;
    let wrap: Function = lua.named_registry_value("rbx_wrap")?;
    let wait: Function = wrap.call(native)?;
    lua.set_named_registry_value("rbx_signal_wait", wait)?;
    Ok(())
}

impl UserData for LSignal {
    fn add_fields<F: mlua::UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(mlua::MetaMethod::Type, "RBXScriptSignal");
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        for name in ["connect", "Connect", "connectFirst", "connectLast"] {
            m.add_method(name, |lua, this, f: Value| {
                let Value::Function(f) = f else {
                    return Err(rt(
                        "Attempt to connect failed: Passed value is not a function",
                    ));
                };
                connect(lua, this.target, this.event, f)
            });
        }
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Signal {}", this.event))
        });
        m.add_meta_method(MetaMethod::Index, |lua, _, k: String| -> Result<Value> {
            match k.as_str() {
                "wait" | "Wait" => lua.named_registry_value("rbx_signal_wait"),
                _ => Err(not_member(&k)),
            }
        });
    }
}

impl UserData for LConnection {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(mlua::MetaMethod::Type, "RBXScriptConnection");
        for name in ["connected", "Connected"] {
            f.add_field_method_get(name, |lua, this| {
                Ok(with_state(lua, |s| s.conns.contains_key(&this.id)))
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        for name in ["disconnect", "Disconnect"] {
            m.add_method(name, |lua, this, ()| {
                disconnect(lua, this.id);
                Ok(())
            });
        }
        m.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Connection"));
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
    }
}
