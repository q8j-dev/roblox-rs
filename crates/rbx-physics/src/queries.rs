use crate::math::{from_parry_vec, to_parry_vec, to_pose};
use crate::world::PhysicsWorld;
use parry3d::query::{Ray, RayCast, contact};
use rbx_dom::{DataModel, InstanceId};
use rbx_math::Vec3;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub part: InstanceId,
    pub position: Vec3,
    pub normal: Vec3,
    pub distance: f32,
    pub material: Option<u32>,
}

impl PhysicsWorld {
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        filter: impl Fn(InstanceId) -> bool,
    ) -> Option<RayHit> {
        self.raycast_ex(origin, direction, filter, true, false)
    }

    pub fn raycast_ex(
        &self,
        origin: Vec3,
        direction: Vec3,
        filter: impl Fn(InstanceId) -> bool,
        ignore_water: bool,
        cells_are_cubes: bool,
    ) -> Option<RayHit> {
        let len = direction.length();
        if len <= 0.0 || !len.is_finite() {
            return None;
        }
        let dir = direction / len;
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(dir));
        let mut best: Option<RayHit> = None;
        let mut ids = Vec::new();
        self.spatial.query_ray(origin, dir, len, &mut ids);
        ids.sort();
        for id in ids {
            let Some(&pi) = self.index.get(&id) else {
                continue;
            };
            let p = &self.parts[pi];
            if !filter(p.id) {
                continue;
            }
            let max = best.map_or(len, |b| b.distance);
            let pose = to_pose(&(p.cframe * p.shape.local));
            let aabb = p.shape.collider.compute_aabb(&pose);
            if aabb.cast_local_ray(&ray, max, true).is_none() {
                continue;
            }
            if let Some(hit) = p
                .shape
                .collider
                .cast_ray_and_get_normal(&pose, &ray, max, true)
                && hit.time_of_impact <= max
            {
                best = Some(RayHit {
                    part: p.id,
                    position: origin + dir * hit.time_of_impact,
                    normal: from_parry_vec(hit.normal),
                    distance: hit.time_of_impact,
                    material: None,
                });
            }
        }
        if let Some(tid) = self.terrain.id
            && filter(tid)
        {
            let max = best.map_or(len, |b| b.distance);
            if let Some((t, n, m)) =
                self.terrain
                    .raycast_opts(origin, dir, max, ignore_water, cells_are_cubes)
                && t <= max
            {
                best = Some(RayHit {
                    part: tid,
                    position: origin + dir * t,
                    normal: n,
                    distance: t,
                    material: Some(m),
                });
            }
        }
        best
    }

    pub fn parts_in_region(
        &self,
        min: Vec3,
        max: Vec3,
        filter: impl Fn(InstanceId) -> bool,
        limit: usize,
    ) -> Vec<InstanceId> {
        let mut out = Vec::new();
        let query = parry3d::bounding_volume::Aabb::new(to_parry_vec(min), to_parry_vec(max));
        let mut ordered: Vec<&crate::world::Part> = self
            .candidates(&query)
            .into_iter()
            .map(|i| &self.parts[i])
            .collect();
        ordered.sort_by_key(|p| p.id);
        for p in ordered {
            if out.len() >= limit {
                break;
            }
            let pose = to_pose(&(p.cframe * p.shape.local));
            let a = p.shape.collider.compute_aabb(&pose);
            let lo = from_parry_vec(a.mins);
            let hi = from_parry_vec(a.maxs);
            if lo.x <= max.x
                && hi.x >= min.x
                && lo.y <= max.y
                && hi.y >= min.y
                && lo.z <= max.z
                && hi.z >= min.z
                && filter(p.id)
            {
                out.push(p.id);
            }
        }
        out
    }

    pub fn touching_parts(&self, id: InstanceId) -> Vec<InstanceId> {
        let Some(&pi) = self.index.get(&id) else {
            return Vec::new();
        };
        let p = &self.parts[pi];
        let pose = to_pose(&(p.cframe * p.shape.local));
        let aabb = p.shape.collider.compute_aabb(&pose);
        let mut out = Vec::new();
        for j in self.candidates(&aabb) {
            if j == pi {
                continue;
            }
            let o = &self.parts[j];
            let opose = to_pose(&(o.cframe * o.shape.local));
            let oa = o.shape.collider.compute_aabb(&opose);
            if oa.mins.x > aabb.maxs.x
                || oa.maxs.x < aabb.mins.x
                || oa.mins.y > aabb.maxs.y
                || oa.maxs.y < aabb.mins.y
                || oa.mins.z > aabb.maxs.z
                || oa.maxs.z < aabb.mins.z
            {
                continue;
            }
            if let Ok(Some(c)) = contact(
                &pose,
                p.shape.collider.as_ref(),
                &opose,
                o.shape.collider.as_ref(),
                0.0,
            ) && c.dist < 0.0
            {
                out.push(o.id);
            }
        }
        out.sort();
        out
    }

    pub fn connected_parts(&self, id: InstanceId, recursive: bool) -> Vec<InstanceId> {
        if !self.index.contains_key(&id) {
            return Vec::new();
        }
        let mut seen = HashSet::new();
        let mut out = vec![id];
        seen.insert(id);
        let mut queue = VecDeque::from([id]);
        while let Some(cur) = queue.pop_front() {
            let mut next: Vec<InstanceId> = self
                .joints
                .values()
                .filter(|j| j.in_world)
                .filter_map(|j| {
                    if j.part0 == Some(cur) {
                        j.part1
                    } else if j.part1 == Some(cur) {
                        j.part0
                    } else {
                        None
                    }
                })
                .filter(|p| self.index.contains_key(p))
                .collect();
            next.sort();
            for n in next {
                if seen.insert(n) {
                    out.push(n);
                    if recursive {
                        queue.push_back(n);
                    }
                }
            }
            if !recursive {
                break;
            }
        }
        out
    }

    pub fn root_part(&self, id: InstanceId) -> Option<InstanceId> {
        let a = self.assembly_of(id)?;
        Some(self.parts[a.root].id)
    }

    pub fn assembly_mass(&self, id: InstanceId) -> f32 {
        self.assembly_of(id).map_or(0.0, |a| a.mass)
    }

    pub fn part_mass(&self, id: InstanceId) -> f32 {
        self.part(id).map_or(0.0, |p| p.mass)
    }

    pub fn is_grounded(&self, id: InstanceId) -> bool {
        self.assembly_of(id).is_some_and(|a| a.anchored)
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlastResult {
    pub hits: Vec<(InstanceId, f32)>,
}

impl PhysicsWorld {
    pub fn part_radius(&self, id: InstanceId) -> f32 {
        self.part(id).map_or(0.0, |p| p.shape.extents_radius())
    }

    fn parts_touching_sphere_extents(&self, center: Vec3, radius: f32) -> Vec<InstanceId> {
        let r = Vec3::splat(radius);
        let mut ids = self.parts_in_region(center - r, center + r, |_| true, usize::MAX);
        ids.sort();
        ids
    }

    pub fn apply_impulse(&mut self, id: InstanceId, impulse: Vec3, at: Vec3, angular: Vec3) {
        let Some(p) = self.part(id) else {
            return;
        };
        let a = p.assembly;
        let Some(asm) = self.assemblies.get_mut(a) else {
            return;
        };
        if asm.anchored || asm.mass <= 0.0 {
            return;
        }
        asm.sleeping = false;
        asm.history.wake();
        asm.linear += impulse / asm.mass;
        let inv = asm.inv_inertia_world();
        asm.angular += inv * ((at - asm.position).cross(impulse) + angular);
        asm.moved = true;
    }

    pub fn explosion(
        &mut self,
        dm: &mut DataModel,
        position: Vec3,
        blast_radius: f32,
        blast_pressure: f32,
        destroy_joint_percent: f32,
        shielded: impl Fn(&DataModel, InstanceId) -> bool,
    ) -> BlastResult {
        self.sync(dm);
        self.rebuild_assemblies();
        let mut result = BlastResult::default();
        if blast_radius <= 0.0 {
            return result;
        }
        let max_object = blast_radius * 2.0;
        let parts = self.parts_touching_sphere_extents(position, blast_radius);
        for &id in &parts {
            if self.part_radius(id) < max_object {
                let p = self.part(id).map_or(position, |p| p.cframe.translation);
                result.hits.push((id, (p - position).length()));
            }
        }
        if blast_pressure > 0.0 {
            let unjoin = if destroy_joint_percent < 1.0 && destroy_joint_percent > 0.0 {
                self.parts_touching_sphere_extents(position, blast_radius * destroy_joint_percent)
            } else {
                parts.clone()
            };
            if destroy_joint_percent > 0.0 {
                for &id in &unjoin {
                    if self.part(id).is_some()
                        && self.part_radius(id) < max_object
                        && !shielded(dm, id)
                    {
                        self.destroy_joints(dm, id);
                    }
                }
                self.sync(dm);
                self.rebuild_assemblies();
            }
            for &id in &parts {
                let Some(p) = self.part(id) else {
                    continue;
                };
                let radius = p.shape.extents_radius();
                if radius >= max_object {
                    continue;
                }
                let center = p.cframe.translation;
                let delta = center - position;
                let normal = if delta == Vec3::ZERO {
                    Vec3::Y
                } else {
                    delta.normalize()
                };
                let impulse = normal * blast_pressure * radius * radius * (1.0 / 4560.0);
                if let Some(character) = dm.parent_of(id)
                    && let Some(h) = dm.find_first_child_of_class(character, "Humanoid")
                    && let Some(sim) = self.humanoid_mut(h)
                {
                    sim.activate_physics = true;
                    sim.activate_impulse = impulse;
                }
                self.apply_impulse(id, impulse, center, impulse * 0.5 * radius);
            }
        }
        result
    }
}
