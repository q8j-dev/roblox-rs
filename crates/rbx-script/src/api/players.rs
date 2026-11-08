use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, arg_string};
use crate::instance::push_instance;
use crate::runtime::{Engine, MessageType, engine, expect_instance, fire_lua, pump};
use crate::types::{LVector3, rt};
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec3};

fn players_of(dm: &DataModel, svc: InstanceId) -> Vec<InstanceId> {
    dm.children_of(svc)
        .iter()
        .copied()
        .filter(|c| dm.class_of(*c) == Some("Player"))
        .collect()
}

fn objects(lua: &Lua, ids: &[InstanceId]) -> Result<mlua::Table> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, id) in ids.iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, *id)?)?;
    }
    Ok(t)
}

fn get_players(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let ids = players_of(&engine(lua).lock().dm, id);
    ret(lua, objects(lua, &ids)?)
}

fn player_from_character(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let character = match args.first() {
        None | Some(Value::Nil) => return ret(lua, Value::Nil),
        Some(v) => expect_instance(lua, v, "Instance")?,
    };
    let found = {
        let e = engine(lua);
        let e = e.lock();
        players_of(&e.dm, id)
            .into_iter()
            .find(|p| e.dm.get_ref(*p, "Character") == Some(character))
    };
    match found {
        Some(p) => ret(lua, push_instance(lua, p)?),
        None => ret(lua, Value::Nil),
    }
}

fn player_by_user_id(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let uid = arg_number(args.first()).unwrap_or(0.0) as i64;
    let found = {
        let e = engine(lua);
        let e = e.lock();
        players_of(&e.dm, id).into_iter().find(|p| {
            e.dm.get(*p, "UserId")
                .ok()
                .and_then(|v| v.as_i32())
                .map(|x| x as i64)
                == Some(uid)
        })
    };
    match found {
        Some(p) => ret(lua, push_instance(lua, p)?),
        None => ret(lua, Value::Nil),
    }
}

fn create_local_player(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let uid = arg_number(args.first()).unwrap_or(0.0) as i32;
    let teleported = arg_bool(args.get(1));
    let player = make_local_player(lua, id, uid, teleported, None)?;
    ret(lua, push_instance(lua, player)?)
}

pub fn make_local_player(
    lua: &Lua,
    players: InstanceId,
    uid: i32,
    teleported: bool,
    name: Option<&str>,
) -> Result<InstanceId> {
    let player = {
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.dm.local_player.is_some_and(|p| e.dm.contains(p)) {
            return Err(rt("Local player already exists"));
        }
        let p = e.dm.create("Player").map_err(|x| rt(x.to_string()))?;
        if let Some(n) = name {
            e.dm.set_name(p, n);
        }
        e.dm.set_unchecked(p, "TeleportedIn", Variant::Bool(teleported));
        e.dm.set_unchecked(p, "UserId", Variant::Int(uid));
        e.dm.local_player = Some(p);
        e.local_player = Some(p);
        e.dm.set_parent(p, Some(players))
            .map_err(|x| rt(x.to_string()))?;
        e.dm.notify_changed(players, "LocalPlayer");
        let gear = e.dm.create("StarterGear").map_err(|x| rt(x.to_string()))?;
        let _ = e.dm.set_parent(gear, Some(p));
        create_player_gui(&mut e, p);
        p
    };
    pump(lua);
    Ok(player)
}

fn create_player_gui(e: &mut Engine, player: InstanceId) {
    if e.dm
        .find_first_child_of_class(player, "PlayerGui")
        .is_none()
        && let Ok(g) = e.dm.create("PlayerGui")
    {
        let _ = e.dm.set_parent(g, Some(player));
    }
}

pub fn ensure_default_player_scripts(e: &mut Engine) -> Option<InstanceId> {
    let starter = e.dm.get_service("StarterPlayer").ok()?;
    let scripts = match e
        .dm
        .find_first_child_of_class(starter, "StarterPlayerScripts")
    {
        Some(s) => s,
        None => {
            let s = e.dm.create("StarterPlayerScripts").ok()?;
            let _ = e.dm.set_parent(s, Some(starter));
            s
        }
    };
    for (name, file) in [
        ("ControlScript", "fonts/characterControlScript.rbxmx"),
        ("CameraScript", "fonts/characterCameraScript.rbxmx"),
    ] {
        if e.dm.find_first_child(scripts, name, false).is_some() {
            continue;
        }
        for root in load_content_model(e, file) {
            let _ = e.dm.set(root, "Archivable", Variant::Bool(false));
            let _ = e.dm.set_parent(root, Some(scripts));
        }
    }
    Some(scripts)
}

pub fn ensure_player_scripts(e: &mut Engine) -> bool {
    let Some(player) = e.dm.local_player.filter(|p| e.dm.contains(*p)) else {
        return false;
    };
    if e.dm
        .find_first_child_of_class(player, "PlayerScripts")
        .is_some()
    {
        return false;
    }
    let Ok(ps) = e.dm.create("PlayerScripts") else {
        return false;
    };
    if let Some(scripts) = ensure_default_player_scripts(e) {
        for c in e.dm.children_of(scripts).to_vec() {
            let archivable = e.dm.get_bool(c, "Archivable");
            let _ = e.dm.set(c, "Archivable", Variant::Bool(true));
            if let Ok(Some(copy)) = e.dm.clone_instance(c) {
                let _ = e.dm.set_parent(copy, Some(ps));
            }
            let _ = e.dm.set(c, "Archivable", Variant::Bool(archivable));
        }
    }
    let _ = e.dm.set_parent(ps, Some(player));
    true
}

pub fn ensure_camera_subject(e: &mut Engine) {
    let Some(player) = e.dm.local_player else {
        return;
    };
    let Some(character) =
        e.dm.get_ref(player, "Character")
            .filter(|c| e.dm.contains(*c))
    else {
        return;
    };
    let Some(humanoid) = e.dm.find_first_child_of_class(character, "Humanoid") else {
        return;
    };
    let Some(ws) = e.dm.find_service("Workspace") else {
        return;
    };
    let Some(cam) =
        e.dm.get_ref(ws, "CurrentCamera")
            .or_else(|| e.dm.find_first_child_of_class(ws, "Camera"))
    else {
        return;
    };
    let subject =
        e.dm.get_ref(cam, "CameraSubject")
            .filter(|s| e.dm.contains(*s) && e.dm.in_tree(*s));
    if subject.is_none() {
        let _ = e.dm.set(cam, "CameraSubject", Variant::Ref(Some(humanoid)));
        let _ = e.dm.set(cam, "CameraType", Variant::Enum(5));
    }
}

fn copy_children(dm: &mut DataModel, from: InstanceId, to: InstanceId) {
    for c in dm.children_of(from).to_vec() {
        if let Ok(Some(copy)) = dm.clone_instance(c) {
            let _ = dm.set_parent(copy, Some(to));
        }
    }
}

pub fn load_content_model(e: &mut Engine, rel: &str) -> Vec<InstanceId> {
    let Some(root) = e.content_root.clone() else {
        return Vec::new();
    };
    let path = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let Ok(data) = rbx_base::vfs::read(&path) else {
        return Vec::new();
    };
    match e.dm.load_model(&data) {
        Ok((roots, _)) => roots,
        Err(_) => Vec::new(),
    }
}

struct SpawnData {
    cframe: CFrame,
    force_field: i32,
}

fn choose_spawn(e: &Engine, player: InstanceId, preferred: &str) -> Option<SpawnData> {
    let ws = e.dm.find_service("Workspace")?;
    let neutral = e.dm.get_bool(player, "Neutral");
    let team =
        e.dm.get(player, "TeamColor")
            .ok()
            .and_then(|v| v.as_brick_color());
    let spawns: Vec<InstanceId> =
        e.dm.descendants(ws)
            .into_iter()
            .filter(|d| e.dm.class_of(*d) == Some("SpawnLocation") && e.dm.get_bool(*d, "Enabled"))
            .collect();
    if spawns.is_empty() {
        return None;
    }
    if !preferred.is_empty()
        && let Some(s) = spawns.iter().find(|s| e.dm.name_of(**s) == preferred)
    {
        return Some(spawn_data(e, *s));
    }
    if let Some(r) =
        e.dm.get_ref(player, "RespawnLocation")
            .filter(|r| spawns.contains(r))
    {
        return Some(spawn_data(e, r));
    }
    let candidates: Vec<InstanceId> = spawns
        .iter()
        .copied()
        .filter(|s| {
            if e.dm.get_bool(*s, "Neutral") {
                return true;
            }
            !neutral
                && e.dm
                    .get(*s, "TeamColor")
                    .ok()
                    .and_then(|v| v.as_brick_color())
                    == team
        })
        .collect();
    let pool = if candidates.is_empty() {
        spawns
    } else {
        candidates
    };
    let pick = (crate::runtime::random_seed() % pool.len() as u64) as usize;
    Some(spawn_data(e, pool[pick]))
}

fn spawn_data(e: &Engine, s: InstanceId) -> SpawnData {
    SpawnData {
        cframe: e
            .dm
            .get(s, "CFrame")
            .ok()
            .and_then(|v| v.as_cframe())
            .unwrap_or_default(),
        force_field: e
            .dm
            .get(s, "Duration")
            .ok()
            .and_then(|v| v.as_i32())
            .unwrap_or(10),
    }
}

fn world_extents(e: &Engine) -> Option<(Vec3, Vec3)> {
    let ws = e.dm.find_service("Workspace")?;
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for d in e.dm.descendants(ws) {
        if e.dm.is_a(d, "BasePart") && e.dm.class_of(d) != Some("Terrain") {
            let cf =
                e.dm.get(d, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default();
            let s = rbx_physics::world::part_size(&e.dm, d) * 0.5;
            for i in 0..8 {
                let c = Vec3::new(
                    if i & 1 == 0 { -s.x } else { s.x },
                    if i & 2 == 0 { -s.y } else { s.y },
                    if i & 4 == 0 { -s.z } else { s.z },
                );
                let p = cf * c;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    if lo.x.is_finite() {
        Some((lo, hi))
    } else {
        None
    }
}

fn joint_frame(id: rbx_math::NormalId, x: f32, y: f32, z: f32) -> CFrame {
    CFrame::new(
        rbx_physics::joins::normal_id_to_matrix(id),
        Vec3::new(x, y, z),
    )
}

pub fn build_joints(dm: &mut DataModel, humanoid: InstanceId) {
    use rbx_math::NormalId::{Left, Right, Top};
    if dm.find_first_child_of_class(humanoid, "Status").is_none()
        && let Ok(status) = dm.create_internal("Status")
    {
        let _ = dm.set(status, "Archivable", Variant::Bool(false));
        let _ = dm.set_parent(status, Some(humanoid));
    }
    let Some(character) = dm.parent_of(humanoid).filter(|m| dm.is_a(*m, "Model")) else {
        return;
    };
    if dm.get_bool(dm.root(), "ForceR15") {
        return;
    }
    let part = |dm: &DataModel, name: &str| {
        dm.find_first_child(character, name, false)
            .filter(|p| dm.is_a(*p, "BasePart"))
    };
    let Some(torso) = part(dm, "Torso") else {
        return;
    };
    let motor = |dm: &mut DataModel,
                 name: &str,
                 p0: InstanceId,
                 p1: Option<InstanceId>,
                 c0: CFrame,
                 c1: CFrame,
                 parent: InstanceId| {
        let Ok(m) = dm.create("Motor6D") else {
            return;
        };
        dm.set_name(m, name);
        let _ = dm.set(m, "MaxVelocity", Variant::Float(0.1));
        let _ = dm.set(m, "Part0", Variant::Ref(Some(p0)));
        let _ = dm.set(m, "Part1", Variant::Ref(p1));
        let _ = dm.set(m, "C0", Variant::CFrame(c0));
        let _ = dm.set(m, "C1", Variant::CFrame(c1));
        let _ = dm.set_parent(m, Some(parent));
    };
    if let Some(root) = part(dm, "HumanoidRootPart") {
        let f = joint_frame(Top, 0.0, 0.0, 0.0);
        motor(dm, "RootJoint", root, Some(torso), f, f, root);
    }
    let limbs = [
        (
            "Right Shoulder",
            "Right Arm",
            joint_frame(Right, 1.0, 0.5, 0.0),
            joint_frame(Right, -0.5, 0.5, 0.0),
        ),
        (
            "Left Shoulder",
            "Left Arm",
            joint_frame(Left, -1.0, 0.5, 0.0),
            joint_frame(Left, 0.5, 0.5, 0.0),
        ),
        (
            "Right Hip",
            "Right Leg",
            joint_frame(Right, 1.0, -1.0, 0.0),
            joint_frame(Right, 0.5, 1.0, 0.0),
        ),
        (
            "Left Hip",
            "Left Leg",
            joint_frame(Left, -1.0, -1.0, 0.0),
            joint_frame(Left, -0.5, 1.0, 0.0),
        ),
        (
            "Neck",
            "Head",
            joint_frame(Top, 0.0, 1.0, 0.0),
            joint_frame(Top, 0.0, -0.5, 0.0),
        ),
    ];
    for (name, limb, c0, c1) in limbs {
        let p1 = part(dm, limb);
        motor(dm, name, torso, p1, c0, c1, torso);
    }
}

pub fn load_character(lua: &Lua, player: InstanceId, in_game: bool, preferred: &str) -> Result<()> {
    let (old, model) = {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        let ws = e
            .dm
            .find_service("Workspace")
            .ok_or_else(|| rt("LoadCharacter can only be called when Player is in the world"))?;
        if !e
            .dm
            .find_service("Players")
            .is_some_and(|p| e.dm.parent_of(player) == Some(p))
        {
            return Err(rt(
                "LoadCharacter can only be called when Player is in the world",
            ));
        }
        let starter = e.dm.find_service("StarterPlayer");
        let mut model = None;
        if let Some(sp) = starter
            && let Some(sc) =
                e.dm.find_first_child(sp, "StarterCharacter", false)
                    .filter(|c| e.dm.class_of(*c) == Some("Model"))
            && let Ok(Some(copy)) = e.dm.clone_instance(sc)
        {
            model = Some(copy);
        }
        let force_r15 = e.dm.get_bool(e.dm.root(), "ForceR15");
        let model = match model {
            Some(m) => m,
            None => {
                let file = if force_r15 {
                    "fonts/characterR15.rbxm"
                } else {
                    "fonts/character3.rbxm"
                };
                let roots = load_content_model(e, file);
                match roots
                    .first()
                    .copied()
                    .filter(|m| e.dm.class_of(*m) == Some("Model"))
                {
                    Some(m) => m,
                    None => return Err(rt("character3.rbxm should contain a Character model")),
                }
            }
        };
        if let Some(sp) = starter
            && let Some(dh) = e.dm.find_first_child_of_class(sp, "Humanoid")
            && let Ok(Some(copy)) = e.dm.clone_instance(dh)
        {
            if let Some(h) = e.dm.find_first_child_of_class(model, "Humanoid") {
                let _ = e.dm.destroy(h);
            }
            e.dm.set_name(copy, "Humanoid");
            let _ = e.dm.set_parent(copy, Some(model));
        }
        let mut override_sound = false;
        let mut override_health = false;
        let mut override_animate = false;
        if let Some(sp) = starter
            && let Some(scs) =
                e.dm.find_first_child_of_class(sp, "StarterCharacterScripts")
        {
            for c in e.dm.children_of(scs).to_vec() {
                match e.dm.name_of(c) {
                    "Sound" => override_sound = true,
                    "Health" => override_health = true,
                    "Animate" => override_animate = true,
                    _ => {}
                }
                if let Ok(Some(copy)) = e.dm.clone_instance(c) {
                    let _ = e.dm.set_parent(copy, Some(model));
                }
            }
        }
        let mut extras = Vec::new();
        if !override_sound {
            extras.extend(load_content_model(e, "fonts/humanoidSoundNewLocal.rbxmx"));
        }
        if in_game && !override_health {
            extras.extend(load_content_model(
                e,
                "fonts/humanoidHealthRegenScript.rbxm",
            ));
        }
        if !override_animate {
            let keyframes_available =
                !e.animate_fallback || crate::content::fetch(e, "rbxassetid://180435571").is_some();
            let file = if force_r15 {
                "fonts/humanoidAnimateR15.rbxm"
            } else if keyframes_available {
                "fonts/humanoidAnimateLocalKeyframe2.rbxm"
            } else {
                "fonts/humanoidAnimateLocal.rbxm"
            };
            extras.extend(load_content_model(e, file));
        }
        for x in extras {
            let _ = e.dm.set_parent(x, Some(model));
        }
        let name = e.dm.name_of(player).to_string();
        e.dm.set_name(model, &name);
        if let Some(h) = e.dm.find_first_child_of_class(model, "Humanoid") {
            build_joints(&mut e.dm, h);
        }
        let _ = e.dm.set(model, "Archivable", Variant::Bool(false));
        let old = e.dm.get_ref(player, "Character");
        if e.backend_processing() {
            while let Some(bp) = e.dm.find_first_child_of_class(player, "Backpack") {
                let _ = e.dm.set_parent(bp, None);
            }
            if let Ok(bp) = e.dm.create("Backpack") {
                let _ = e.dm.set_parent(bp, Some(player));
                if let Some(pack) = e.dm.find_service("StarterPack") {
                    copy_children(&mut e.dm, pack, bp);
                }
                if let Some(gear) = e.dm.find_first_child_of_class(player, "StarterGear") {
                    copy_children(&mut e.dm, gear, bp);
                }
            }
            let filtering = e.dm.get_bool(ws, "FilteringEnabled");
            if !filtering {
                rebuild_gui(e, player);
            }
        }
        (old, (model, ws))
    };
    if let Some(old) = old {
        if let Ok(v) = push_instance(lua, old) {
            fire_lua(
                lua,
                player,
                "CharacterRemoving",
                MultiValue::from_vec(vec![v]),
            );
        }
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.dm.contains(old) {
            let _ = e.dm.destroy(old);
        }
    }
    let (model, ws) = model;
    {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        let _ = e.dm.set(player, "Character", Variant::Ref(Some(model)));
        let _ = e.dm.set_parent(model, Some(ws));
        let _ = e.dm.set(player, "AppearanceDidLoad", Variant::Bool(true));
        let spawn = choose_spawn(e, player, preferred).unwrap_or_else(|| {
            let mut location = Vec3::new(0.0, 100.0, 0.0);
            if let Some((lo, hi)) = world_extents(e) {
                let inside = (0..3).all(|k| lo[k] <= 0.0 && hi[k] >= 0.0);
                if !inside {
                    location += Vec3::new((lo.x + hi.x) * 0.5, lo.y, (lo.z + hi.z) * 0.5);
                }
            }
            SpawnData {
                cframe: CFrame::from_translation(location),
                force_field: 0,
            }
        });
        if spawn.force_field > 0
            && let Ok(ff) = e.dm.create("ForceField")
        {
            let _ = e.dm.set_parent(ff, Some(model));
            let due = e.game_time + spawn.force_field as f64;
            e.debris_timers.push((due, ff));
            e.debris_queue.push(ff);
        }
        let mut drop_point = spawn.cframe.translation;
        drop_point.y += 7.0;
        let world = e.world.clone();
        let mut w = world.lock();
        w.sync(&mut e.dm);
        let parts: Vec<InstanceId> =
            e.dm.descendants(model)
                .into_iter()
                .filter(|d| e.dm.is_a(*d, "BasePart"))
                .collect();
        let primary =
            e.dm.get_ref(model, "PrimaryPart")
                .or_else(|| e.dm.find_first_child(model, "Torso", false))
                .or_else(|| parts.first().copied());
        if let Some(p) = primary {
            let at =
                e.dm.get(p, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default()
                    .translation;
            w.move_group(&mut e.dm, &parts, drop_point - at, false);
            let mut look = spawn.cframe.look_vector();
            if (look - Vec3::Y).length() < 1e-4 {
                look = spawn.cframe.up_vector();
            }
            look.y = 0.0;
            let look = look.normalize_or(Vec3::NEG_Z);
            let cur =
                e.dm.get(p, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default();
            let target = CFrame::look_at(cur.translation, cur.translation + look, Vec3::Y);
            let delta = target * cur.inverse();
            std::mem::drop(w);
            for part in &parts {
                let cf =
                    e.dm.get(*part, "CFrame")
                        .ok()
                        .and_then(|v| v.as_cframe())
                        .unwrap_or_default();
                let _ = e.dm.set(
                    *part,
                    "CFrame",
                    Variant::CFrame((delta * cf).orthonormalized()),
                );
            }
        }
    }
    pump(lua);
    if let Ok(v) = push_instance(lua, model) {
        fire_lua(lua, player, "CharacterAdded", MultiValue::from_vec(vec![v]));
    }
    pump(lua);
    Ok(())
}

fn rebuild_gui(e: &mut Engine, player: InstanceId) {
    let Some(gui) = e.dm.find_first_child_of_class(player, "PlayerGui") else {
        return;
    };
    let Some(starter) = e.dm.find_service("StarterGui") else {
        return;
    };
    let copied = e.gui_copied.contains(&player);
    if !copied {
        copy_children(&mut e.dm, starter, gui);
        e.gui_copied.insert(player);
    } else if e.dm.get_bool(starter, "ResetPlayerGuiOnSpawn") {
        let _ = e.dm.destroy_all_children(gui);
        copy_children(&mut e.dm, starter, gui);
    }
}

fn player_load_character(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let in_game = match args.first() {
        None | Some(Value::Nil) => true,
        Some(v) => arg_bool(Some(v)),
    };
    load_character(lua, id, in_game, "")?;
    none()
}

fn remove_character(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let old = engine(lua).lock().dm.get_ref(id, "Character");
    if let Some(old) = old {
        if let Ok(v) = push_instance(lua, old) {
            fire_lua(lua, id, "CharacterRemoving", MultiValue::from_vec(vec![v]));
        }
        let eng = engine(lua);
        let mut e = eng.lock();
        let _ = e.dm.set(id, "Character", Variant::Ref(None));
        if e.dm.contains(old) {
            let _ = e.dm.destroy(old);
        }
    }
    none()
}

fn distance_from_character(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let point = match args.first() {
        Some(Value::UserData(ud)) => ud
            .borrow::<LVector3>()
            .map(|v| v.0)
            .map_err(|_| rt("Unable to cast value to Vector3"))?,
        _ => return Err(rt("Argument 1 missing or nil")),
    };
    let e = engine(lua);
    let e = e.lock();
    let head =
        e.dm.get_ref(id, "Character")
            .and_then(|c| e.dm.find_first_child(c, "Head", false));
    match head {
        Some(h) => {
            let p =
                e.dm.get(h, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default()
                    .translation;
            ret(lua, (p - point).length() as f64)
        }
        None => ret(lua, 0.0f64),
    }
}

fn kick(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let msg = arg_string(args.first()).unwrap_or_default();
    let eng = engine(lua);
    let mut e = eng.lock();
    if !msg.is_empty() {
        e.print(MessageType::Info, format!("Kicked: {msg}"));
    }
    if e.dm.local_player == Some(id) {
        e.dm.local_player = None;
        e.local_player = None;
    }
    let _ = e.dm.destroy(id);
    none()
}

fn get_mouse(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let mouse = {
        let eng = engine(lua);
        let mut e = eng.lock();
        match e.mice.get(&id).copied().filter(|m| e.dm.contains(*m)) {
            Some(m) => m,
            None => {
                let m =
                    e.dm.create_internal("PlayerMouse")
                        .or_else(|_| e.dm.create_internal("Mouse"))
                        .map_err(|x| rt(x.to_string()))?;
                e.dm.retain(m);
                e.mice.insert(id, m);
                m
            }
        }
    };
    ret(lua, push_instance(lua, mouse)?)
}

fn has_appearance_loaded(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let v = engine(lua).lock().dm.get_bool(id, "AppearanceDidLoad");
    ret(lua, v)
}

fn load_character_appearance(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let _ = engine(lua)
        .lock()
        .dm
        .set(id, "AppearanceDidLoad", Variant::Bool(true));
    none()
}

fn clear_character_appearance(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(c) = e.dm.get_ref(id, "Character") else {
        return none();
    };
    for child in e.dm.children_of(c).to_vec() {
        let class = e.dm.class_of(child).unwrap_or("");
        if matches!(
            class,
            "Hat"
                | "Accoutrement"
                | "Shirt"
                | "Pants"
                | "ShirtGraphic"
                | "BodyColors"
                | "CharacterMesh"
        ) {
            let _ = e.dm.destroy(child);
        }
    }
    none()
}

fn player_move(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let humanoid = {
        let e = engine(lua);
        let e = e.lock();
        e.dm.get_ref(id, "Character")
            .and_then(|c| e.dm.find_first_child_of_class(c, "Humanoid"))
    };
    if let Some(h) = humanoid {
        super::call(
            lua,
            h,
            "Humanoid",
            "Move",
            MultiValue::from_vec(args.to_vec()),
        )?;
    }
    none()
}

fn web_false(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn web_zero(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, 0i64)
}

fn role_in_group(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, "Guest")
}

fn friends_online(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn data_ready(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    engine(lua)
        .lock()
        .dm
        .set_unchecked(id, "DataReady", Variant::Bool(true));
    ret(lua, true)
}

fn save_value(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let key = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    let stored = crate::convert::lua_to_variant(lua, v, &rbx_reflection::ValueType::Variant)
        .unwrap_or(Variant::Nil);
    engine(lua).lock().player_data.insert((id, key), stored);
    none()
}

fn load_value(lua: &Lua, id: InstanceId, args: &[Value], default: Value) -> Result<Ret> {
    let key = arg_string(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))?;
    let v = engine(lua).lock().player_data.get(&(id, key)).cloned();
    match v {
        Some(v) => ret(lua, crate::convert::variant_to_lua(lua, &v, None)?),
        None => ret(lua, default),
    }
}

fn load_number(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_value(lua, id, args, Value::Number(0.0))
}

fn load_string(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_value(lua, id, args, Value::String(lua.create_string("")?))
}

fn load_boolean(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_value(lua, id, args, Value::Boolean(false))
}

fn load_instance(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    load_value(lua, id, args, Value::Nil)
}

pub fn register(r: &mut Registry) {
    r.add(
        "Players",
        &["GetPlayers", "getPlayers", "players"],
        get_players,
    );
    r.add(
        "Players",
        &[
            "GetPlayerFromCharacter",
            "playerFromCharacter",
            "getPlayerFromCharacter",
        ],
        player_from_character,
    );
    r.add(
        "Players",
        &["GetPlayerByUserId", "GetPlayerByID", "GetPlayerById"],
        player_by_user_id,
    );
    r.add("Players", &["CreateLocalPlayer"], create_local_player);
    r.add("Player", &["LoadCharacter"], player_load_character);
    r.add("Player", &["RemoveCharacter"], remove_character);
    r.add(
        "Player",
        &["DistanceFromCharacter"],
        distance_from_character,
    );
    r.add("Player", &["Kick"], kick);
    r.add("Player", &["GetMouse"], get_mouse);
    r.add("Player", &["HasAppearanceLoaded"], has_appearance_loaded);
    r.add(
        "Player",
        &["LoadCharacterAppearance"],
        load_character_appearance,
    );
    r.add(
        "Player",
        &["ClearCharacterAppearance"],
        clear_character_appearance,
    );
    r.add("Player", &["Move"], player_move);
    r.add(
        "Player",
        &[
            "IsFriendsWith",
            "isFriendsWith",
            "IsBestFriendsWith",
            "IsInGroup",
        ],
        web_false,
    );
    r.add(
        "Player",
        &["GetRankInGroup", "GetWebPersonalServerRank"],
        web_zero,
    );
    r.add("Player", &["GetRoleInGroup"], role_in_group);
    r.add("Player", &["GetFriendsOnline"], friends_online);
    r.add(
        "Player",
        &["WaitForDataReady", "waitForDataReady"],
        data_ready,
    );
    r.add(
        "Player",
        &[
            "SaveNumber",
            "saveNumber",
            "SaveString",
            "saveString",
            "SaveBoolean",
            "saveBoolean",
            "SaveInstance",
            "saveInstance",
        ],
        save_value,
    );
    r.add("Player", &["LoadNumber", "loadNumber"], load_number);
    r.add("Player", &["LoadString", "loadString"], load_string);
    r.add("Player", &["LoadBoolean", "loadBoolean"], load_boolean);
    r.add("Player", &["LoadInstance", "loadInstance"], load_instance);
}
