use super::{Registry, Ret, none, ret};
use crate::convert::{arg_number, arg_string};
use crate::instance::push_instance;
use crate::runtime::{
    ThreadInfo, current_info, engine, fire_lua, spawn_function_result, with_state,
};
use crate::types::rt;
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{InstanceId, Variant};
use std::collections::HashMap;

#[derive(Default)]
pub struct TextChatState {
    pub timestamps: HashMap<InstanceId, i64>,
    synced_players: usize,
}

const NAME_COLORS: [[u8; 3]; 8] = [
    [253, 41, 67],
    [1, 162, 255],
    [2, 184, 87],
    [107, 50, 124],
    [218, 133, 65],
    [245, 205, 48],
    [232, 186, 200],
    [215, 197, 154],
];

const COMMANDS: [(&str, &str, &str); 9] = [
    ("RBXClearCommand", "/clear", "/cls"),
    ("RBXConsoleCommand", "/console", ""),
    ("RBXUnmuteCommand", "/unmute", "/um"),
    ("RBXEmoteCommand", "/emote", "/e"),
    ("RBXWhisperCommand", "/whisper", "/w"),
    ("RBXMuteCommand", "/mute", "/m"),
    ("RBXHelpCommand", "/help", "/?"),
    ("RBXVersionCommand", "/version", "/v"),
    ("RBXTeamCommand", "/team", "/t"),
];

pub fn name_color(name: &str) -> [u8; 3] {
    let len = name.len() as i64;
    let mut value: i64 = 0;
    for (i, b) in name.bytes().enumerate() {
        let mut c = b as i64;
        let mut reverse = len - i as i64;
        if len % 2 == 1 {
            reverse -= 1;
        }
        if reverse % 4 >= 2 {
            c = -c;
        }
        value += c;
    }
    NAME_COLORS[value.rem_euclid(8) as usize]
}

pub fn register(r: &mut Registry) {
    r.add(
        "TextChannel",
        &["SendAsync", "SendInternalAsync"],
        send_async,
    );
    r.add("TextChannel", &["DisplaySystemMessage"], display_system);
    r.add("TextChannel", &["AddUserAsync"], add_user);
    r.add("TextChannel", &["SetDirectChatRequester"], set_requester);
    r.add(
        "TextChatService",
        &[
            "CanUserChatAsync",
            "CanUsersChatAsync",
            "CanUsersWhisperAsync",
        ],
        yes,
    );
    r.add("TextChatService", &["CanUsersDirectChatAsync"], direct_chat);
    r.add(
        "TextChatService",
        &["GetChatableUserCountAsync"],
        chatable_count,
    );
    r.add(
        "TextChatService",
        &[
            "GetChatGroupsAsync",
            "GetPresetsAsync",
            "GetTextChannelWindows",
        ],
        empty,
    );
    r.add(
        "TextChatService",
        &[
            "ChatIsLoaded",
            "SendExpChatMessageClientRendered",
            "SendExpChatWindowScroll",
            "SendExpChatWindowStatusChange",
            "SendTextChatCommandClientSent",
            "SendEnableChatButtonClicked",
            "SendEnableChatButtonShown",
            "OnUserChatSettingUpdateAsync",
        ],
        nothing,
    );
    r.add("TextChatService", &["DisplayBubble"], display_bubble);
    r.add("Chat", &["CanUserChatAsync", "CanUsersChatAsync"], yes);
}

fn nothing(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, Value::Nil)
}

fn yes(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, true)
}

fn empty(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn direct_chat(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    match args.get(1) {
        Some(Value::Table(t)) => ret(lua, t.clone()),
        _ => ret(lua, lua.create_table()?),
    }
}

fn chatable_count(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let count = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.find_service("Players")
            .map(|p| {
                e.dm.children_of(p)
                    .iter()
                    .filter(|c| e.dm.class_of(**c) == Some("Player"))
                    .count()
            })
            .unwrap_or(0)
    };
    ret(lua, count as i64)
}

fn set_requester(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    if let Some(p) = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v))
    {
        let _ = engine(lua)
            .lock()
            .dm
            .set(id, "DirectChatRequester", Variant::Ref(Some(p)));
    }
    none()
}

fn create_child(
    dm: &mut rbx_dom::DataModel,
    parent: InstanceId,
    class: &str,
    name: &str,
) -> Option<InstanceId> {
    if let Some(found) = dm.find_first_child(parent, name, false) {
        return Some(found);
    }
    let id = dm.create_internal(class).ok()?;
    dm.set_name(id, name);
    let _ = dm.set_parent_ignore_lock(id, Some(parent));
    Some(id)
}

pub fn ensure(lua: &Lua) {
    let eng = engine(lua);
    let mut e = eng.lock();
    let Ok(tcs) = e.dm.get_service("TextChatService") else {
        return;
    };
    if !matches!(e.dm.get(tcs, "ChatVersion"), Ok(Variant::Enum(1))) {
        return;
    }
    for class in [
        "ChannelTabsConfiguration",
        "ChatInputBarConfiguration",
        "ChatWindowConfiguration",
        "BubbleChatConfiguration",
    ] {
        create_child(&mut e.dm, tcs, class, class);
    }
    let general = if e.dm.get_bool(tcs, "CreateDefaultTextChannels") {
        let folder = create_child(&mut e.dm, tcs, "Folder", "TextChannels");
        let mut general = None;
        if let Some(folder) = folder {
            for name in ["RBXGeneral", "RBXSystem"] {
                if let Some(ch) = create_child(&mut e.dm, folder, "TextChannel", name) {
                    let _ = e.dm.set(ch, "IsDefaultTextChannel", Variant::Bool(true));
                    let _ =
                        e.dm.set(ch, "AddPlayersOnJoin", Variant::Bool(name == "RBXGeneral"));
                    if name == "RBXGeneral" {
                        general = Some(ch);
                    }
                }
            }
        }
        general
    } else {
        None
    };
    if let Some(bar) =
        e.dm.find_first_child(tcs, "ChatInputBarConfiguration", false)
        && let Some(general) = general
    {
        let _ =
            e.dm.set(bar, "TargetTextChannel", Variant::Ref(Some(general)));
    }
    if e.dm.get_bool(tcs, "CreateDefaultCommands")
        && let Some(folder) = create_child(&mut e.dm, tcs, "Folder", "TextChatCommands")
    {
        for (name, primary, secondary) in COMMANDS {
            if let Some(cmd) = create_child(&mut e.dm, folder, "TextChatCommand", name) {
                let _ =
                    e.dm.set(cmd, "PrimaryAlias", Variant::String(primary.into()));
                let _ =
                    e.dm.set(cmd, "SecondaryAlias", Variant::String(secondary.into()));
            }
        }
    }
}

fn ensure_source(
    dm: &mut rbx_dom::DataModel,
    channel: InstanceId,
    player: InstanceId,
) -> Option<InstanceId> {
    let user_id = dm.get(player, "UserId").ok().and_then(|v| v.as_f64())? as i64;
    let existing = dm.children_of(channel).iter().copied().find(|c| {
        dm.class_of(*c) == Some("TextSource")
            && dm.get(*c, "UserId").ok().and_then(|v| v.as_f64()) == Some(user_id as f64)
    });
    if existing.is_some() {
        return existing;
    }
    let username = dm.name_of(player).to_string();
    let display = dm
        .get(player, "DisplayName")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| username.clone());
    let src = dm.create_internal("TextSource").ok()?;
    dm.set_name(src, &username);
    dm.set_unchecked(src, "UserId", Variant::Int64(user_id));
    dm.set_unchecked(src, "Username", Variant::String(username));
    dm.set_unchecked(src, "DisplayName", Variant::String(display));
    dm.set_unchecked(src, "CanSend", Variant::Bool(true));
    let _ = dm.set_parent_ignore_lock(src, Some(channel));
    Some(src)
}

pub fn sync(lua: &Lua) {
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(players) = e.dm.find_service("Players") else {
        return;
    };
    let list: Vec<InstanceId> =
        e.dm.children_of(players)
            .iter()
            .copied()
            .filter(|c| e.dm.class_of(*c) == Some("Player"))
            .collect();
    if with_state(lua, |s| s.text_chat.synced_players) == list.len() {
        return;
    }
    let Some(tcs) = e.dm.find_service("TextChatService") else {
        return;
    };
    let channels: Vec<InstanceId> =
        e.dm.descendants(tcs)
            .into_iter()
            .filter(|c| {
                e.dm.class_of(*c) == Some("TextChannel") && e.dm.get_bool(*c, "AddPlayersOnJoin")
            })
            .collect();
    if channels.is_empty() {
        return;
    }
    for ch in channels {
        for &p in &list {
            ensure_source(&mut e.dm, ch, p);
        }
    }
    with_state(lua, |s| s.text_chat.synced_players = list.len());
}

fn add_user(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let uid = arg_number(args.first()).unwrap_or(0.0) as i64;
    let src = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let player = e.dm.find_service("Players").and_then(|p| {
            e.dm.children_of(p).iter().copied().find(|c| {
                e.dm.class_of(*c) == Some("Player")
                    && e.dm.get(*c, "UserId").ok().and_then(|v| v.as_f64()) == Some(uid as f64)
            })
        });
        player.and_then(|p| ensure_source(&mut e.dm, id, p))
    };
    let v = match src {
        Some(s) => push_instance(lua, s)?,
        None => Value::Nil,
    };
    Ok(Ret::Values(MultiValue::from_vec(vec![
        v,
        Value::Boolean(src.is_some()),
    ])))
}

fn now_millis() -> i64 {
    rbx_base::time::SystemTime::now()
        .duration_since(rbx_base::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_message(
    lua: &Lua,
    channel: InstanceId,
    source: Option<InstanceId>,
    text: &str,
    metadata: &str,
) -> Result<InstanceId> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let msg =
        e.dm.create_internal("TextChatMessage")
            .map_err(|err| rt(err.to_string()))?;
    let _ = e.dm.set(msg, "Text", Variant::String(text.into()));
    let _ = e.dm.set(msg, "Metadata", Variant::String(metadata.into()));
    let _ = e.dm.set(msg, "TextChannel", Variant::Ref(Some(channel)));
    let _ = e.dm.set(msg, "TextSource", Variant::Ref(source));
    let user_id = source
        .and_then(|s| e.dm.get(s, "UserId").ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i64;
    let guid = uuid_like();
    let _ = e.dm.set(
        msg,
        "MessageId",
        Variant::String(format!("{user_id}-{guid}")),
    );
    if let Some(s) = source {
        let username = e.dm.name_of(s).to_string();
        let display =
            e.dm.get(s, "DisplayName")
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| username.clone());
        let [r, g, b] = name_color(&username);
        let prefix = format!("<font color=\"#{r:02x}{g:02x}{b:02x}\">{display}:</font>");
        let _ = e.dm.set(msg, "PrefixText", Variant::String(prefix));
    }
    drop(e);
    with_state(lua, |s| s.text_chat.timestamps.insert(msg, now_millis()));
    Ok(msg)
}

fn uuid_like() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut words = [0u64; 2];
    for (i, w) in words.iter_mut().enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            rbx_base::time::SystemTime::now()
                .duration_since(rbx_base::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
                + i as u128,
        );
        *w = h.finish();
    }
    let [a, b] = words;
    format!(
        "{{{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}}}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        (a as u16) & 0x0fff,
        ((b >> 48) as u16 & 0x3fff) | 0x8000,
        b & 0xffff_ffff_ffff
    )
}

fn set_status(lua: &Lua, msg: InstanceId, status: &str) {
    let value = rbx_reflection::database()
        .enum_desc("TextChatMessageStatus")
        .and_then(|d| d.from_name(status));
    if let Some(v) = value {
        let _ = engine(lua).lock().dm.set(msg, "Status", Variant::Enum(v));
    }
}

fn apply_incoming(lua: &Lua, target: InstanceId, msg: InstanceId) -> Result<()> {
    let Some(func) = super::bindables::callback(lua, target, "OnIncomingMessage") else {
        return Ok(());
    };
    let arg = push_instance(lua, msg)?;
    let info = ThreadInfo {
        script: None,
        identity: current_info(lua).identity,
    };
    let Some(Ok(values)) = spawn_function_result(lua, func, MultiValue::from_vec(vec![arg]), info)
    else {
        return Ok(());
    };
    let Some(props) = values
        .into_iter()
        .next()
        .and_then(|v| crate::runtime::instance_from_value(lua, &v))
    else {
        return Ok(());
    };
    let eng = engine(lua);
    let mut e = eng.lock();
    for name in ["PrefixText", "Text", "Translation"] {
        if let Ok(Variant::String(v)) = e.dm.get(props, name)
            && !v.is_empty()
        {
            let _ = e.dm.set(msg, name, Variant::String(v));
        }
    }
    Ok(())
}

fn deliver(lua: &Lua, channel: InstanceId, msg: InstanceId) -> Result<()> {
    let tcs = engine(lua).lock().dm.find_service("TextChatService");
    if let Some(t) = tcs {
        apply_incoming(lua, t, msg)?;
    }
    apply_incoming(lua, channel, msg)?;
    set_status(lua, msg, "Success");
    let v = push_instance(lua, msg)?;
    fire_lua(
        lua,
        channel,
        "MessageReceived",
        MultiValue::from_vec(vec![v.clone()]),
    );
    if let Some(t) = tcs {
        fire_lua(lua, t, "MessageReceived", MultiValue::from_vec(vec![v]));
    }
    Ok(())
}

fn local_source(lua: &Lua, channel: InstanceId) -> Option<InstanceId> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let player = e.dm.local_player?;
    ensure_source(&mut e.dm, channel, player)
}

fn command_for(lua: &Lua, text: &str) -> Option<InstanceId> {
    let eng = engine(lua);
    let e = eng.lock();
    let tcs = e.dm.find_service("TextChatService")?;
    let word = text.split_whitespace().next()?.to_ascii_lowercase();
    e.dm.descendants(tcs).into_iter().find(|c| {
        e.dm.class_of(*c) == Some("TextChatCommand")
            && e.dm.get_bool(*c, "Enabled")
            && ["PrimaryAlias", "SecondaryAlias"].iter().any(|p| {
                e.dm.get(*c, p)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_ascii_lowercase))
                    .is_some_and(|a| !a.is_empty() && a == word)
            })
    })
}

fn send_async(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let metadata = arg_string(args.get(1)).unwrap_or_default();
    let source = local_source(lua, id);
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    let msg = new_message(lua, id, source, &escaped, &metadata)?;
    set_status(lua, msg, "Sending");
    let tcs = engine(lua).lock().dm.find_service("TextChatService");
    if let Some(t) = tcs {
        let v = push_instance(lua, msg)?;
        fire_lua(lua, t, "SendingMessage", MultiValue::from_vec(vec![v]));
    }
    if let Some(cmd) = command_for(lua, &text) {
        set_status(lua, msg, "Success");
        let src = match source {
            Some(s) => push_instance(lua, s)?,
            None => Value::Nil,
        };
        let t = Value::String(lua.create_string(&text)?);
        fire_lua(lua, cmd, "Triggered", MultiValue::from_vec(vec![src, t]));
    } else if !text.trim().is_empty() {
        deliver(lua, id, msg)?;
    }
    ret(lua, push_instance(lua, msg)?)
}

fn display_system(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let text = arg_string(args.first()).unwrap_or_default();
    let metadata = arg_string(args.get(1)).unwrap_or_default();
    let msg = new_message(lua, id, None, &text, &metadata)?;
    deliver(lua, id, msg)?;
    ret(lua, push_instance(lua, msg)?)
}

fn show_bubble(lua: &Lua, target: InstanceId, msg: InstanceId) -> Result<()> {
    let tcs = engine(lua).lock().dm.find_service("TextChatService");
    let Some(t) = tcs else {
        return Ok(());
    };
    let a = push_instance(lua, target)?;
    let b = push_instance(lua, msg)?;
    fire_lua(lua, t, "BubbleDisplayed", MultiValue::from_vec(vec![a, b]));
    let text = engine(lua)
        .lock()
        .dm
        .get(msg, "Text")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let eng = engine(lua);
    let mut e = eng.lock();
    let now = e.game_time;
    e.dm.chat_bubbles.push(rbx_dom::ChatBubble {
        target,
        text,
        created: now,
    });
    Ok(())
}

fn display_bubble(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(target) = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v))
    else {
        return none();
    };
    let text = arg_string(args.get(1)).unwrap_or_default();
    let channel = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.descendants(id)
            .into_iter()
            .find(|c| e.dm.name_of(*c) == "RBXGeneral")
    };
    let Some(channel) = channel else {
        return none();
    };
    let msg = new_message(lua, channel, None, &text, "")?;
    set_status(lua, msg, "Success");
    show_bubble(lua, target, msg)?;
    none()
}

pub fn timestamp(lua: &Lua, id: InstanceId) -> Option<i64> {
    with_state(lua, |s| s.text_chat.timestamps.get(&id).copied())
}
