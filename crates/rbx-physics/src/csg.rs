use crate::math::to_parry_vec;
use crate::shape::{PartShape, ShapeKind};
use parry3d::shape::SharedShape;
use rbx_csg::{CsgMesh, PhysicsData, hash_key_of, md5_hex, physics};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::Vec3;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

type HullSet = Arc<Vec<Vec<Vec3>>>;

static DECOMPOSITIONS: LazyLock<Mutex<HashMap<String, HullSet>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn decompose(mesh: &CsgMesh, key: String) -> HullSet {
    if let Ok(cache) = DECOMPOSITIONS.lock()
        && let Some(h) = cache.get(&key)
    {
        return h.clone();
    }
    let verts: Vec<_> = mesh.positions().into_iter().map(to_parry_vec).collect();
    let tris: Vec<[u32; 3]> = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| t.iter().all(|i| (*i as usize) < verts.len()))
        .copied()
        .collect();
    let params = parry3d::transformation::vhacd::VHACDParameters {
        concavity: 0.05,
        max_convex_hulls: 64,
        ..Default::default()
    };
    let hulls: Vec<Vec<Vec3>> = if tris.is_empty() {
        Vec::new()
    } else {
        parry3d::transformation::vhacd::VHACD::decompose(&params, &verts, &tris, true)
            .compute_exact_convex_hulls(&verts, &tris)
            .into_iter()
            .map(|(pts, _)| pts.into_iter().map(crate::math::from_parry_vec).collect())
            .collect()
    };
    let set = Arc::new(hulls);
    if let Ok(mut cache) = DECOMPOSITIONS.lock() {
        cache.insert(key, set.clone());
    }
    set
}

fn bytes(v: Option<&Variant>) -> Vec<u8> {
    match v {
        Some(Variant::BinaryString(b)) => b.clone(),
        Some(Variant::String(s)) | Some(Variant::Content(s)) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

pub fn resolve(dm: &DataModel, data: Vec<u8>) -> Vec<u8> {
    let Some(key) = hash_key_of(&data) else {
        return data;
    };
    let key = String::from_utf8_lossy(key).into_owned();
    for service in ["CSGDictionaryService", "NonReplicatedCSGDictionaryService"] {
        let Some(s) = dm.find_service(service) else {
            continue;
        };
        for &c in dm.children_of(s) {
            if dm.class_of(c) != Some("BinaryStringValue") {
                continue;
            }
            let value = bytes(dm.get_raw(c, "Value"));
            if md5_hex(&value) == key {
                return value;
            }
        }
    }
    Vec::new()
}

pub fn union_scale(dm: &DataModel, id: InstanceId, size: Vec3) -> Vec3 {
    let initial = dm
        .get_raw(id, "InitialSize")
        .and_then(|v| v.as_vec3())
        .unwrap_or(size);
    let initial = Vec3::new(
        if initial.x != 0.0 { initial.x } else { size.x },
        if initial.y != 0.0 { initial.y } else { size.y },
        if initial.z != 0.0 { initial.z } else { size.z },
    );
    let v = size / initial;
    let symmetric = matches!(dm.get_raw(id, "FormFactor"), Some(Variant::Enum(0)));
    if symmetric {
        Vec3::splat(v.x.min(v.y).min(v.z))
    } else {
        v
    }
}

pub fn union_shape(dm: &DataModel, id: InstanceId, size: Vec3) -> PartShape {
    let h = size.max(Vec3::splat(0.01)) * 0.5;
    let block = || SharedShape::cuboid(h.x, h.y, h.z);
    let scale = union_scale(dm, id, size);
    let data = resolve(dm, bytes(dm.get_raw(id, "PhysicsData")));
    let collider = match physics::parse(&data) {
        PhysicsData::Block => block(),
        PhysicsData::Hulls(hulls) => {
            let parts: Vec<_> = hulls
                .iter()
                .filter_map(|hull| {
                    let pts: Vec<_> = hull
                        .vertices
                        .iter()
                        .map(|v| to_parry_vec((hull.rotation * *v + hull.translation) * scale))
                        .collect();
                    SharedShape::convex_hull(&pts).map(|s| (parry3d::math::Pose3::IDENTITY, s))
                })
                .collect();
            if parts.is_empty() {
                block()
            } else {
                SharedShape::compound(parts)
            }
        }
        PhysicsData::OutOfDate(_) => block(),
        PhysicsData::Missing => {
            let raw = resolve(dm, bytes(dm.get_raw(id, "MeshData")));
            let fidelity = match dm.get_raw(id, "CollisionFidelity") {
                Some(Variant::Enum(v)) => *v,
                _ => 0,
            };
            match CsgMesh::from_binary(&raw).filter(|m| m.is_not_empty()) {
                Some(_) if fidelity == 2 => block(),
                Some(m) if fidelity == 1 => {
                    let pts: Vec<_> = m
                        .positions()
                        .into_iter()
                        .map(|p| to_parry_vec(p * scale))
                        .collect();
                    SharedShape::convex_hull(&pts).unwrap_or_else(block)
                }
                Some(m) => {
                    let hulls = decompose(&m, md5_hex(&raw));
                    let parts: Vec<_> = hulls
                        .iter()
                        .filter_map(|h| {
                            let pts: Vec<_> = h.iter().map(|p| to_parry_vec(*p * scale)).collect();
                            SharedShape::convex_hull(&pts)
                                .map(|s| (parry3d::math::Pose3::IDENTITY, s))
                        })
                        .collect();
                    if parts.is_empty() {
                        block()
                    } else {
                        SharedShape::compound(parts)
                    }
                }
                None => block(),
            }
        }
    };
    PartShape::hollow_box(ShapeKind::Compound, size, collider)
}
