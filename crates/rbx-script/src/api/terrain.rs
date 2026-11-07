use super::{Registry, Ret, none, ret};
use crate::convert::{arg_bool, arg_number, enum_value, lua_to_variant, variant_to_lua};
use crate::instance::push_instance;
use crate::runtime::{Engine, RunMode, engine, expect_instance};
use crate::types::rt;
use mlua::{Lua, Result, Table, Value};
use rbx_dom::rbx_terrain::smooth::{self, Cell, CellBox, Region};
use rbx_dom::rbx_terrain::{CELL_SIZE, LegacyGrid, SmoothGrid, conversion, legacy, region};
use rbx_dom::{InstanceId, Variant, VariantType};
use rbx_math::{CFrame, IVec3, Region3, Region3int16, Vec3, Vector3int16};
use rbx_reflection::ValueType;

const MAX_LUA_REGION: u64 = 4 * 1024 * 1024;
const MAX_CPP_REGION: u64 = 64 * 1024 * 1024;

fn typed(lua: &Lua, args: &[Value], i: usize, t: VariantType) -> Result<Variant> {
    match args.get(i) {
        None | Some(Value::Nil) => Err(rt(format!("Argument {} missing or nil", i + 1))),
        Some(v) => lua_to_variant(lua, v.clone(), &ValueType::Value(t)),
    }
}

fn int_arg(args: &[Value], i: usize) -> Result<i32> {
    arg_number(args.get(i))
        .map(|n| n as i32)
        .ok_or_else(|| rt(format!("Argument {} missing or nil", i + 1)))
}

fn xyz(args: &[Value]) -> Result<IVec3> {
    Ok(IVec3::new(
        int_arg(args, 0)?,
        int_arg(args, 1)?,
        int_arg(args, 2)?,
    ))
}

fn enum_arg(lua: &Lua, args: &[Value], i: usize, name: &str) -> Result<u32> {
    match args.get(i) {
        None | Some(Value::Nil) => Err(rt(format!("Argument {} missing or nil", i + 1))),
        Some(v) => enum_value(lua, v, name),
    }
}

fn vec3_arg(lua: &Lua, args: &[Value], i: usize) -> Result<Vec3> {
    match typed(lua, args, i, VariantType::Vector3)? {
        Variant::Vector3(v) => Ok(v),
        _ => Err(rt("Unable to cast value to Vector3")),
    }
}

fn region16_arg(lua: &Lua, args: &[Value], i: usize) -> Result<Region3int16> {
    match typed(lua, args, i, VariantType::Region3int16)? {
        Variant::Region3int16(r) => Ok(r),
        _ => Err(rt("Unable to cast value to Region3int16")),
    }
}

fn region_arg(lua: &Lua, args: &[Value], i: usize) -> Result<Region3> {
    match typed(lua, args, i, VariantType::Region3)? {
        Variant::Region3(r) => Ok(r),
        _ => Err(rt("Unable to cast value to Region3")),
    }
}

fn v16(v: Vector3int16) -> IVec3 {
    IVec3::new(v.x as i32, v.y as i32, v.z as i32)
}

fn enum_out(lua: &Lua, name: &str, v: u32) -> Result<Value> {
    variant_to_lua(lua, &Variant::Enum(v), Some(&ValueType::Enum(name.into())))
}

fn vec_out(lua: &Lua, v: Vec3) -> Result<Value> {
    variant_to_lua(lua, &Variant::Vector3(v), None)
}

enum Grid<'a> {
    Legacy(&'a mut LegacyGrid),
    Smooth(&'a mut SmoothGrid),
}

fn with_grid<R>(lua: &Lua, id: InstanceId, f: impl FnOnce(Grid<'_>) -> Result<R>) -> Result<R> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let t = e.dm.terrain_mut(id);
    let r = if let Some(g) = t.smooth.as_mut() {
        f(Grid::Smooth(g))
    } else if let Some(g) = t.legacy.as_mut() {
        f(Grid::Legacy(g))
    } else {
        return Err(rt("Terrain API is not available"));
    };
    if r.is_ok() {
        t.dirty = true;
    }
    r
}

fn with_smooth<R>(
    lua: &Lua,
    id: InstanceId,
    f: impl FnOnce(&mut SmoothGrid) -> Result<R>,
) -> Result<R> {
    with_grid(lua, id, |g| match g {
        Grid::Smooth(s) => f(s),
        Grid::Legacy(_) => Err(rt("Smooth terrain API is not available")),
    })
}

fn read_grid<R>(
    lua: &Lua,
    id: InstanceId,
    f: impl FnOnce(Option<&LegacyGrid>, Option<&SmoothGrid>) -> R,
) -> Result<R> {
    let eng = engine(lua);
    let e = eng.lock();
    match e.dm.terrain(id) {
        Some(t) if t.is_initialized() => Ok(f(t.legacy.as_ref(), t.smooth.as_ref())),
        _ => Err(rt("Terrain API is not available")),
    }
}

fn smooth_cell(material: u32, block: u32) -> Cell {
    if material == legacy::MATERIAL_WATER as u32 {
        Cell::new(smooth::MATERIAL_WATER, smooth::OCCUPANCY_MAX)
    } else if material == legacy::MATERIAL_EMPTY as u32 || block == legacy::BLOCK_EMPTY as u32 {
        Cell::AIR
    } else {
        Cell::new(
            conversion::voxel_from_material(conversion::material_from_cell_material(
                material as u8,
            )),
            conversion::occupancy_from_block(block as u8),
        )
    }
}

fn set_cells_smooth(g: &mut SmoothGrid, min: IVec3, max: IVec3, cell: Cell) {
    if min.cmpgt(max).any() {
        return;
    }
    let r = Region::new(min, max + IVec3::ONE);
    let mut b = CellBox::sized(r.size());
    if cell.material() != smooth::MATERIAL_AIR {
        let s = r.size();
        for y in 0..s.y {
            for z in 0..s.z {
                b.row_mut(0, y, z, s.x).fill(cell);
            }
        }
    }
    g.write(&r, &b);
}

fn set_cell_legacy(g: &mut LegacyGrid, p: IVec3, material: u32, block: u32, orientation: u32) {
    if material == legacy::MATERIAL_WATER as u32 {
        g.set_cell(p, legacy::water_cell(0, 0), legacy::MATERIAL_WATER);
    } else if material == legacy::MATERIAL_EMPTY as u32 || block == legacy::BLOCK_EMPTY as u32 {
        g.set_cell(p, legacy::EMPTY_CELL, legacy::MATERIAL_WATER);
    } else {
        g.set_cell(
            p,
            legacy::solid_cell(block as u8, orientation as u8),
            material as u8,
        );
    }
}

fn get_cell(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    let (m, b, o) = read_grid(lua, id, |lg, sg| {
        if let Some(g) = sg {
            let c = g.get_cell(p.x, p.y, p.z);
            if c.material() == smooth::MATERIAL_WATER {
                return (legacy::MATERIAL_WATER as u32, 0, 0);
            }
            let block = conversion::block_from_cell(c);
            if block == legacy::BLOCK_EMPTY {
                (0, 0, 0)
            } else {
                (
                    conversion::cell_material_from_material(conversion::material_from_voxel(
                        c.material(),
                    )) as u32,
                    block as u32,
                    0,
                )
            }
        } else if let Some(g) = lg {
            let c = g.get_cell(p);
            if legacy::block(c) == legacy::BLOCK_EMPTY {
                let water = !legacy::is_empty(g.get_water_cell(p));
                (
                    if water {
                        legacy::MATERIAL_WATER as u32
                    } else {
                        0
                    },
                    0,
                    0,
                )
            } else {
                (
                    g.get_material(p) as u32,
                    legacy::block(c) as u32,
                    legacy::orientation(c) as u32,
                )
            }
        } else {
            (0, 0, 0)
        }
    })?;
    ret(
        lua,
        (
            enum_out(lua, "CellMaterial", m)?,
            enum_out(lua, "CellBlock", b)?,
            enum_out(lua, "CellOrientation", o)?,
        ),
    )
}

fn set_cell(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    let m = enum_arg(lua, args, 3, "CellMaterial")?;
    let b = enum_arg(lua, args, 4, "CellBlock")?;
    let o = enum_arg(lua, args, 5, "CellOrientation")?;
    with_grid(lua, id, |g| {
        match g {
            Grid::Smooth(s) => set_cells_smooth(s, p, p, smooth_cell(m, b)),
            Grid::Legacy(l) => set_cell_legacy(l, p, m, b, o),
        }
        Ok(())
    })?;
    none()
}

fn set_cells(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let r = region16_arg(lua, args, 0)?;
    let m = enum_arg(lua, args, 1, "CellMaterial")?;
    let b = enum_arg(lua, args, 2, "CellBlock")?;
    let o = enum_arg(lua, args, 3, "CellOrientation")?;
    let (min, max) = (v16(r.min), v16(r.max));
    with_grid(lua, id, |g| {
        match g {
            Grid::Smooth(s) => set_cells_smooth(s, min, max, smooth_cell(m, b)),
            Grid::Legacy(l) => {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        for x in min.x..=max.x {
                            set_cell_legacy(l, IVec3::new(x, y, z), m, b, o);
                        }
                    }
                }
            }
        }
        Ok(())
    })?;
    none()
}

fn get_water_cell(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    let (has, f, d) = read_grid(lua, id, |lg, sg| {
        if let Some(g) = sg {
            (
                g.get_cell(p.x, p.y, p.z).material() == smooth::MATERIAL_WATER,
                0,
                0,
            )
        } else if let Some(g) = lg {
            let w = g.get_water_cell(p);
            if legacy::is_empty(w) {
                (false, 0, 0)
            } else {
                (
                    true,
                    legacy::water_force(w) as u32,
                    legacy::water_direction(w) as u32,
                )
            }
        } else {
            (false, 0, 0)
        }
    })?;
    ret(
        lua,
        (
            has,
            enum_out(lua, "WaterForce", f)?,
            enum_out(lua, "WaterDirection", d)?,
        ),
    )
}

fn set_water_cell(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    let f = enum_arg(lua, args, 3, "WaterForce")?;
    let d = enum_arg(lua, args, 4, "WaterDirection")?;
    with_grid(lua, id, |g| {
        match g {
            Grid::Smooth(s) => set_cells_smooth(
                s,
                p,
                p,
                Cell::new(smooth::MATERIAL_WATER, smooth::OCCUPANCY_MAX),
            ),
            Grid::Legacy(l) => l.set_cell(
                p,
                legacy::water_cell(f as u8, d as u8),
                legacy::MATERIAL_WATER,
            ),
        }
        Ok(())
    })?;
    none()
}

fn autowedge_cell(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    let r = with_grid(lua, id, |g| {
        Ok(match g {
            Grid::Smooth(_) => false,
            Grid::Legacy(l) => l.autowedge_cell(p),
        })
    })?;
    ret(lua, r)
}

fn autowedge_cells(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let r = region16_arg(lua, args, 0)?;
    with_grid(lua, id, |g| {
        if let Grid::Legacy(l) = g {
            l.autowedge_cells(v16(r.min), v16(r.max));
        }
        Ok(())
    })?;
    none()
}

fn cell_corner(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    ret(lua, vec_out(lua, legacy::cell_corner(p))?)
}

fn cell_center(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let p = xyz(args)?;
    ret(lua, vec_out(lua, legacy::cell_center(p))?)
}

fn world_to_cell(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    let v = vec3_arg(lua, args, 0)?;
    ret(lua, vec_out(lua, legacy::world_to_cell_floor(v).as_vec3())?)
}

fn prefer(
    lg: Option<&LegacyGrid>,
    sg: Option<&SmoothGrid>,
    world: Vec3,
    prefer_solid: bool,
) -> IVec3 {
    let center = legacy::world_to_cell_floor(world);
    if let Some(g) = sg {
        let valid = |p: IVec3| {
            (g.get_cell(p.x, p.y, p.z).material() != smooth::MATERIAL_AIR) == prefer_solid
        };
        if valid(center) {
            return center;
        }
        let mut candidates = Vec::with_capacity(27);
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let p = center + IVec3::new(x, y, z);
                    candidates.push((p, (world - legacy::cell_center(p)).length_squared()));
                }
            }
        }
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        return candidates
            .into_iter()
            .map(|c| c.0)
            .find(|p| valid(*p))
            .unwrap_or(center);
    }
    let Some(g) = lg else { return center };
    let ideal = |p: IVec3| {
        let c = g.get_cell(p);
        (prefer_solid && legacy::block(c) != legacy::BLOCK_EMPTY)
            || (!prefer_solid && legacy::is_empty(c))
    };
    let water_alt = |p: IVec3| {
        let c = g.get_cell(p);
        let has = !legacy::is_empty(g.get_water_cell(p));
        (prefer_solid && has) || (!prefer_solid && has && legacy::block(c) == legacy::BLOCK_EMPTY)
    };
    if ideal(center) {
        return center;
    }
    let eps = 0.000_488_281_25f32;
    let mut water = None;
    for y in [-1.0f32, 1.0] {
        for z in [-1.0f32, 1.0] {
            for x in [-1.0f32, 1.0] {
                let t = legacy::world_to_cell_floor(world + Vec3::new(eps * x, eps * y, eps * z));
                if ideal(t) {
                    return t;
                }
                if water_alt(t) {
                    water = Some(t);
                }
            }
        }
    }
    if let Some(w) = water {
        return w;
    }
    let mut best = center;
    let mut found = false;
    let mut best_d = 10.0f32;
    let mut water_best: Option<IVec3> = None;
    let mut water_d = 10.0f32;
    let dirs = [
        IVec3::X,
        IVec3::Z,
        -IVec3::X,
        -IVec3::Z,
        IVec3::Y,
        -IVec3::Y,
    ];
    for (i, d) in dirs.iter().enumerate() {
        let n = center + *d;
        let blocked = match i {
            2 => center.x == 0,
            3 => center.z == 0,
            5 => center.y == 0,
            _ => false,
        };
        if blocked {
            continue;
        }
        let dist = (legacy::cell_center(n) - world).length_squared();
        if ideal(n) && dist < best_d {
            found = true;
            best = n;
            best_d = dist;
        }
        if water_alt(n) && dist < water_d {
            water_best = Some(n);
            water_d = dist;
        }
    }
    match (found, water_best) {
        (false, Some(w)) => w,
        _ => best,
    }
}

fn world_to_cell_pref(lua: &Lua, id: InstanceId, args: &[Value], solid: bool) -> Result<Ret> {
    let v = vec3_arg(lua, args, 0)?;
    let p = read_grid(lua, id, |lg, sg| prefer(lg, sg, v, solid))?;
    ret(lua, vec_out(lua, p.as_vec3())?)
}

fn clear(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    with_grid(lua, id, |g| {
        match g {
            Grid::Smooth(s) => s.clear(),
            Grid::Legacy(l) => l.clear(),
        }
        Ok(())
    })?;
    none()
}

fn count_cells(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let e = eng.lock();
    let n = e.dm.terrain(id).map_or(0, |t| t.non_empty_cell_count());
    drop(e);
    ret(lua, n as i64)
}

fn copy_region(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let r = region16_arg(lua, args, 0)?;
    let (min, max) = (v16(r.min), v16(r.max));
    let created = {
        let eng = engine(lua);
        let mut guard = eng.lock();
        let e = &mut *guard;
        let (lg, sg) = match e.dm.terrain(id) {
            Some(t) if t.is_initialized() => (t.legacy.clone(), t.smooth.clone()),
            _ => return Err(rt("Terrain API is not available")),
        };
        let tr =
            e.dm.create("TerrainRegion")
                .map_err(|x| rt(x.to_string()))?;
        let _ = e.dm.set(tr, "ExtentsMin", Variant::Vector3int16(r.min));
        let _ = e.dm.set(tr, "ExtentsMax", Variant::Vector3int16(r.max));
        let data = e.dm.terrain_mut(tr);
        if let Some(src) = sg {
            let mut g = SmoothGrid::new();
            region::copy_smooth(&mut g, &src, min, max, IVec3::ZERO, false);
            data.smooth = Some(g);
        } else if let Some(src) = lg {
            let mut g = LegacyGrid::new();
            region::copy_legacy(&mut g, &src, min, max, IVec3::ZERO, false);
            data.legacy = Some(g);
        }
        data.dirty = true;
        tr
    };
    ret(lua, push_instance(lua, created)?)
}

fn paste_region(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let tr = match args.first() {
        Some(v) if !matches!(v, Value::Nil) => expect_instance(lua, v, "TerrainRegion")?,
        _ => return Err(rt("Argument 1 missing or nil")),
    };
    let corner = match typed(lua, args, 1, VariantType::Vector3int16)? {
        Variant::Vector3int16(v) => v16(v),
        _ => return Err(rt("Unable to cast value to Vector3int16")),
    };
    let paste_empty = arg_bool(args.get(2));
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    if e.dm.class_of(tr) != Some("TerrainRegion") {
        return Err(rt("region has to be a TerrainRegion"));
    }
    let emin = match e.dm.get_raw(tr, "ExtentsMin") {
        Some(Variant::Vector3int16(v)) => v16(*v),
        _ => IVec3::ZERO,
    };
    let emax = match e.dm.get_raw(tr, "ExtentsMax") {
        Some(Variant::Vector3int16(v)) => v16(*v),
        _ => IVec3::ZERO,
    };
    let (slg, ssg) = match e.dm.terrain(tr) {
        Some(t) => (t.legacy.clone(), t.smooth.clone()),
        None => (None, None),
    };
    let offset = corner - emin;
    let t = e.dm.terrain_mut(id);
    if let Some(target) = t.smooth.as_mut() {
        let src = ssg.ok_or_else(|| rt("TerrainRegion is not smooth"))?;
        region::copy_smooth(target, &src, emin, emax, offset, paste_empty);
    } else if let Some(target) = t.legacy.as_mut() {
        let src = slg.ok_or_else(|| rt("TerrainRegion is smooth"))?;
        let lim = legacy::EXTENT_RADIUS;
        let lo = (IVec3::splat(-lim) - offset).max(emin);
        let hi = (IVec3::splat(lim) - offset).min(emax);
        region::copy_legacy(target, &src, lo, hi, offset, paste_empty);
    } else {
        return Err(rt("Terrain API is not available"));
    }
    t.dirty = true;
    none()
}

fn is_edit_mode(e: &Engine) -> bool {
    matches!(e.mode, RunMode::Standalone) && e.dm.local_player.is_none() && e.local_player.is_none()
}

fn convert_to_smooth(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let eng = engine(lua);
    let mut guard = eng.lock();
    let e = &mut *guard;
    let is_terrain = e.dm.class_of(id) == Some("Terrain");
    if is_terrain && !is_edit_mode(e) {
        let t = e.dm.terrain(id);
        if t.is_some_and(|t| t.is_smooth()) {
            return Err(rt("Terrain is already smooth!"));
        }
        if !t.is_some_and(|t| t.is_initialized()) {
            return Err(rt("Terrain API is not available"));
        }
        return Err(rt("Terrain can only be converted in edit mode"));
    }
    let t = e.dm.terrain_mut(id);
    if t.smooth.is_some() {
        return Err(rt(if is_terrain {
            "Terrain is already smooth!"
        } else {
            "TerrainRegion is already smooth!"
        }));
    }
    let Some(old) = t.legacy.take() else {
        return Err(rt("Terrain API is not available"));
    };
    let mut g = SmoothGrid::new();
    conversion::convert_to_smooth(&old, &mut g);
    t.smooth = Some(g);
    t.dirty = true;
    e.dm.notify_changed(id, "IsSmooth");
    none()
}

fn region_at_resolution(r: Region3, res: f64) -> Result<Region> {
    if res != 4.0 {
        return Err(rt("Resolution has to be 4"));
    }
    let min = r.min / res as f32;
    let max = r.max / res as f32;
    let imin = IVec3::new(min.x as i32, min.y as i32, min.z as i32);
    let imax = IVec3::new(max.x as i32, max.y as i32, max.z as i32);
    if imin.as_vec3() != min || imax.as_vec3() != max {
        return Err(rt(
            "Region has to be aligned to the grid (use Region3:ExpandToGrid)",
        ));
    }
    if imin.cmpge(imax).any() {
        return Err(rt("Region cannot be empty"));
    }
    let out = Region::new(imin, imax);
    if out.chunk_count(0) > MAX_LUA_REGION {
        return Err(rt("Region is too large"));
    }
    Ok(out)
}

fn region_from_extents(min: Vec3, max: Vec3) -> Result<Region> {
    let r = Region::from_extents(min, max);
    if r.is_empty() {
        return Err(rt("Extents cannot be empty"));
    }
    if r.chunk_count(0) > MAX_CPP_REGION {
        return Err(rt("Extents are too large"));
    }
    Ok(r)
}

fn read_voxels(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(lua, args, 0)?;
    let res = arg_number(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))?;
    let rr = region_at_resolution(region, res)?;
    let b = {
        let eng = engine(lua);
        let e = eng.lock();
        match e.dm.terrain(id) {
            Some(t) => match &t.smooth {
                Some(g) => g.read(&rr, 0),
                None => return Err(rt("Smooth terrain API is not available")),
            },
            None => return Err(rt("Smooth terrain API is not available")),
        }
    };
    let s = b.size();
    let mats = lua.create_table()?;
    let occs = lua.create_table()?;
    let mut cache: Vec<(u8, Value)> = Vec::new();
    for x in 0..s.x {
        let mx = lua.create_table()?;
        let ox = lua.create_table()?;
        for y in 0..s.y {
            let my = lua.create_table()?;
            let oy = lua.create_table()?;
            for z in 0..s.z {
                let c = b.get(x, y, z);
                let m = match cache.iter().find(|(k, _)| *k == c.material()) {
                    Some((_, v)) => v.clone(),
                    None => {
                        let v = enum_out(
                            lua,
                            "Material",
                            conversion::material_from_voxel(c.material()),
                        )?;
                        cache.push((c.material(), v.clone()));
                        v
                    }
                };
                my.raw_set(z + 1, m)?;
                let occ = if c.material() == smooth::MATERIAL_AIR {
                    0.0
                } else {
                    (c.occupancy() as f64 + 1.0) / 256.0
                };
                oy.raw_set(z + 1, occ)?;
            }
            mx.raw_set(y + 1, my)?;
            ox.raw_set(y + 1, oy)?;
        }
        mats.raw_set(x + 1, mx)?;
        occs.raw_set(x + 1, ox)?;
    }
    let size = s.as_vec3();
    mats.raw_set("Size", vec_out(lua, size)?)?;
    occs.raw_set("Size", vec_out(lua, size)?)?;
    ret(lua, (mats, occs))
}

fn sub_table(t: &Table, i: i32, what: &str, dims: String) -> Result<Table> {
    match t.raw_get::<Value>(i)? {
        Value::Table(v) => Ok(v),
        _ => Err(rt(format!(
            "Bad argument {what}[{i}] to 'WriteVoxels' ({dims} array expected)"
        ))),
    }
}

fn write_voxels(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(lua, args, 0)?;
    let res = arg_number(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))?;
    if args.len() < 3 {
        return Err(rt("Argument 3 missing or nil"));
    }
    if args.len() < 4 {
        return Err(rt("Argument 4 missing or nil"));
    }
    let rr = region_at_resolution(region, res)?;
    let s = rr.size();
    let dims = format!("{}x{}x{}", s.x, s.y, s.z);
    let mats = match &args[2] {
        Value::Table(t) if t.raw_len() == s.x as usize => t.clone(),
        _ => {
            return Err(rt(format!(
                "Bad argument materials to 'WriteVoxels' ({dims} array expected)"
            )));
        }
    };
    let occs = match &args[3] {
        Value::Table(t) if t.raw_len() == s.x as usize => t.clone(),
        _ => {
            return Err(rt(format!(
                "Bad argument occupancy to 'WriteVoxels' ({dims} array expected)"
            )));
        }
    };
    let mut b = CellBox::sized(s);
    for x in 0..s.x {
        let mx = sub_table(&mats, x + 1, "materials", format!("{}x{}", s.y, s.z))?;
        let ox = sub_table(&occs, x + 1, "occupancy", format!("{}x{}", s.y, s.z))?;
        if mx.raw_len() != s.y as usize {
            return Err(rt(format!(
                "Bad argument materials[{}] to 'WriteVoxels' ({}x{} array expected)",
                x + 1,
                s.y,
                s.z
            )));
        }
        if ox.raw_len() != s.y as usize {
            return Err(rt(format!(
                "Bad argument occupancy[{}] to 'WriteVoxels' ({}x{} array expected)",
                x + 1,
                s.y,
                s.z
            )));
        }
        for y in 0..s.y {
            let my = sub_table(&mx, y + 1, "materials", format!("{}-element", s.z))?;
            let oy = sub_table(&ox, y + 1, "occupancy", format!("{}-element", s.z))?;
            if my.raw_len() != s.z as usize {
                return Err(rt(format!(
                    "Bad argument materials[{}][{}] to 'WriteVoxels' ({}-element array expected)",
                    x + 1,
                    y + 1,
                    s.z
                )));
            }
            if oy.raw_len() != s.z as usize {
                return Err(rt(format!(
                    "Bad argument occupancy[{}][{}] to 'WriteVoxels' ({}-element array expected)",
                    x + 1,
                    y + 1,
                    s.z
                )));
            }
            for z in 0..s.z {
                let mv: Value = my.raw_get(z + 1)?;
                let material = enum_value(lua, &mv, "Material").map_err(|_| {
                    rt(format!("Bad argument materials[{}][{}][{}] to 'WriteVoxels' (Enum.Material expected, got {})", x + 1, y + 1, z + 1, mv.type_name()))
                })?;
                let ov: Value = oy.raw_get(z + 1)?;
                let occ = match &ov {
                    Value::Number(n) => *n as f32,
                    Value::Integer(n) => *n as f32,
                    _ => {
                        return Err(rt(format!(
                            "Bad argument occupancy[{}][{}][{}] to 'WriteVoxels' (number expected, got {})",
                            x + 1,
                            y + 1,
                            z + 1,
                            ov.type_name()
                        )));
                    }
                };
                let vm = conversion::voxel_from_material(material);
                if vm == smooth::MATERIAL_AIR || !occ.is_finite() || occ <= 0.0 {
                    b.set(x, y, z, Cell::AIR);
                } else {
                    let oi = occ * 256.0 - 0.5;
                    b.set(x, y, z, Cell::new(vm, (oi as i32).clamp(0, 255) as u8));
                }
            }
        }
    }
    with_smooth(lua, id, |g| {
        g.write(&rr, &b);
        Ok(())
    })?;
    none()
}

fn fill_region(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let region = region_arg(lua, args, 0)?;
    let res = arg_number(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))?;
    let material = enum_arg(lua, args, 2, "Material")?;
    with_smooth(lua, id, |g| {
        let rr = region_at_resolution(region, res)?;
        let vm = conversion::voxel_from_material(material);
        let cell = if vm == smooth::MATERIAL_AIR {
            Cell::AIR
        } else {
            Cell::new(vm, smooth::OCCUPANCY_MAX)
        };
        let mut b = CellBox::sized(rr.size());
        if vm != smooth::MATERIAL_AIR {
            let s = rr.size();
            for y in 0..s.y {
                for z in 0..s.z {
                    b.row_mut(0, y, z, s.x).fill(cell);
                }
            }
        }
        g.write(&rr, &b);
        Ok(())
    })?;
    none()
}

fn fill_cell(cell: &mut Cell, material: u8, occ: i32) {
    if material == smooth::MATERIAL_AIR {
        if cell.occupancy() as i32 <= occ {
            *cell = Cell::AIR;
        } else {
            let aocc = 255 - occ;
            if cell.occupancy() as i32 > aocc {
                *cell = Cell::new(cell.material(), aocc as u8);
            }
        }
    } else if cell.occupancy() as i32 <= occ && occ != 0 {
        *cell = Cell::new(material, occ as u8);
    }
}

fn fill_block(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let cf = match typed(lua, args, 0, VariantType::CFrame)? {
        Variant::CFrame(c) => c,
        _ => return Err(rt("Unable to cast value to CoordinateFrame")),
    };
    let size = vec3_arg(lua, args, 1)?;
    let material = enum_arg(lua, args, 2, "Material")?;
    with_smooth(lua, id, |g| {
        fill_block_grid(g, &cf, size, material)?;
        Ok(())
    })?;
    none()
}

fn fill_block_grid(g: &mut SmoothGrid, cf: &CFrame, size: Vec3, material: u32) -> Result<()> {
    let half = size / 2.0;
    let r = cf.rotation;
    let ext = Vec3::new(
        r.x_axis.x.abs() * half.x + r.y_axis.x.abs() * half.y + r.z_axis.x.abs() * half.z,
        r.x_axis.y.abs() * half.x + r.y_axis.y.abs() * half.y + r.z_axis.y.abs() * half.z,
        r.x_axis.z.abs() * half.x + r.y_axis.z.abs() * half.y + r.z_axis.z.abs() * half.z,
    );
    let rr = region_from_extents(cf.translation - ext, cf.translation + ext)?;
    let s = rr.size();
    let mut b = g.read(&rr, 0);
    let vm = conversion::voxel_from_material(material);
    if vm == smooth::MATERIAL_AIR && b.is_empty() {
        return Ok(());
    }
    let clamped = (size / CELL_SIZE).min(Vec3::ONE);
    let half_offset = size * (0.5 / CELL_SIZE) + Vec3::splat(0.5);
    let corner_world = (rr.begin.as_vec3() + Vec3::splat(0.5)) * CELL_SIZE;
    let local_corner = (r.transpose() * (corner_world - cf.translation)) / CELL_SIZE;
    let ax = Vec3::new(r.x_axis.x, r.y_axis.x, r.z_axis.x);
    let ay = Vec3::new(r.x_axis.y, r.y_axis.y, r.z_axis.y);
    let az = Vec3::new(r.x_axis.z, r.y_axis.z, r.z_axis.z);
    let mut ly = local_corner;
    for y in 0..s.y {
        let mut lz = ly;
        for z in 0..s.z {
            let mut lx = lz;
            let row = b.row_mut(0, y, z, s.x);
            for cell in row.iter_mut() {
                let dist = half_offset - lx.abs();
                let fx = dist.x.min(clamped.x).max(0.0);
                let fy = dist.y.min(clamped.y).max(0.0);
                let fz = dist.z.min(clamped.z).max(0.0);
                let factor = fx.min(fy.min(fz));
                let occ = (factor * 255.0 + 0.5) as i32;
                fill_cell(cell, vm, occ);
                lx += ax;
            }
            lz += az;
        }
        ly += ay;
    }
    g.write(&rr, &b);
    Ok(())
}

fn fill_ball(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let center = vec3_arg(lua, args, 0)?;
    let radius = arg_number(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))? as f32;
    let material = enum_arg(lua, args, 2, "Material")?;
    with_smooth(lua, id, |g| {
        fill_ball_grid(g, center, radius, material, false)
    })?;
    none()
}

pub fn fill_ball_grid(
    g: &mut SmoothGrid,
    center: Vec3,
    radius: f32,
    material: u32,
    skip_water: bool,
) -> Result<()> {
    let rr = region_from_extents(center - Vec3::splat(radius), center + Vec3::splat(radius))?;
    let s = rr.size();
    let mut b = g.read(&rr, 0);
    let vm = conversion::voxel_from_material(material);
    if vm == smooth::MATERIAL_AIR && b.is_empty() {
        return Ok(());
    }
    let skip = if skip_water {
        smooth::MATERIAL_WATER as i32
    } else {
        smooth::MATERIAL_MAX as i32 + 1
    };
    let cv = center / CELL_SIZE;
    let vr = radius / CELL_SIZE;
    for y in 0..s.y {
        for z in 0..s.z {
            let mut diff = Vec3::new(
                rr.begin.x as f32,
                (y + rr.begin.y) as f32,
                (z + rr.begin.z) as f32,
            ) + Vec3::splat(0.5)
                - cv;
            let row = b.row_mut(0, y, z, s.x);
            for cell in row.iter_mut() {
                let d = diff.length();
                let occ = ((vr - d + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as i32;
                if cell.material() as i32 != skip {
                    fill_cell(cell, vm, occ);
                }
                diff.x += 1.0;
            }
        }
    }
    g.write(&rr, &b);
    Ok(())
}

pub fn register(r: &mut Registry) {
    r.add("Terrain", &["GetCell"], get_cell);
    r.add("Terrain", &["SetCell"], set_cell);
    r.add("Terrain", &["SetCells"], set_cells);
    r.add("Terrain", &["GetWaterCell"], get_water_cell);
    r.add("Terrain", &["SetWaterCell"], set_water_cell);
    r.add("Terrain", &["AutowedgeCell"], autowedge_cell);
    r.add("Terrain", &["AutowedgeCells"], autowedge_cells);
    r.add("Terrain", &["CellCornerToWorld"], cell_corner);
    r.add("Terrain", &["CellCenterToWorld"], cell_center);
    r.add("Terrain", &["WorldToCell"], world_to_cell);
    r.add("Terrain", &["WorldToCellPreferSolid"], |lua, id, a| {
        world_to_cell_pref(lua, id, a, true)
    });
    r.add("Terrain", &["WorldToCellPreferEmpty"], |lua, id, a| {
        world_to_cell_pref(lua, id, a, false)
    });
    r.add("Terrain", &["Clear"], clear);
    r.add("Terrain", &["CountCells"], count_cells);
    r.add("Terrain", &["CopyRegion"], copy_region);
    r.add("Terrain", &["PasteRegion"], paste_region);
    r.add("Terrain", &["ConvertToSmooth"], convert_to_smooth);
    r.add("TerrainRegion", &["ConvertToSmooth"], convert_to_smooth);
    r.add("Terrain", &["ReadVoxels"], read_voxels);
    r.add("Terrain", &["WriteVoxels"], write_voxels);
    r.add("Terrain", &["FillRegion"], fill_region);
    r.add("Terrain", &["FillBlock"], fill_block);
    r.add("Terrain", &["FillBall"], fill_ball);
}
