use crate::math::{from_parry_vec, to_parry_vec, to_pose};
use crate::world::{ContactPair, PhysicalMode, PhysicsWorld};
use parry3d::bounding_volume::Aabb;
use parry3d::math::Pose3;
use parry3d::query::{DefaultQueryDispatcher, PersistentQueryDispatcher, Ray, RayCast};
use parry3d::shape::SharedShape;
use rbx_dom::rbx_terrain::changes::Changes;
use rbx_dom::rbx_terrain::smooth::{self, Region};
use rbx_dom::rbx_terrain::{
    CELL_SIZE, LegacyGrid, SmoothGrid, conversion, legacy, material_table, mesher,
};
use rbx_dom::{DataModel, InstanceId};
use rbx_math::{CFrame, IVec3, Mat3, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub const SMOOTH_CHUNK_LOG2: i32 = 3;
pub const SMOOTH_CHUNK: i32 = 1 << SMOOTH_CHUNK_LOG2;
const LARGE_CONTACT_VOLUME: i64 = 64;
pub const TERRAIN_FRICTION: f32 = 0.5;
pub const TERRAIN_ELASTICITY: f32 = 0.3;
const MC_SEARCH_RAY_MAX: f32 = 2048.0;
const MC_RAY_ZERO_SLOPE_TOLERANCE: f32 = 0.0005;
const MC_HUGE_VAL: f32 = 9_999_999.0;

#[derive(Clone, Copy, Debug)]
pub struct TerrainRayHit {
    pub distance: f32,
    pub position: Vec3,
    pub normal: Vec3,
    pub material: u32,
}

const BLOCK_FACES: [(usize, f32); 6] = [
    (1, -1.0),
    (0, 1.0),
    (2, -1.0),
    (0, -1.0),
    (2, 1.0),
    (1, 1.0),
];

fn block_cell_hit(origin: Vec3, dir: Vec3, cell: IVec3) -> Option<(Vec3, Vec3)> {
    let lo = legacy::cell_corner(cell).to_array();
    let o = origin.to_array();
    let d = dir.to_array();
    for (axis, s) in BLOCK_FACES {
        if s * d[axis] >= 0.0 {
            continue;
        }
        let plane = if s > 0.0 {
            lo[axis] + CELL_SIZE
        } else {
            lo[axis]
        };
        if s * (o[axis] - plane) < 0.0 {
            continue;
        }
        let t = (plane - o[axis]) / d[axis];
        let mut p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        p[axis] = plane;
        let inside = (0..3)
            .filter(|i| *i != axis)
            .all(|i| p[i] >= lo[i] && p[i] <= lo[i] + CELL_SIZE);
        if inside {
            let mut n = [0.0f32; 3];
            n[axis] = s;
            return Some((Vec3::from_array(p), Vec3::from_array(n)));
        }
    }
    None
}

static CELL_SHAPES: LazyLock<Vec<SharedShape>> = LazyLock::new(|| {
    let h = CELL_SIZE * 0.5;
    let pts = |list: &[[f32; 3]]| -> SharedShape {
        let v: Vec<_> = list
            .iter()
            .map(|p| to_parry_vec(Vec3::new(p[0] * h, p[1] * h, p[2] * h)))
            .collect();
        SharedShape::convex_hull(&v).unwrap_or_else(|| SharedShape::cuboid(h, h, h))
    };
    let mut shapes = vec![SharedShape::cuboid(h, h, h)];
    for points in &legacy::BLOCK_POINTS[1..] {
        shapes.push(pts(points));
    }
    shapes
});

pub fn cell_shape(cell: u8) -> Option<&'static SharedShape> {
    let b = legacy::block(cell);
    if b >= legacy::BLOCK_EMPTY {
        return None;
    }
    CELL_SHAPES.get(b as usize)
}

pub fn cell_frame(pos: IVec3, cell: u8) -> CFrame {
    let o = legacy::orientation(cell);
    let rot = if o > 0 {
        Mat3::from_rotation_y(o as f32 * std::f32::consts::FRAC_PI_2)
    } else {
        Mat3::IDENTITY
    };
    CFrame {
        rotation: rot,
        translation: legacy::cell_center(pos),
    }
}

#[derive(Clone, Copy, Default)]
struct ChunkSlice {
    solid: u64,
    water: u64,
}

#[derive(Clone)]
struct SmoothMesh {
    shape: Option<SharedShape>,
    materials: Vec<u8>,
    water: Option<SharedShape>,
}

#[derive(Default)]
pub struct TerrainCollider {
    pub id: Option<InstanceId>,
    pub legacy: Option<LegacyGrid>,
    pub smooth: Option<SmoothGrid>,
    generation: Option<u64>,
    partition: HashMap<IVec3, [ChunkSlice; 8]>,
    meshes: parking_lot::Mutex<HashMap<IVec3, Option<SmoothMesh>>>,
    pub contacts: HashMap<(InstanceId, IVec3), ContactPair>,
    pub touching: HashMap<InstanceId, i64>,
    pub water_parts: HashSet<InstanceId>,
    pub buoyant: HashSet<InstanceId>,
}

pub struct ChunkTouch {
    pub id: IVec3,
    pub solid: bool,
    pub water: bool,
}

impl TerrainCollider {
    pub fn is_active(&self) -> bool {
        self.id.is_some()
            && (self
                .legacy
                .as_ref()
                .is_some_and(|g| g.non_empty_cell_count() > 0)
                || self.smooth.as_ref().is_some_and(|g| g.is_allocated()))
    }

    pub fn sync(&mut self, dm: &DataModel) -> Vec<Aabb> {
        let mut changed = Vec::new();
        let id = dm.workspace_terrain();
        let data = id.and_then(|i| dm.terrain(i));
        let Some(data) = data.filter(|d| d.is_initialized()) else {
            if self.id.is_some() || self.legacy.is_some() || self.smooth.is_some() {
                *self = TerrainCollider::default();
                changed.push(Aabb::new(
                    parry3d::math::Vector::splat(-1e9),
                    parry3d::math::Vector::splat(1e9),
                ));
            }
            return changed;
        };
        let full = self.id != id
            || self.legacy.is_some() != data.legacy.is_some()
            || self.smooth.is_some() != data.smooth.is_some();
        let log = match (&data.legacy, &data.smooth) {
            (Some(g), _) => &g.changes,
            (_, Some(g)) => &g.changes,
            _ => return changed,
        };
        let generation = log.generation();
        if !full && self.generation == Some(generation) {
            return changed;
        }
        let changes = if full {
            Changes::Everything
        } else {
            log.since(self.generation.unwrap_or(0))
        };
        match changes {
            Changes::Everything => {
                self.id = id;
                self.legacy = data.legacy.clone();
                self.smooth = data.smooth.clone();
                self.partition.clear();
                self.meshes.lock().clear();
                self.contacts.clear();
                if let Some(g) = &self.smooth {
                    let regions = g.non_empty_regions();
                    let mut ids = HashSet::new();
                    for r in regions {
                        for c in r.expand(1).chunk_ids(SMOOTH_CHUNK_LOG2 as u32) {
                            ids.insert(c);
                        }
                    }
                    for c in ids {
                        self.update_partition(c);
                    }
                }
            }
            Changes::Since(entries) => {
                let entries = entries.to_vec();
                self.legacy = data.legacy.clone();
                self.smooth = data.smooth.clone();
                let mut dirty = HashSet::new();
                for (min, max) in entries {
                    changed.push(Aabb::new(
                        to_parry_vec(min.as_vec3() * CELL_SIZE - Vec3::ONE),
                        to_parry_vec((max + IVec3::ONE).as_vec3() * CELL_SIZE + Vec3::ONE),
                    ));
                    let r = Region::new(min, max + IVec3::ONE).expand(1);
                    for c in r.chunk_ids(SMOOTH_CHUNK_LOG2 as u32) {
                        dirty.insert(c);
                    }
                    if self.legacy.is_some() {
                        let lo = legacy::world_to_cell_floor(min.as_vec3() * CELL_SIZE);
                        let hi = legacy::world_to_cell_floor(max.as_vec3() * CELL_SIZE);
                        self.contacts
                            .retain(|(_, cell), _| !(cell.cmpge(lo).all() && cell.cmple(hi).all()));
                    }
                }
                if self.smooth.is_some() {
                    for c in &dirty {
                        self.meshes.lock().remove(c);
                        self.update_partition(*c);
                    }
                    self.contacts.retain(|(_, chunk), _| !dirty.contains(chunk));
                }
            }
        }
        self.generation = Some(generation);
        changed
    }

    fn update_partition(&mut self, id: IVec3) {
        let Some(grid) = &self.smooth else { return };
        let region = Region::from_chunk(id, SMOOTH_CHUNK_LOG2 as u32).expand(1);
        let b = grid.read(&region, 0);
        if b.is_empty() {
            self.partition.remove(&id);
            return;
        }
        const SIZE: usize = (SMOOTH_CHUNK as usize) + 2;
        let mut bits = [[0u32; SIZE]; SIZE];
        for (y, plane) in bits.iter_mut().enumerate() {
            for (z, row) in plane.iter_mut().enumerate() {
                let mut solid = 0u32;
                let mut water = 0u32;
                for x in 0..SIZE {
                    let m = b.get(x as i32, y as i32, z as i32).material();
                    solid |= ((m > smooth::MATERIAL_WATER) as u32) << x;
                    water |= ((m == smooth::MATERIAL_WATER) as u32) << x;
                }
                *row = solid | (water << 16);
            }
        }
        for plane in bits.iter_mut() {
            for row in plane.iter_mut() {
                *row = (*row << 1) | *row | (*row >> 1);
            }
        }
        let mut temp = [[0u32; SIZE]; SIZE];
        for y in 0..SIZE {
            for z in 1..SIZE - 1 {
                temp[y][z] = bits[y][z - 1] | bits[y][z] | bits[y][z + 1];
            }
        }
        for y in 1..SIZE - 1 {
            for z in 0..SIZE {
                bits[y][z] = temp[y - 1][z] | temp[y][z] | temp[y + 1][z];
            }
        }
        let mut slices = [ChunkSlice::default(); 8];
        for (y, slice) in slices.iter_mut().enumerate() {
            let mut s = 0u64;
            let mut w = 0u64;
            for z in 0..SMOOTH_CHUNK as usize {
                let solid_row = ((bits[y + 1][z + 1] >> 1) & 0xff) as u64;
                let water_row = ((bits[y + 1][z + 1] >> 17) & 0xff) as u64;
                s |= solid_row << (z * 8);
                w |= water_row << (z * 8);
            }
            *slice = ChunkSlice { solid: s, water: w };
        }
        self.partition.insert(id, slices);
    }

    pub fn chunks_touching(&self, min: Vec3, max: Vec3) -> Vec<ChunkTouch> {
        let region = Region::from_extents(min, max);
        let mut out = Vec::new();
        if region.is_empty() {
            return out;
        }
        let min_pos = region.begin;
        let max_pos = region.end - IVec3::ONE;
        let lo = (min_pos - IVec3::ONE) >> SMOOTH_CHUNK_LOG2;
        let hi = (max_pos + IVec3::ONE) >> SMOOTH_CHUNK_LOG2;
        if (hi - lo).as_i64vec3().element_product() > 4096 {
            for (id, slices) in &self.partition {
                if id.cmpge(lo).all() && id.cmple(hi).all() {
                    Self::touch(*id, slices, min_pos, max_pos, &mut out);
                }
            }
            out.sort_by_key(|t| (t.id.y, t.id.z, t.id.x));
            return out;
        }
        for cy in lo.y..=hi.y {
            for cz in lo.z..=hi.z {
                for cx in lo.x..=hi.x {
                    let id = IVec3::new(cx, cy, cz);
                    if let Some(slices) = self.partition.get(&id) {
                        Self::touch(id, slices, min_pos, max_pos, &mut out);
                    }
                }
            }
        }
        out
    }

    fn touch(
        id: IVec3,
        slices: &[ChunkSlice; 8],
        min_pos: IVec3,
        max_pos: IVec3,
        out: &mut Vec<ChunkTouch>,
    ) {
        let off = id << SMOOTH_CHUNK_LOG2;
        let top = IVec3::splat(SMOOTH_CHUNK - 1);
        let rmin = (min_pos - off).max(IVec3::ZERO).min(top);
        let rmax = (max_pos - off).max(IVec3::ZERO).min(top);
        let mut mask = 0u64;
        for z in rmin.z..=rmax.z {
            for x in rmin.x..=rmax.x {
                mask |= 1u64 << (z * 8 + x);
            }
        }
        let mut solid = false;
        let mut water = false;
        for y in rmin.y..=rmax.y {
            let s = &slices[y as usize];
            solid |= s.solid & mask != 0;
            water |= s.water & mask != 0;
            if solid && water {
                break;
            }
        }
        if solid || water {
            out.push(ChunkTouch { id, solid, water });
        }
    }

    fn mesh(&self, id: IVec3) -> Option<SmoothMesh> {
        if let Some(m) = self.meshes.lock().get(&id) {
            return m.clone();
        }
        let grid = self.smooth.as_ref()?;
        let region = Region::from_chunk(id, SMOOTH_CHUNK_LOG2 as u32).expand(1);
        let b = grid.read(&region, 0);
        let mut result = None;
        if !b.is_empty() {
            let options = mesher::Options {
                materials: material_table(),
                generate_water: true,
            };
            let geom = mesher::generate_geometry(&b, region.begin, 0, &options);
            let mut indices = Vec::new();
            let mut water_indices = Vec::new();
            let mut materials = Vec::new();
            for tri in geom.indices.as_chunks::<3>().0 {
                let v = [
                    &geom.vertices[tri[0] as usize],
                    &geom.vertices[tri[1] as usize],
                    &geom.vertices[tri[2] as usize],
                ];
                if mesher::BasicMesh::is_water(v[0], v[1], v[2]) {
                    water_indices.push([tri[0], tri[1], tri[2]]);
                    continue;
                }
                indices.push([tri[0], tri[1], tri[2]]);
                materials.push(v[0].material);
            }
            let verts: Vec<_> = geom
                .vertices
                .iter()
                .map(|v| to_parry_vec(v.position * CELL_SIZE))
                .collect();
            let build = |idx: Vec<[u32; 3]>| {
                if idx.is_empty() {
                    None
                } else {
                    SharedShape::trimesh(verts.clone(), idx).ok()
                }
            };
            let shape = build(indices);
            let water = build(water_indices);
            if shape.is_some() || water.is_some() {
                result = Some(SmoothMesh {
                    shape,
                    materials,
                    water,
                });
            }
        }
        self.meshes.lock().insert(id, result.clone());
        result
    }

    pub fn triangle_material(&self, chunk: IVec3, triangle: u32) -> Option<u32> {
        let m = self.mesh(chunk)?;
        let n = m.materials.len();
        if n == 0 {
            return None;
        }
        Some(conversion::material_from_voxel(
            m.materials[triangle as usize % n],
        ))
    }

    pub fn cells_in_box(&self, min: Vec3, max: Vec3) -> bool {
        if !self.is_active() {
            return false;
        }
        if let Some(g) = &self.legacy {
            let lo = legacy::world_to_cell_floor(min);
            let hi = legacy::world_to_cell_floor(max);
            for id in g.non_empty_chunks_in_region(lo, hi) {
                let a = legacy::chunk_min(id).max(lo);
                let b = legacy::chunk_max(id).min(hi);
                for y in a.y..=b.y {
                    for z in a.z..=b.z {
                        for x in a.x..=b.x {
                            if legacy::block(g.get_cell(IVec3::new(x, y, z))) != legacy::BLOCK_EMPTY
                            {
                                return true;
                            }
                        }
                    }
                }
            }
            return false;
        }
        if let Some(g) = &self.smooth {
            let region = Region::from_extents(min, max);
            if region.is_empty() {
                return false;
            }
            return !g.read(&region, 0).is_empty();
        }
        false
    }

    pub fn water_at_cell(&self, pos: IVec3) -> bool {
        if let Some(g) = &self.legacy {
            return !legacy::is_empty(g.get_water_cell(pos));
        }
        if let Some(g) = &self.smooth {
            return g.get_cell(pos.x, pos.y, pos.z).material() == smooth::MATERIAL_WATER;
        }
        false
    }

    pub fn raycast(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        ignore_water: bool,
    ) -> Option<TerrainRayHit> {
        self.raycast_opts(origin, dir, max, ignore_water, false)
    }

    pub fn raycast_opts(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        ignore_water: bool,
        cubes: bool,
    ) -> Option<TerrainRayHit> {
        if !self.is_active() {
            return None;
        }
        if self.legacy.is_some() {
            return self.raycast_legacy(origin, dir, max, ignore_water, cubes);
        }
        if self.smooth.is_some() {
            return self.raycast_smooth(origin, dir, max, ignore_water);
        }
        None
    }

    fn raycast_legacy(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        ignore_water: bool,
        cubes: bool,
    ) -> Option<TerrainRayHit> {
        let g = self.legacy.as_ref()?;
        let max = max.min(MC_SEARCH_RAY_MAX);
        let o = origin.to_array();
        let d = dir.to_array();
        if o.iter().chain(d.iter()).any(|v| !v.is_finite()) {
            return None;
        }
        if o.iter().any(|v| v.abs() > MC_HUGE_VAL) {
            return None;
        }
        let mut cell = legacy::world_to_cell_floor(origin);
        let started_in_water = g.cell_has_water(g.get_cell(cell), cell);
        let mut time_step = [0.0f32; 3];
        let mut next_time = [0.0f32; 3];
        let mut direction = [0i32; 3];
        let corner = legacy::cell_corner(cell).to_array();
        for i in 0..3 {
            time_step[i] = if d[i].abs() < MC_RAY_ZERO_SLOPE_TOLERANCE {
                MC_HUGE_VAL
            } else {
                CELL_SIZE / d[i].abs()
            };
            if d[i] > 0.0 {
                next_time[i] = (corner[i] + CELL_SIZE - o[i]) * time_step[i] / CELL_SIZE;
                direction[i] = 1;
            } else {
                next_time[i] = (o[i] - corner[i]) * time_step[i] / CELL_SIZE;
                direction[i] = -1;
            }
        }
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(dir));
        loop {
            let c = g.get_cell(cell);
            let mut found = None;
            if legacy::block(c) != legacy::BLOCK_EMPTY {
                if cubes || legacy::block(c) == legacy::BLOCK_SOLID {
                    found = block_cell_hit(origin, dir, cell);
                } else if let Some(shape) = cell_shape(c) {
                    let pose = to_pose(&cell_frame(cell, c));
                    if let Some(hit) = shape.cast_ray_and_get_normal(&pose, &ray, f32::MAX, true) {
                        if hit.time_of_impact > 0.0 {
                            found = Some((
                                origin + dir * hit.time_of_impact,
                                from_parry_vec(hit.normal),
                            ));
                        } else {
                            let behind = origin - dir * 1e-4;
                            if !shape.contains_point(&pose, to_parry_vec(behind)) {
                                found = Some((origin, from_parry_vec(hit.normal)));
                            }
                        }
                    }
                }
            }
            if found.is_none() && !ignore_water && g.cell_has_water(c, cell) != started_in_water {
                found = block_cell_hit(origin, dir, cell);
            }
            if let Some((position, normal)) = found {
                let distance = dir.dot(position - origin);
                if distance <= 0.0 {
                    return None;
                }
                return Some(TerrainRayHit {
                    distance,
                    position,
                    normal,
                    material: conversion::GRASS,
                });
            }
            let axis = if next_time[0] < next_time[1] {
                if next_time[0] < next_time[2] { 0 } else { 2 }
            } else if next_time[1] < next_time[2] {
                1
            } else {
                2
            };
            let passed = next_time[axis];
            next_time[axis] += time_step[axis];
            cell[axis] += direction[axis];
            if passed > max {
                return None;
            }
        }
    }

    fn raycast_smooth(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        ignore_water: bool,
    ) -> Option<TerrainRayHit> {
        if !origin.is_finite() || !dir.is_finite() {
            return None;
        }
        let max = max.min(MC_SEARCH_RAY_MAX);
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(dir));
        let end = origin + dir * max;
        let lo = origin.min(end);
        let hi = origin.max(end);
        let chunk_world = (SMOOTH_CHUNK as f32) * CELL_SIZE;
        let cmin = IVec3::new(
            (lo.x / chunk_world).floor() as i32,
            (lo.y / chunk_world).floor() as i32,
            (lo.z / chunk_world).floor() as i32,
        ) - IVec3::ONE;
        let cmax = IVec3::new(
            (hi.x / chunk_world).floor() as i32,
            (hi.y / chunk_world).floor() as i32,
            (hi.z / chunk_world).floor() as i32,
        ) + IVec3::ONE;
        let mut ids: Vec<IVec3> = if (cmax - cmin).as_i64vec3().element_product() > 4096 {
            self.partition
                .keys()
                .copied()
                .filter(|c| c.cmpge(cmin).all() && c.cmple(cmax).all())
                .collect()
        } else {
            let mut v = Vec::new();
            for y in cmin.y..=cmax.y {
                for z in cmin.z..=cmax.z {
                    for x in cmin.x..=cmax.x {
                        let id = IVec3::new(x, y, z);
                        if self.partition.contains_key(&id) {
                            v.push(id);
                        }
                    }
                }
            }
            v
        };
        ids.sort_by_key(|c| (c.y, c.z, c.x));
        let mut best: Option<TerrainRayHit> = None;
        for id in ids {
            let bmin =
                (id << SMOOTH_CHUNK_LOG2).as_vec3() * CELL_SIZE - Vec3::splat(CELL_SIZE * 2.0);
            let bmax = bmin + Vec3::splat(chunk_world + CELL_SIZE * 4.0);
            let limit = best.map_or(max, |b| b.distance);
            let aabb = Aabb::new(to_parry_vec(bmin), to_parry_vec(bmax));
            if aabb.cast_local_ray(&ray, limit, true).is_none() {
                continue;
            }
            let Some(m) = self.mesh(id) else {
                continue;
            };
            if let Some(shape) = &m.shape
                && let Some(hit) =
                    shape.cast_ray_and_get_normal(&Pose3::IDENTITY, &ray, limit, true)
            {
                let tri = match hit.feature {
                    parry3d::shape::FeatureId::Face(f) => (f as usize) % m.materials.len().max(1),
                    _ => 0,
                };
                let mat = m
                    .materials
                    .get(tri)
                    .copied()
                    .unwrap_or(conversion::MATERIAL_DEFAULT);
                best = Some(TerrainRayHit {
                    distance: hit.time_of_impact,
                    position: origin + dir * hit.time_of_impact,
                    normal: from_parry_vec(hit.normal),
                    material: conversion::material_from_voxel(mat),
                });
            }
            let limit = best.map_or(max, |b| b.distance);
            if !ignore_water
                && let Some(water) = &m.water
                && let Some(hit) =
                    water.cast_ray_and_get_normal(&Pose3::IDENTITY, &ray, limit, true)
                && best.is_none_or(|b| hit.time_of_impact < b.distance)
            {
                best = Some(TerrainRayHit {
                    distance: hit.time_of_impact,
                    position: origin + dir * hit.time_of_impact,
                    normal: from_parry_vec(hit.normal),
                    material: conversion::WATER,
                });
            }
        }
        best
    }
}

fn weighted(wx: f32, x: f32, wy: f32, y: f32) -> f32 {
    let den = wx + wy;
    if den <= 0.0 {
        return (x + y) * 0.5;
    }
    (wx * x + wy * y) / den
}

pub struct TerrainContactParams {
    pub friction: f32,
    pub restitution: f32,
}

impl PhysicsWorld {
    pub(crate) fn terrain_params(
        &self,
        part: usize,
        material: Option<u32>,
    ) -> TerrainContactParams {
        let p = &self.parts[part];
        match (self.mode, material) {
            (PhysicalMode::Legacy, _) => TerrainContactParams {
                friction: crate::step::legacy_friction(p.legacy_friction, TERRAIN_FRICTION),
                restitution: p.legacy_elasticity.min(TERRAIN_ELASTICITY),
            },
            (PhysicalMode::New, m) => {
                let t = rbx_math::materials::material_physics(m.unwrap_or(conversion::PLASTIC));
                TerrainContactParams {
                    friction: weighted(
                        p.friction_weight,
                        p.friction,
                        t.friction_weight,
                        t.friction,
                    ),
                    restitution: weighted(
                        p.elasticity_weight,
                        p.elasticity,
                        t.elasticity_weight,
                        t.elasticity,
                    ),
                }
            }
        }
    }

    pub(crate) fn terrain_narrowphase(&mut self) -> Vec<(InstanceId, IVec3)> {
        let mut active = Vec::new();
        if !self.terrain.is_active() {
            return active;
        }
        let Some(terrain_id) = self.terrain.id else {
            return active;
        };
        let long_step = (self.step_count / crate::world::WORLD_STEPS_PER_LONG_STEP as u64) as i64;
        let step = self.step_count;
        let dispatcher = DefaultQueryDispatcher;
        let is_smooth = self.terrain.smooth.is_some();
        let mut candidates: Vec<usize> = Vec::new();
        for (ai, a) in self.assemblies.iter().enumerate() {
            if a.anchored || a.sleeping || a.kinematic {
                continue;
            }
            for &pi in &a.parts {
                candidates.push(pi);
            }
            let _ = ai;
        }
        candidates.sort_by_key(|&i| self.parts[i].id);
        let mut overlapping_parts: HashSet<InstanceId> = HashSet::new();
        let mut water_parts: HashSet<InstanceId> = HashSet::new();
        let mut checked_parts: HashSet<InstanceId> = HashSet::new();
        for pi in candidates {
            let part = &self.parts[pi];
            let pid = part.id;
            checked_parts.insert(pid);
            let pose_a = to_pose(&(part.cframe * part.shape.local));
            let mins = from_parry_vec(part.aabb.mins);
            let maxs = from_parry_vec(part.aabb.maxs);
            let ext = legacy::EXTENT_RADIUS as f32 * CELL_SIZE;
            let mins = mins.max(Vec3::splat(-ext));
            let maxs = maxs.min(Vec3::splat(ext + CELL_SIZE));
            if mins.cmpgt(maxs).any() {
                continue;
            }
            let shape_a = part.shape.collider.clone();
            let mut features: Vec<(IVec3, SharedShape, Pose3)> = Vec::new();
            if is_smooth {
                let touches = self.terrain.chunks_touching(mins, maxs);
                for t in touches {
                    if t.water {
                        water_parts.insert(pid);
                    }
                    if t.solid
                        && let Some(m) = self.terrain.mesh(t.id)
                        && let Some(shape) = m.shape
                    {
                        features.push((t.id, shape, Pose3::IDENTITY));
                    }
                }
            } else if let Some(g) = &self.terrain.legacy {
                let lo = legacy::world_to_cell_floor(mins);
                let hi = legacy::world_to_cell_floor(maxs);
                let size = (hi - lo + IVec3::ONE).as_i64vec3();
                let push_cell = |cell: IVec3, features: &mut Vec<(IVec3, SharedShape, Pose3)>| {
                    let c = g.get_cell(cell);
                    if let Some(shape) = cell_shape(c) {
                        features.push((cell, shape.clone(), to_pose(&cell_frame(cell, c))));
                    }
                };
                if size.element_product() > LARGE_CONTACT_VOLUME {
                    for chunk in g.non_empty_chunks_in_region(lo, hi) {
                        let cmin = legacy::chunk_min(chunk).max(lo);
                        let cmax = legacy::chunk_max(chunk).min(hi);
                        for y in cmin.y..=cmax.y {
                            for z in cmin.z..=cmax.z {
                                for x in cmin.x..=cmax.x {
                                    push_cell(IVec3::new(x, y, z), &mut features);
                                }
                            }
                        }
                    }
                } else {
                    for y in lo.y..=hi.y {
                        for z in lo.z..=hi.z {
                            for x in lo.x..=hi.x {
                                push_cell(IVec3::new(x, y, z), &mut features);
                            }
                        }
                    }
                }
                let wlo = lo;
                let whi = hi;
                let wsize = whi - wlo + IVec3::ONE;
                let mut touching_water = false;
                let stride = |s: i32| if s > 1 { s - 1 } else { 1 };
                let mut y = wlo.y;
                'outer: while y <= whi.y {
                    let mut z = wlo.z;
                    while z <= whi.z {
                        let mut x = wlo.x;
                        while x <= whi.x {
                            if !legacy::is_empty(g.get_water_cell(IVec3::new(x, y, z))) {
                                touching_water = true;
                                break 'outer;
                            }
                            x += stride(wsize.x);
                        }
                        z += stride(wsize.z);
                    }
                    y += stride(wsize.y);
                }
                if !touching_water {
                    let mid = (wlo + whi) / 2;
                    let inside = (mid.x > wlo.x && mid.x < whi.x)
                        || (mid.y > wlo.y && mid.y < whi.y)
                        || (mid.z > wlo.z && mid.z < whi.z);
                    if inside && !legacy::is_empty(g.get_water_cell(mid)) {
                        touching_water = true;
                    }
                }
                if touching_water {
                    water_parts.insert(pid);
                }
            }
            let mut seen_features: HashSet<IVec3> = HashSet::new();
            let mut part_overlap = false;
            for (feature, shape_b, pose_b) in features {
                seen_features.insert(feature);
                let key = (pid, feature);
                let pos12 = pose_a.inv_mul(&pose_b);
                let pair = self
                    .terrain
                    .contacts
                    .entry(key)
                    .or_insert_with(|| ContactPair {
                        a: pid,
                        b: terrain_id,
                        manifolds: Vec::new(),
                        workspace: None,
                        collisions: Vec::new(),
                        last_long_step: -1,
                        overlapping: false,
                        seen: 0,
                    });
                pair.seen = step;
                let ok = dispatcher
                    .contact_manifolds(
                        &pos12,
                        shape_a.as_ref(),
                        shape_b.as_ref(),
                        0.0,
                        &mut pair.manifolds,
                        &mut pair.workspace,
                    )
                    .is_ok();
                let overlapping = ok
                    && pair
                        .manifolds
                        .iter()
                        .any(|m| m.points.iter().any(|c| c.dist <= 0.0));
                pair.overlapping = overlapping;
                if overlapping {
                    part_overlap = true;
                    if self.parts[pi].can_collide {
                        active.push(key);
                    }
                }
            }
            self.terrain
                .contacts
                .retain(|(p, f), _| *p != pid || seen_features.contains(f));
            if part_overlap {
                overlapping_parts.insert(pid);
            }
        }
        let mut touched = Vec::new();
        let mut ended = Vec::new();
        for pid in &overlapping_parts {
            if self.terrain.touching.insert(*pid, long_step).is_none() {
                touched.push((*pid, terrain_id));
            }
        }
        let stale: Vec<InstanceId> = self
            .terrain
            .touching
            .iter()
            .filter(|(p, last)| {
                !overlapping_parts.contains(*p)
                    && (checked_parts.contains(*p) || !self.index.contains_key(*p))
                    && **last < long_step
            })
            .map(|(p, _)| *p)
            .collect();
        for p in stale {
            self.terrain.touching.remove(&p);
            ended.push((p, terrain_id));
        }
        for p in self.terrain.water_parts.clone() {
            if checked_parts.contains(&p) && !water_parts.contains(&p) {
                self.terrain.water_parts.remove(&p);
            }
            if !self.index.contains_key(&p) {
                self.terrain.water_parts.remove(&p);
            }
        }
        self.terrain.water_parts.extend(water_parts);
        let index = &self.index;
        self.terrain
            .contacts
            .retain(|(p, _), _| index.contains_key(p));
        for t in touched {
            self.events_mut().touched.push(t);
        }
        for e in ended {
            self.events_mut().touch_ended.push(e);
        }
        active.sort_by_key(|(p, f)| (*p, f.x, f.y, f.z));
        active
    }

    pub fn raycast_terrain(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        ignore_water: bool,
    ) -> Option<TerrainRayHit> {
        if !self.terrain.is_active() {
            return None;
        }
        self.terrain.raycast(origin, dir, max, ignore_water)
    }
}
