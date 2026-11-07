use crate::behavior::Behavior;
use crate::datamodel::DataModel;
use rbx_math::{Region3int16, Vector3int16};
use rbx_reflection::{InstanceId, Variant};
use rbx_terrain::{LegacyGrid, SmoothGrid};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct TerrainData {
    pub legacy: Option<LegacyGrid>,
    pub smooth: Option<SmoothGrid>,
    pub dirty: bool,
    pub errors: Vec<String>,
}

impl TerrainData {
    pub fn is_initialized(&self) -> bool {
        self.legacy.is_some() || self.smooth.is_some()
    }

    pub fn is_smooth(&self) -> bool {
        self.smooth.is_some()
    }

    pub fn non_empty_cell_count(&self) -> u32 {
        if let Some(s) = &self.smooth {
            return s.non_empty_cell_count_approx();
        }
        self.legacy.as_ref().map_or(0, |g| g.non_empty_cell_count())
    }
}

fn bytes(v: &Variant) -> Vec<u8> {
    match v {
        Variant::BinaryString(b) => b.clone(),
        Variant::String(s) | Variant::ProtectedString(s) | Variant::Content(s) => {
            s.as_bytes().to_vec()
        }
        _ => Vec::new(),
    }
}

impl DataModel {
    pub fn terrain(&self, id: InstanceId) -> Option<&TerrainData> {
        self.terrain_data.get(&id)
    }

    pub fn terrain_mut(&mut self, id: InstanceId) -> &mut TerrainData {
        self.terrain_data.entry(id).or_default()
    }

    pub fn workspace_terrain(&self) -> Option<InstanceId> {
        let ws = self.find_service("Workspace")?;
        self.children_of(ws)
            .iter()
            .copied()
            .find(|c| self.class_of(*c) == Some("Terrain"))
    }

    pub fn terrain_ids(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.terrain_data.keys().copied()
    }

    pub(crate) fn terrain_serialized(&self, id: InstanceId, prop: &str) -> Option<Variant> {
        let t = self.terrain_data.get(&id)?;
        if !t.dirty {
            return None;
        }
        match prop {
            "ClusterGridV3" | "GridV3" => Some(Variant::BinaryString(
                t.legacy.as_ref().map(|g| g.encode_v3()).unwrap_or_default(),
            )),
            "SmoothGrid" => Some(Variant::BinaryString(
                t.smooth.as_ref().map(|g| g.serialize()).unwrap_or_default(),
            )),
            _ => None,
        }
    }
}

fn decode(dm: &mut DataModel, id: InstanceId, prop: &str) {
    let Some(raw) = dm.get_raw(id, prop).map(bytes) else {
        return;
    };
    if raw.is_empty() {
        return;
    }
    let t = dm.terrain_mut(id);
    let result = match prop {
        "ClusterGrid" | "ClusterGridV2" | "ClusterGridV3" | "GridV3" => {
            t.smooth = None;
            let mut g = LegacyGrid::new();
            let r = match prop {
                "ClusterGrid" => g.decode_v1(&raw),
                "ClusterGridV2" => g.decode_v2(&raw),
                _ => g.decode_v3(&raw),
            };
            t.legacy = Some(g);
            t.dirty = matches!(prop, "ClusterGrid" | "ClusterGridV2");
            r
        }
        "SmoothGrid" => {
            t.legacy = None;
            let mut g = SmoothGrid::new();
            let r = g.deserialize(&raw);
            t.smooth = Some(g);
            t.dirty = false;
            r
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        t.errors.push(e.to_string());
    }
}

pub struct TerrainBehavior;

impl Behavior for TerrainBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["IsSmooth", "MaxExtents"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "IsSmooth" => Some(Variant::Bool(dm.terrain(id).is_some_and(|t| t.is_smooth()))),
            "MaxExtents" => {
                let r = rbx_terrain::legacy::EXTENT_RADIUS as i16;
                Some(Variant::Region3int16(Region3int16 {
                    min: Vector3int16::new(-r, -r, -r),
                    max: Vector3int16::new(r, r, r),
                }))
            }
            "ClusterGridV3" | "SmoothGrid" => dm.terrain_serialized(id, prop),
            _ => None,
        }
    }

    fn changed(&self, dm: &mut DataModel, id: InstanceId, prop: &'static str) {
        if matches!(
            prop,
            "ClusterGrid" | "ClusterGridV2" | "ClusterGridV3" | "SmoothGrid"
        ) {
            decode(dm, id, prop);
        }
    }

    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        child: InstanceId,
        _old: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        if id == child && new_parent.is_some() {
            let smooth_replicate = dm
                .get_raw(id, "SmoothReplicate")
                .and_then(|v| v.as_i32())
                .unwrap_or(-1);
            let t = dm.terrain_mut(id);
            if !t.is_initialized() {
                if smooth_replicate == 1 {
                    t.smooth = Some(SmoothGrid::new());
                } else {
                    t.legacy = Some(LegacyGrid::new());
                }
            }
        }
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        dm.terrain_data.remove(&id);
    }
}

fn v3i16(v: Option<&Variant>) -> Vector3int16 {
    match v {
        Some(Variant::Vector3int16(v)) => *v,
        _ => Vector3int16::default(),
    }
}

pub struct TerrainRegionBehavior;

impl Behavior for TerrainRegionBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["IsSmooth", "SizeInCells"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "IsSmooth" => Some(Variant::Bool(dm.terrain(id).is_some_and(|t| t.is_smooth()))),
            "SizeInCells" => {
                let min = v3i16(dm.get_raw(id, "ExtentsMin"));
                let max = v3i16(dm.get_raw(id, "ExtentsMax"));
                Some(Variant::Vector3(rbx_math::Vec3::new(
                    (max.x as i32 - min.x as i32 + 1) as f32,
                    (max.y as i32 - min.y as i32 + 1) as f32,
                    (max.z as i32 - min.z as i32 + 1) as f32,
                )))
            }
            "GridV3" | "SmoothGrid" => dm.terrain_serialized(id, prop),
            _ => None,
        }
    }

    fn changed(&self, dm: &mut DataModel, id: InstanceId, prop: &'static str) {
        if matches!(prop, "GridV3" | "SmoothGrid") {
            decode(dm, id, prop);
        }
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        dm.terrain_data.remove(&id);
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Terrain", &TerrainBehavior);
    map.insert("TerrainRegion", &TerrainRegionBehavior);
}
