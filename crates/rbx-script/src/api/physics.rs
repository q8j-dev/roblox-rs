use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, variant_to_lua};
use crate::instance::push_instance;
use crate::runtime::{engine, expect_instance, instance_from_value};
use crate::types::{LRay, LRegion3, LVector3, normal_id_arg, rt};
use mlua::{Lua, Result, Value};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Region3, Vec3};
use rbx_physics::PhysicsWorld;
use rbx_reflection::ValueType;
use rustc_hash::FxHashSet as HashSet;

const AIR_MATERIAL: u32 = 1792;

fn ray_arg(v: Option<&Value>) -> Result<rbx_math::Ray> {
    match v {
        Some(Value::UserData(ud)) => ud
            .borrow::<LRay>()
            .map(|r| r.0)
            .map_err(|_| rt("Unable to cast value to Ray")),
        None | Some(Value::Nil) => Err(rt("Argument 1 missing or nil")),
        _ => Err(rt("Unable to cast value to Ray")),
    }
}

fn region_arg(v: Option<&Value>) -> Result<Region3> {
    match v {
        Some(Value::UserData(ud)) => ud
            .borrow::<LRegion3>()
            .map(|r| r.0)
            .map_err(|_| rt("Unable to cast value to Region3")),
        None | Some(Value::Nil) => Err(rt("Argument 1 missing or nil")),
        _ => Err(rt("Unable to cast value to Region3")),
    }
}

fn vec3_arg(v: Option<&Value>, n: usize) -> Result<Vec3> {
    match v {
        Some(Value::Vector(v)) => Ok::<_, mlua::Error>(rbx_math::Vec3::new(v.x(), v.y(), v.z())),
        None | Some(Value::Nil) => Err(rt(format!("Argument {n} missing or nil"))),
        _ => Err(rt("Unable to cast value to Vector3")),
    }
}

fn cframe_arg(v: Option<&Value>, n: usize) -> Result<CFrame> {
    match v {
        Some(Value::UserData(ud)) => ud
            .borrow::<crate::types::LCFrame>()
            .map(|r| r.0)
            .map_err(|_| rt("Unable to cast value to CoordinateFrame")),
        None | Some(Value::Nil) => Err(rt(format!("Argument {n} missing or nil"))),
        _ => Err(rt("Unable to cast value to CoordinateFrame")),
    }
}

pub fn objects_arg(lua: &Lua, v: Option<&Value>) -> Result<Vec<InstanceId>> {
    match v {
        Some(Value::Table(t)) => {
            let mut out = Vec::new();
            for pair in t.clone().sequence_values::<Value>() {
                let v = pair?;
                if let Some(id) = instance_from_value(lua, &v) {
                    out.push(id);
                } else if !matches!(v, Value::Nil) {
                    return Err(rt("Unable to cast value to Objects"));
                }
            }
            Ok(out)
        }
        None | Some(Value::Nil) => Ok(Vec::new()),
        _ => Err(rt("Unable to cast value to Objects")),
    }
}

fn ignore_set(dm: &DataModel, roots: &[InstanceId]) -> HashSet<InstanceId> {
    let mut s = HashSet::default();
    for &r in roots {
        if dm.contains(r) {
            s.insert(r);
            s.extend(dm.descendants(r));
        }
    }
    s
}

fn objects_table(lua: &Lua, ids: &[InstanceId]) -> Result<mlua::Table> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, &id) in ids.iter().enumerate() {
        t.raw_set(i + 1, push_instance(lua, id)?)?;
    }
    Ok(t)
}

pub(crate) fn with_world<R>(
    lua: &Lua,
    f: impl FnOnce(&mut PhysicsWorld, &mut DataModel) -> R,
) -> R {
    let eng = engine(lua);
    let mut e = eng.lock();
    let world = e.world.clone();
    let mut w = world.lock();
    w.sync(&mut e.dm);
    w.rebuild_assemblies();
    f(&mut w, &mut e.dm)
}

pub fn hinges_detected(lua: &Lua, id: InstanceId) -> Result<Value> {
    let n = with_world(lua, |w, _| w.vehicle_seat_hinges(id).len());
    Ok(Value::Integer(n as mlua::Integer))
}

fn material_value(lua: &Lua, m: u32) -> Result<Value> {
    variant_to_lua(
        lua,
        &Variant::Enum(m),
        Some(&ValueType::Enum("Material".into())),
    )
}

fn ray_common(
    lua: &Lua,
    ray: rbx_math::Ray,
    ignore_roots: Vec<InstanceId>,
    cubes: bool,
    ignore_water: bool,
) -> Result<Ret> {
    let hit = with_world(lua, |w, dm| {
        let ignore = ignore_set(dm, &ignore_roots);
        let hit = w.raycast_ex(
            ray.origin,
            ray.direction,
            |id| !ignore.contains(&id),
            ignore_water,
            cubes,
        );
        hit.map(|h| {
            let mat = h.material.unwrap_or_else(|| {
                dm.get(h.part, "Material")
                    .ok()
                    .and_then(|v| v.as_enum())
                    .unwrap_or(256)
            });
            (h, mat)
        })
    });
    match hit {
        Some((h, mat)) => ret(
            lua,
            (
                push_instance(lua, h.part)?,
                LVector3(h.position),
                LVector3(h.normal),
                material_value(lua, mat)?,
            ),
        ),
        None => ret(
            lua,
            (
                Value::Nil,
                LVector3(ray.origin + ray.direction),
                LVector3(Vec3::ZERO),
                material_value(lua, AIR_MATERIAL)?,
            ),
        ),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Nil) | Some(Value::Boolean(false)))
}

fn find_part_on_ray(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ray = ray_arg(args.first())?;
    let ignore = match args.get(1) {
        Some(Value::Nil) | None => Vec::new(),
        Some(v) => vec![expect_instance(lua, v, "Instance")?],
    };
    ray_common(lua, ray, ignore, truthy(args.get(2)), truthy(args.get(3)))
}

fn find_part_on_ray_list(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let ray = ray_arg(args.first())?;
    let ignore = objects_arg(lua, args.get(1))?;
    ray_common(lua, ray, ignore, truthy(args.get(2)), truthy(args.get(3)))
}

fn region_common(
    lua: &Lua,
    region: Region3,
    ignore_roots: Vec<InstanceId>,
    max: usize,
) -> Vec<InstanceId> {
    let max = max.min(100);
    let volume = (region.max - region.min).abs();
    if volume.x * volume.y * volume.z > 100000.0 {
        engine(lua).lock().print(
            crate::runtime::MessageType::Warning,
            format!(
                "Volume between min and max exceeds limit of {:.6}",
                100000.0f32
            ),
        );
    }
    with_world(lua, |w, dm| {
        let ignore = ignore_set(dm, &ignore_roots);
        w.parts_in_region(region.min, region.max, |id| !ignore.contains(&id), max)
    })
}

fn find_parts_in_region3(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(args.first())?;
    let ignore = match args.get(1) {
        Some(Value::Nil) | None => Vec::new(),
        Some(v) => vec![expect_instance(lua, v, "Instance")?],
    };
    let max = arg_number(args.get(2)).unwrap_or(20.0).max(0.0) as usize;
    let found = region_common(lua, region, ignore, max);
    ret(lua, objects_table(lua, &found)?)
}

fn find_parts_in_region3_list(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(args.first())?;
    let ignore = objects_arg(lua, args.get(1))?;
    let max = arg_number(args.get(2)).unwrap_or(20.0).max(0.0) as usize;
    let found = region_common(lua, region, ignore, max);
    ret(lua, objects_table(lua, &found)?)
}

fn is_region3_empty(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(args.first())?;
    let ignore = match args.get(1) {
        Some(Value::Nil) | None => Vec::new(),
        Some(v) => vec![expect_instance(lua, v, "Instance")?],
    };
    let found = region_common(lua, region, ignore, 1);
    let terrain =
        found.is_empty() && with_world(lua, |w, _| w.terrain.cells_in_box(region.min, region.max));
    ret(lua, found.is_empty() && !terrain)
}

fn is_region3_empty_list(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(args.first())?;
    let ignore = objects_arg(lua, args.get(1))?;
    let found = region_common(lua, region, ignore, 1);
    let terrain =
        found.is_empty() && with_world(lua, |w, _| w.terrain.cells_in_box(region.min, region.max));
    ret(lua, found.is_empty() && !terrain)
}

fn get_num_awake_parts(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    let n = with_world(lua, |w, _| w.num_awake_parts());
    ret(lua, n as i64)
}

fn get_real_physics_fps(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, 60.0f64)
}

fn get_physics_throttling(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, 100i64)
}

fn pgs_is_enabled(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, true)
}

fn set_physics_throttle_enabled(_lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    none()
}

fn get_physics_analyzer_break_on_issue(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, engine(lua).lock().physics_analyzer_break)
}

fn set_physics_analyzer_break_on_issue(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    engine(lua).lock().physics_analyzer_break = arg_bool(args.first());
    none()
}

fn get_physics_analyzer_issue(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, lua.create_table()?)
}

fn parts_under(dm: &DataModel, roots: &[InstanceId]) -> Vec<InstanceId> {
    let mut out = Vec::new();
    let mut seen = HashSet::default();
    for &r in roots {
        if !dm.contains(r) {
            continue;
        }
        let mut all = vec![r];
        all.extend(dm.descendants(r));
        for id in all {
            if dm.is_a(id, "BasePart") && seen.insert(id) {
                out.push(id);
            }
        }
    }
    out
}

fn model_parts(dm: &DataModel, model: InstanceId) -> Vec<InstanceId> {
    let mut out = Vec::new();
    let mut stack: Vec<InstanceId> = dm.children_of(model).iter().rev().copied().collect();
    while let Some(c) = stack.pop() {
        if dm.is_a(c, "BasePart") {
            out.push(c);
        } else if dm.is_a(c, "Model") {
            stack.extend(dm.children_of(c).iter().rev().copied());
        }
    }
    out
}

fn workspace_make_joints(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let roots = objects_arg(lua, args.first())?;
    with_world(lua, |w, dm| {
        for p in parts_under(dm, &roots) {
            w.join(dm, p);
        }
    });
    none()
}

fn workspace_break_joints(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let roots = objects_arg(lua, args.first())?;
    with_world(lua, |w, dm| {
        for p in parts_under(dm, &roots) {
            w.destroy_joints(dm, p);
        }
    });
    none()
}

fn join_to_outsiders(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let roots = objects_arg(lua, args.first())?;
    let mode = match args.get(1) {
        Some(v @ Value::UserData(_)) => crate::convert::enum_value(lua, v, "JointCreationMode")?,
        Some(Value::Number(n)) => *n as u32,
        Some(Value::Integer(n)) => *n as u32,
        Some(v @ Value::String(_)) => crate::convert::enum_value(lua, v, "JointCreationMode")?,
        _ => 0,
    };
    if mode == 2 {
        return none();
    }
    with_world(lua, |w, dm| {
        let parts = parts_under(dm, &roots);
        let group: HashSet<InstanceId> = parts.iter().copied().collect();
        for p in &parts {
            w.join_filtered(dm, *p, |o| !group.contains(&o));
        }
        if mode == 1 {
            for p in &parts {
                w.join_filtered(dm, *p, |o| group.contains(&o));
            }
        }
    });
    none()
}

fn unjoin_from_outsiders(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let roots = objects_arg(lua, args.first())?;
    with_world(lua, |w, dm| {
        let parts = parts_under(dm, &roots);
        let group: HashSet<InstanceId> = parts.iter().copied().collect();
        for p in &parts {
            w.sync(dm);
            let joints = w.joints_of(*p);
            for j in joints {
                let other = w.joints.get(&j).and_then(|jj| {
                    if jj.part0 == Some(*p) {
                        jj.part1
                    } else {
                        jj.part0
                    }
                });
                if other.is_some_and(|o| !group.contains(&o)) && dm.contains(j) {
                    let _ = dm.destroy(j);
                }
            }
        }
    });
    none()
}

fn part_break_joints(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    with_world(lua, |w, dm| w.destroy_joints(dm, id));
    none()
}

fn part_make_joints(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    with_world(lua, |w, dm| {
        w.join(dm, id);
    });
    none()
}

fn model_break_joints(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    with_world(lua, |w, dm| {
        for p in model_parts(dm, id) {
            w.destroy_joints(dm, p);
        }
    });
    none()
}

fn model_make_joints(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    with_world(lua, |w, dm| {
        for p in model_parts(dm, id) {
            w.join(dm, p);
        }
    });
    none()
}

fn get_mass(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let m = with_world(lua, |w, dm| {
        if w.part(id).is_some() {
            w.part_mass(id)
        } else {
            let size = rbx_physics::world::part_size(dm, id);
            rbx_physics::shape::PartShape::new(rbx_physics::world::shape_kind_of(dm, id), size)
                .volume
                * mass_density(dm, id, w.mode)
        }
    });
    ret(lua, m as f64)
}

fn mass_density(dm: &DataModel, id: InstanceId, mode: rbx_physics::world::PhysicalMode) -> f32 {
    if mode == rbx_physics::world::PhysicalMode::Legacy {
        return 1.0;
    }
    if let Ok(Variant::PhysicalProperties(p)) = dm.get(id, "CustomPhysicalProperties")
        && let Some(c) = p.custom
    {
        return c.density;
    }
    let m = dm
        .get(id, "Material")
        .ok()
        .and_then(|v| v.as_enum())
        .unwrap_or(256);
    rbx_math::materials::material_physics(m).density
}

fn get_root_part(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = with_world(lua, |w, _| w.root_part(id)).unwrap_or(id);
    ret(lua, push_instance(lua, r)?)
}

fn get_connected_parts(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let recursive = arg_bool(args.first());
    let parts = with_world(lua, |w, _| w.connected_parts(id, recursive));
    let parts = if parts.is_empty() { vec![id] } else { parts };
    ret(lua, objects_table(lua, &parts)?)
}

fn get_touching_parts(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let parts = with_world(lua, |w, _| w.touching_parts(id));
    ret(lua, objects_table(lua, &parts)?)
}

fn is_grounded(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let g = with_world(lua, |w, dm| {
        if w.part(id).is_some() {
            w.is_grounded(id)
        } else {
            dm.get_bool(id, "Anchored")
        }
    });
    ret(lua, g)
}

fn get_render_cframe(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let cf = engine(lua)
        .lock()
        .dm
        .get(id, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .unwrap_or_default();
    ret(lua, crate::types::LCFrame(cf))
}

fn resize(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let Some(face) = args.first().and_then(|v| normal_id_arg(lua, v)) else {
        return Err(rt("Unable to cast value to NormalId"));
    };
    let amount = arg_number(args.get(1)).unwrap_or(0.0).trunc() as i32;
    let ok = with_world(lua, |w, dm| {
        w.resize_part(dm, id, face, amount, |dm, s| rbx_dom::clamp_size(dm, id, s))
    });
    ret(lua, ok)
}

fn get_network_owner(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let owner = engine(lua).lock().network_owner(id);
    match owner {
        Some(p) => ret(lua, push_instance(lua, p)?),
        None => ret(lua, Value::Nil),
    }
}

fn set_network_owner(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let player = match args.first() {
        Some(Value::Nil) | None => None,
        Some(v) => Some(expect_instance(lua, v, "Instance")?),
    };
    let eng = engine(lua);
    let mut e = eng.lock();
    if let Err(msg) = e.can_set_network_owner(id) {
        return Err(rt(msg));
    }
    if player.is_some_and(|p| !e.dm.is_a(p, "Player")) {
        return Err(rt(
            "SetNetworkOwner only takes player or 'nil' instance as an argument.",
        ));
    }
    e.set_network_owner(id, player, false);
    none()
}

fn set_network_ownership_auto(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let mut e = eng.lock();
    if let Err(msg) = e.can_set_network_owner(id) {
        return Err(rt(msg));
    }
    e.set_network_owner(id, None, true);
    none()
}

fn get_network_ownership_auto(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let auto = engine(lua).lock().network_ownership_auto(id);
    ret(lua, auto)
}

fn can_set_network_ownership(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let r = engine(lua).lock().can_set_network_owner(id);
    match r {
        Ok(()) => ret(lua, (true, Value::Nil)),
        Err(m) => ret(lua, (false, m)),
    }
}

fn primary_part(dm: &DataModel, model: InstanceId) -> Option<InstanceId> {
    dm.get_ref(model, "PrimaryPart")
        .filter(|p| dm.is_descendant_of(*p, model))
}

fn computed_primary_part(dm: &DataModel, model: InstanceId) -> Option<InstanceId> {
    if let Some(p) = primary_part(dm, model) {
        return Some(p);
    }
    let mut best: Option<(InstanceId, f32)> = None;
    for d in dm.descendants(model) {
        if dm.is_a(d, "BasePart") {
            let (lo, hi) = part_extents_world(dm, d);
            let area = (hi.x - lo.x) * (hi.z - lo.z);
            if best.is_none_or(|(_, b)| area > b) {
                best = Some((d, area));
            }
        }
    }
    best.map(|b| b.0)
}

fn part_extents_local(dm: &DataModel, id: InstanceId) -> (Vec3, Vec3) {
    let s = rbx_physics::world::part_size(dm, id) * 0.5;
    (-s, s)
}

fn part_extents_world(dm: &DataModel, id: InstanceId) -> (Vec3, Vec3) {
    let cf = dm
        .get(id, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .unwrap_or_default();
    express_extents(part_extents_local(dm, id), &cf, &CFrame::default())
}

fn express_extents(local: (Vec3, Vec3), from: &CFrame, to: &CFrame) -> (Vec3, Vec3) {
    let (lo, hi) = local;
    let mut mn = Vec3::splat(f32::INFINITY);
    let mut mx = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let p = to.point_to_object_space(*from * c);
        mn = mn.min(p);
        mx = mx.max(p);
    }
    (mn, mx)
}

fn model_location(dm: &DataModel, model: InstanceId) -> CFrame {
    model_location_with(dm, model, None)
}

fn model_location_with(
    dm: &DataModel,
    model: InstanceId,
    in_primary: Option<rbx_math::Mat3>,
) -> CFrame {
    if let Some(p) = primary_part(dm, model) {
        let c = dm
            .get(p, "CFrame")
            .ok()
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
        return CFrame::new(
            c.rotation * in_primary.unwrap_or(rbx_math::Mat3::IDENTITY),
            c.translation,
        );
    }
    match computed_primary_part(dm, model) {
        Some(p) => dm
            .get(p, "CFrame")
            .ok()
            .and_then(|v| v.as_cframe())
            .unwrap_or_default(),
        None => CFrame::default(),
    }
}

fn model_extents_local(dm: &DataModel, model: InstanceId, location: &CFrame) -> (Vec3, Vec3) {
    let mut mn = Vec3::ZERO;
    let mut mx = Vec3::ZERO;
    let mut first = true;
    for d in dm.descendants(model) {
        if dm.is_a(d, "BasePart") {
            let cf = dm
                .get(d, "CFrame")
                .ok()
                .and_then(|v| v.as_cframe())
                .unwrap_or_default();
            let (lo, hi) = express_extents(part_extents_local(dm, d), &cf, location);
            if first {
                mn = lo;
                mx = hi;
                first = false;
            } else {
                mn = mn.min(lo);
                mx = mx.max(hi);
            }
        }
    }
    if first {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    (mn.min(Vec3::ZERO), mx.max(Vec3::ZERO))
}

fn get_extents_size(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let e = engine(lua);
    let e = e.lock();
    let loc = model_location_with(&e.dm, id, e.model_in_primary.get(&id).copied());
    let (lo, hi) = model_extents_local(&e.dm, id, &loc);
    ret(lua, LVector3(hi - lo))
}

fn get_model_cframe(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let e = engine(lua);
    let e = e.lock();
    let mut loc = model_location_with(&e.dm, id, e.model_in_primary.get(&id).copied());
    let (lo, hi) = model_extents_local(&e.dm, id, &loc);
    loc.translation = loc * ((lo + hi) * 0.5);
    ret(lua, crate::types::LCFrame(loc))
}

fn get_primary_part_cframe(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let e = engine(lua);
    let e = e.lock();
    match primary_part(&e.dm, id) {
        Some(p) => ret(
            lua,
            crate::types::LCFrame(
                e.dm.get(p, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default(),
            ),
        ),
        None => Err(rt(
            "Model:GetPrimaryPartCFrame() failed because no PrimaryPart has been set, or the PrimaryPart no longer exists. Please set Model.PrimaryPart before using this.",
        )),
    }
}

fn set_primary_part_cframe(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let new_cf = cframe_arg(args.first(), 1)?.orthonormalized();
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(primary) = primary_part(&e.dm, id) else {
        return Err(rt(
            "Model:SetPrimaryPartCFrame() failed because no PrimaryPart has been set, or the PrimaryPart no longer exists. Please set Model.PrimaryPart before using this.",
        ));
    };
    let old =
        e.dm.get(primary, "CFrame")
            .ok()
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
    if old == new_cf {
        return none();
    }
    let delta_t = new_cf.translation - old.translation;
    let delta_r = new_cf.rotation * old.rotation.transpose();
    let rot = CFrame::new(delta_r, Vec3::ZERO);
    let center = new_cf.translation;
    let mut parts: Vec<InstanceId> =
        e.dm.descendants(id)
            .into_iter()
            .filter(|d| e.dm.is_a(*d, "BasePart") && *d != primary)
            .collect();
    parts.push(primary);
    for p in parts {
        let mut cf =
            e.dm.get(p, "CFrame")
                .ok()
                .and_then(|v| v.as_cframe())
                .unwrap_or_default();
        cf.translation += delta_t;
        cf.translation -= center;
        cf = rot * cf;
        cf.translation += center;
        let cf = cf.orthonormalized();
        let _ = e.dm.set(p, "CFrame", Variant::CFrame(cf));
    }
    none()
}

fn translate_by(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let delta = vec3_arg(args.first(), 1)?;
    let eng = engine(lua);
    let mut e = eng.lock();
    let mut all = vec![id];
    all.extend(e.dm.descendants(id));
    for p in all {
        if e.dm.is_a(p, "BasePart") {
            let mut cf =
                e.dm.get(p, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default();
            cf.translation += delta;
            let _ = e.dm.set(p, "CFrame", Variant::CFrame(cf));
        }
    }
    none()
}

fn move_to(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let point = vec3_arg(args.first(), 1)?;
    with_world(lua, |w, dm| {
        if !w.workspace.is_some_and(|ws| dm.is_descendant_of(id, ws)) {
            return;
        }
        if computed_primary_part(dm, id).is_none() {
            return;
        }
        let loc = model_location(dm, id);
        let delta = point - loc.translation;
        let parts = model_all_parts(dm, id);
        w.move_group(dm, &parts, delta, true);
    });
    none()
}

fn model_all_parts(dm: &DataModel, model: InstanceId) -> Vec<InstanceId> {
    dm.descendants(model)
        .into_iter()
        .filter(|d| dm.is_a(*d, "BasePart"))
        .collect()
}

fn reset_orientation_to_identity(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let mut loc = model_location(&e.dm, id);
    let (lo, hi) = model_extents_local(&e.dm, id, &loc);
    loc.translation = loc * ((lo + hi) * 0.5);
    let center = loc.translation;
    let rot = CFrame::new(loc.rotation.transpose(), Vec3::ZERO);
    for p in model_all_parts(&e.dm, id) {
        let mut cf =
            e.dm.get(p, "CFrame")
                .ok()
                .and_then(|v| v.as_cframe())
                .unwrap_or_default();
        cf.translation -= center;
        cf = rot * cf;
        cf.translation += center;
        let _ = e.dm.set(p, "CFrame", Variant::CFrame(cf));
    }
    none()
}

fn set_identity_orientation(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let c = match computed_primary_part(&e.dm, id) {
        Some(p) => {
            e.dm.get(p, "CFrame")
                .ok()
                .and_then(|v| v.as_cframe())
                .unwrap_or_default()
        }
        None => CFrame::default(),
    };
    e.model_in_primary.insert(id, c.rotation.transpose());
    none()
}

fn get_last_force(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let f = with_world(lua, |w, _| {
        w.mover_wake.get(&id).map(|f| f.0).unwrap_or(Vec3::ZERO)
    });
    ret(lua, LVector3(f))
}

fn rocket_fire(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let _ = engine(lua).lock().dm.set(id, "Active", Variant::Bool(true));
    none()
}

fn rocket_abort(lua: &Lua, id: InstanceId, _: &[Value]) -> Result<Ret> {
    let _ = engine(lua)
        .lock()
        .dm
        .set(id, "Active", Variant::Bool(false));
    none()
}

fn set_desired_angle(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let v = arg_number(args.first()).unwrap_or(0.0) as f32;
    let _ = engine(lua)
        .lock()
        .dm
        .set(id, "DesiredAngle", Variant::Float(v));
    none()
}

fn experimental_solver_is_enabled(lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    ret(lua, true)
}

fn zoom_to_extents(_lua: &Lua, _id: InstanceId, _: &[Value]) -> Result<Ret> {
    none()
}

fn set_join_instance(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let target = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v));
    engine(lua).lock().join_after_move.0 = target;
    none()
}

fn set_join_target(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let target = args
        .first()
        .and_then(|v| crate::runtime::instance_from_value(lua, v));
    engine(lua).lock().join_after_move.1 = target;
    none()
}

fn join_noop(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn create_join_after_move_joints(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let Some(target) = engine(lua).lock().join_after_move.0.take() else {
        return none();
    };
    with_world(lua, |w, dm| {
        if !dm.contains(target) {
            return;
        }
        let parts = if dm.is_a(target, "BasePart") {
            vec![target]
        } else if dm.is_a(target, "Model") {
            model_parts(dm, target)
        } else {
            return;
        };
        let terrain = dm.workspace_terrain();
        for j in w.manual_joints(dm, &parts, terrain) {
            let Ok(inst) = dm.create("ManualWeld") else {
                continue;
            };
            let name = if j.terrain {
                format!("{} Terrain Joint", dm.name_of(j.part1))
            } else {
                format!(
                    "{}-to-{} Strong Joint",
                    dm.name_of(j.part0),
                    dm.name_of(j.part1)
                )
            };
            dm.set_name(inst, &name);
            dm.set_unchecked(inst, "Part0", Variant::Ref(Some(j.part0)));
            dm.set_unchecked(inst, "Part1", Variant::Ref(Some(j.part1)));
            dm.set_unchecked(inst, "C0", Variant::CFrame(j.c0));
            dm.set_unchecked(inst, "C1", Variant::CFrame(j.c1));
            dm.set_unchecked(inst, "Surface0", Variant::Int(j.surface0 as i32));
            dm.set_unchecked(inst, "Surface1", Variant::Int(j.surface1 as i32));
            let _ = dm.set_parent(inst, Some(j.parent));
        }
    });
    none()
}

pub fn register(r: &mut Registry) {
    r.add(
        "JointsService",
        &["SetJoinAfterMoveInstance"],
        set_join_instance,
    );
    r.add(
        "JointsService",
        &["SetJoinAfterMoveTarget"],
        set_join_target,
    );
    r.add(
        "JointsService",
        &["ShowPermissibleJoints", "ClearJoinAfterMoveJoints"],
        join_noop,
    );
    r.add(
        "JointsService",
        &["CreateJoinAfterMoveJoints"],
        create_join_after_move_joints,
    );
    r.add(
        "Workspace",
        &["FindPartOnRay", "findPartOnRay"],
        find_part_on_ray,
    );
    r.add(
        "Workspace",
        &["FindPartOnRayWithIgnoreList"],
        find_part_on_ray_list,
    );
    r.add(
        "Workspace",
        &["FindPartsInRegion3", "findPartsInRegion3"],
        find_parts_in_region3,
    );
    r.add(
        "Workspace",
        &["FindPartsInRegion3WithIgnoreList"],
        find_parts_in_region3_list,
    );
    r.add("Workspace", &["IsRegion3Empty"], is_region3_empty);
    r.add(
        "Workspace",
        &["IsRegion3EmptyWithIgnoreList"],
        is_region3_empty_list,
    );
    r.add("Workspace", &["GetNumAwakeParts"], get_num_awake_parts);
    r.add("Workspace", &["GetRealPhysicsFPS"], get_real_physics_fps);
    r.add(
        "Workspace",
        &["GetPhysicsThrottling"],
        get_physics_throttling,
    );
    r.add("Workspace", &["PGSIsEnabled"], pgs_is_enabled);
    r.add(
        "Workspace",
        &["ExperimentalSolverIsEnabled"],
        experimental_solver_is_enabled,
    );
    r.add(
        "Workspace",
        &["SetPhysicsThrottleEnabled"],
        set_physics_throttle_enabled,
    );
    r.add(
        "Workspace",
        &["GetPhysicsAnalyzerBreakOnIssue"],
        get_physics_analyzer_break_on_issue,
    );
    r.add(
        "Workspace",
        &["SetPhysicsAnalyzerBreakOnIssue"],
        set_physics_analyzer_break_on_issue,
    );
    r.add(
        "Workspace",
        &["GetPhysicsAnalyzerIssue"],
        get_physics_analyzer_issue,
    );
    r.add("Workspace", &["MakeJoints"], workspace_make_joints);
    r.add("Workspace", &["BreakJoints"], workspace_break_joints);
    r.add("Workspace", &["JoinToOutsiders"], join_to_outsiders);
    r.add("Workspace", &["UnjoinFromOutsiders"], unjoin_from_outsiders);
    r.add("Workspace", &["ZoomToExtents"], zoom_to_extents);
    r.add(
        "BasePart",
        &["BreakJoints", "breakJoints"],
        part_break_joints,
    );
    r.add("BasePart", &["MakeJoints", "makeJoints"], part_make_joints);
    r.add("BasePart", &["GetMass", "getMass"], get_mass);
    r.add("BasePart", &["GetRootPart"], get_root_part);
    r.add("BasePart", &["GetConnectedParts"], get_connected_parts);
    r.add("BasePart", &["GetTouchingParts"], get_touching_parts);
    r.add("BasePart", &["IsGrounded"], is_grounded);
    r.add("BasePart", &["GetRenderCFrame"], get_render_cframe);
    r.add("BasePart", &["Resize", "resize"], resize);
    r.add("BasePart", &["GetNetworkOwner"], get_network_owner);
    r.add("BasePart", &["SetNetworkOwner"], set_network_owner);
    r.add(
        "BasePart",
        &["SetNetworkOwnershipAuto"],
        set_network_ownership_auto,
    );
    r.add(
        "BasePart",
        &["GetNetworkOwnershipAuto"],
        get_network_ownership_auto,
    );
    r.add(
        "BasePart",
        &["CanSetNetworkOwnership"],
        can_set_network_ownership,
    );
    r.add("Model", &["BreakJoints", "breakJoints"], model_break_joints);
    r.add("Model", &["MakeJoints", "makeJoints"], model_make_joints);
    r.add(
        "Model",
        &["GetExtentsSize", "GetModelSize"],
        get_extents_size,
    );
    r.add("Model", &["GetModelCFrame"], get_model_cframe);
    r.add("Model", &["GetPrimaryPartCFrame"], get_primary_part_cframe);
    r.add("Model", &["SetPrimaryPartCFrame"], set_primary_part_cframe);
    r.add("Model", &["TranslateBy"], translate_by);
    r.add("Model", &["MoveTo", "moveTo", "move"], move_to);
    r.add(
        "Model",
        &["ResetOrientationToIdentity"],
        reset_orientation_to_identity,
    );
    r.add(
        "Model",
        &["SetIdentityOrientation"],
        set_identity_orientation,
    );
    r.add(
        "BodyPosition",
        &["GetLastForce", "lastForce"],
        get_last_force,
    );
    r.add(
        "BodyVelocity",
        &["GetLastForce", "lastForce"],
        get_last_force,
    );
    r.add("RocketPropulsion", &["Fire", "fire"], rocket_fire);
    r.add("RocketPropulsion", &["Abort"], rocket_abort);
    r.add("Motor", &["SetDesiredAngle"], set_desired_angle);
}
