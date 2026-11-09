use rbx_csg::CsgMesh;
use rbx_csg::ops::{self, Primitive, Solid};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};

pub const UNION_FAILED: &str = "This union is not currently solvable.  Try offseting the parts from each other or try unioning each part individually.";

fn color_of(dm: &DataModel, id: InstanceId) -> [u8; 3] {
    let c = match dm.get(id, "BrickColor") {
        Ok(Variant::BrickColor(b)) => b.color3(),
        _ => rbx_math::Color3::new(0.64, 0.64, 0.65),
    };
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [to(c.r), to(c.g), to(c.b)]
}

fn cframe_of(dm: &DataModel, id: InstanceId) -> CFrame {
    dm.get_raw(id, "CFrame")
        .and_then(|v| v.as_cframe())
        .unwrap_or_default()
}

fn size_of(dm: &DataModel, id: InstanceId) -> Vec3 {
    dm.get(id, "Size")
        .ok()
        .and_then(|v| v.as_vec3())
        .unwrap_or(Vec3::ONE)
}

fn mesh_of(dm: &DataModel, id: InstanceId) -> Option<CsgMesh> {
    let raw = rbx_physics::csg::resolve(
        dm,
        match dm.get_raw(id, "MeshData") {
            Some(Variant::BinaryString(b)) => b.clone(),
            Some(Variant::String(s)) => s.as_bytes().to_vec(),
            _ => Vec::new(),
        },
    );
    CsgMesh::from_binary(&raw).filter(|m| m.is_not_empty())
}

fn operation_scale(dm: &DataModel, id: InstanceId) -> Vec3 {
    let size = size_of(dm, id);
    let initial = dm
        .get_raw(id, "InitialSize")
        .and_then(|v| v.as_vec3())
        .unwrap_or(size);
    let axis = |s: f32, i: f32| if i.abs() > 1e-6 { s / i } else { 1.0 };
    Vec3::new(
        axis(size.x, initial.x),
        axis(size.y, initial.y),
        axis(size.z, initial.z),
    )
}

pub fn solid_of(dm: &DataModel, id: InstanceId, cframe: CFrame) -> Option<Solid> {
    let size = size_of(dm, id);
    let color = color_of(dm, id);
    let kind = match dm.class_of(id)? {
        "WedgePart" => Primitive::Wedge,
        "CornerWedgePart" => Primitive::CornerWedge,
        _ if dm.is_a(id, "PartOperation") => {
            let mesh = mesh_of(dm, id)?;
            let tint = (dm.get_bool(id, "UsePartColor") || dm.is_a(id, "NegateOperation"))
                .then_some(color);
            return Some(ops::from_csg_mesh(
                &mesh,
                operation_scale(dm, id),
                cframe,
                tint,
            ));
        }
        _ if dm.is_a(id, "Part") => match dm.get(id, "Shape").ok().and_then(|v| v.as_enum()) {
            Some(0) => Primitive::Ball,
            Some(2) => Primitive::Cylinder,
            _ => Primitive::Block,
        },
        _ if dm.is_a(id, "BasePart") => Primitive::Block,
        _ => return None,
    };
    Some(ops::primitive(kind, size, cframe, color))
}

fn salt() -> [u8; 16] {
    let t = rbx_base::time::unix_seconds();
    let mut x = t.to_bits() ^ 0x9e37_79b9_7f4a_7c15;
    let mut out = [0u8; 16];
    for b in &mut out {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x & 0xff) as u8;
    }
    out
}

fn child_data(dm: &DataModel, ids: &[InstanceId]) -> Result<Vec<u8>, String> {
    let tree = dm.save_tree(ids, rbx_dom::SaveOptions::default());
    rbx_format::write_binary(&tree, rbx_format::BinaryWriteOptions::default())
        .map_err(|e| e.to_string())
}

fn copy_appearance(dm: &mut DataModel, from: InstanceId, to: InstanceId) {
    for prop in [
        "BrickColor",
        "Material",
        "Anchored",
        "CanCollide",
        "Transparency",
        "Reflectance",
        "Locked",
    ] {
        if let Ok(v) = dm.get(from, prop) {
            let _ = dm.set(to, prop, v);
        }
    }
}

fn common_parent(dm: &DataModel, ids: &[InstanceId]) -> Option<InstanceId> {
    let first = dm.parent_of(*ids.first()?)?;
    if ids.iter().all(|&i| dm.parent_of(i) == Some(first)) {
        Some(first)
    } else {
        dm.find_service("Workspace")
    }
}

fn collect_parts(dm: &DataModel, selection: &[InstanceId]) -> Vec<InstanceId> {
    let mut out = Vec::new();
    for &id in selection {
        if dm.is_a(id, "BasePart") && dm.class_of(id) != Some("Terrain") {
            out.push(id);
        } else {
            for d in dm.descendants(id) {
                if dm.is_a(d, "BasePart") && dm.class_of(d) != Some("Terrain") {
                    out.push(d);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn new_operation(
    dm: &mut DataModel,
    class: &str,
    mesh: &CsgMesh,
    center: CFrame,
    size: Vec3,
    sources: &[InstanceId],
) -> Result<InstanceId, String> {
    let data = child_data(dm, sources)?;
    let op = dm.create(class).map_err(|e| e.to_string())?;
    dm.set_name(
        op,
        if class == "NegateOperation" {
            "NegateOperation"
        } else {
            "Union"
        },
    );
    copy_appearance(dm, sources[0], op);
    dm.set_unchecked(op, "Size", Variant::Vector3(size));
    dm.set_unchecked(op, "InitialSize", Variant::Vector3(size));
    dm.set_unchecked(op, "CFrame", Variant::CFrame(center));
    dm.set_unchecked(
        op,
        "MeshData",
        Variant::BinaryString(mesh.to_binary(&salt())),
    );
    dm.set_unchecked(op, "ChildData", Variant::BinaryString(data));
    let _ = dm.set(op, "UsePartColor", Variant::Bool(false));
    Ok(op)
}

pub fn union(dm: &mut DataModel, selection: &[InstanceId]) -> Result<InstanceId, String> {
    let parts = collect_parts(dm, selection);
    let (negative, positive): (Vec<_>, Vec<_>) = parts
        .into_iter()
        .partition(|&p| dm.is_a(p, "NegateOperation"));
    if positive.is_empty() || positive.len() + negative.len() < 2 {
        return Err("Must select at least two parts to union".into());
    }
    let solids = |ids: &[InstanceId]| -> Result<Vec<Solid>, String> {
        ids.iter()
            .map(|&i| solid_of(dm, i, cframe_of(dm, i)).ok_or_else(|| UNION_FAILED.to_string()))
            .collect()
    };
    let mut acc: Option<Solid> = None;
    for s in solids(&positive)? {
        acc = Some(match acc {
            None => s,
            Some(a) => ops::union(&a, &s).map_err(|_| UNION_FAILED.to_string())?,
        });
    }
    let mut acc = acc.ok_or_else(|| UNION_FAILED.to_string())?;
    for s in solids(&negative)? {
        acc = ops::subtract(&acc, &s).map_err(|_| UNION_FAILED.to_string())?;
    }
    let sources: Vec<InstanceId> = positive.iter().chain(&negative).copied().collect();
    let parent = common_parent(dm, &sources);
    let Some((lo, hi)) = acc.bounds() else {
        return Err(UNION_FAILED.into());
    };
    let center = (lo + hi) * 0.5;
    let mesh = ops::to_csg_mesh(&acc, center);
    let op = new_operation(
        dm,
        "UnionOperation",
        &mesh,
        CFrame::new(Mat3::IDENTITY, center),
        (hi - lo).max(Vec3::splat(0.05)),
        &sources,
    )?;
    let _ = dm.set_parent(op, parent);
    for p in sources {
        let _ = dm.destroy(p);
    }
    Ok(op)
}

pub fn negate(dm: &mut DataModel, selection: &[InstanceId]) -> Result<Vec<InstanceId>, String> {
    let parts = collect_parts(dm, selection);
    if parts.is_empty() {
        return Err("Must select at least one part to negate".into());
    }
    let mut result = Vec::new();
    for p in parts {
        if dm.is_a(p, "NegateOperation") {
            result.extend(restore(dm, p)?);
            continue;
        }
        let cframe = cframe_of(dm, p);
        let size = size_of(dm, p);
        let Some(solid) = solid_of(dm, p, CFrame::IDENTITY) else {
            continue;
        };
        let mesh = ops::to_csg_mesh(&solid, Vec3::ZERO);
        let parent = dm.parent_of(p);
        let op = new_operation(dm, "NegateOperation", &mesh, cframe, size, &[p])?;
        let _ = dm.set_parent(op, parent);
        let _ = dm.destroy(p);
        result.push(op);
    }
    Ok(result)
}

fn restore(dm: &mut DataModel, op: InstanceId) -> Result<Vec<InstanceId>, String> {
    let data = match dm.get_raw(op, "ChildData") {
        Some(Variant::BinaryString(b)) if !b.is_empty() => b.clone(),
        _ => return Err("This part has no parts to separate".into()),
    };
    let parent = dm.parent_of(op);
    let (roots, _) = dm.load_model(&data).map_err(|e| e.to_string())?;
    for &r in &roots {
        let _ = dm.set_parent(r, parent);
    }
    let _ = dm.destroy(op);
    Ok(roots)
}

pub fn separate(dm: &mut DataModel, selection: &[InstanceId]) -> Result<Vec<InstanceId>, String> {
    let ops: Vec<InstanceId> = selection
        .iter()
        .copied()
        .filter(|&i| dm.is_a(i, "PartOperation"))
        .collect();
    if ops.is_empty() {
        return Err("Must select a union or negated part to separate".into());
    }
    let mut result = Vec::new();
    for op in ops {
        result.extend(restore(dm, op)?);
    }
    Ok(result)
}
