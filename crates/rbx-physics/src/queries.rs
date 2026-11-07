use crate::math::{from_parry_vec, to_parry_vec, to_pose};
use crate::world::PhysicsWorld;
use parry3d::query::{Ray, RayCast, contact};
use rbx_dom::InstanceId;
use rbx_math::Vec3;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub part: InstanceId,
    pub position: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

impl PhysicsWorld {
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        filter: impl Fn(InstanceId) -> bool,
    ) -> Option<RayHit> {
        let len = direction.length();
        if len <= 0.0 || !len.is_finite() {
            return None;
        }
        let dir = direction / len;
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(dir));
        let mut best: Option<RayHit> = None;
        for p in &self.parts {
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
        let mut ordered: Vec<&crate::world::Part> = self.parts.iter().collect();
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
        for (j, o) in self.parts.iter().enumerate() {
            if j == pi {
                continue;
            }
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
