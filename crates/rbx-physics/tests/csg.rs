use rbx_csg::{CsgMesh, PhysicsData, physics};
use rbx_dom::{DataModel, Variant};
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rbxl" | "rbxlx")
        ) {
            out.push(path);
        }
    }
}

fn bytes(v: Option<&Variant>) -> Vec<u8> {
    match v {
        Some(Variant::BinaryString(b)) => b.clone(),
        Some(Variant::String(s)) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

#[test]
fn every_union_decodes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/places");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let (mut unions, mut hulls, mut blocks, mut meshes, mut absent) = (0, 0, 0, 0, 0);
    let mut problems = Vec::new();
    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let mut dm = DataModel::new();
        if dm.load_place(&data).is_err() {
            continue;
        }
        let ids: Vec<_> = dm.ids().filter(|i| dm.is_a(*i, "PartOperation")).collect();
        for id in ids {
            unions += 1;
            let raw_mesh = bytes(dm.get_raw(id, "MeshData"));
            let phys = rbx_physics::csg::resolve(&dm, bytes(dm.get_raw(id, "PhysicsData")));
            let mesh = rbx_physics::csg::resolve(&dm, raw_mesh.clone());
            let physics_ok = match physics::parse(&phys) {
                PhysicsData::Hulls(_) => {
                    hulls += 1;
                    true
                }
                PhysicsData::Block => {
                    blocks += 1;
                    true
                }
                _ => false,
            };
            if mesh.is_empty() && rbx_csg::is_hash_key(&raw_mesh) {
                absent += 1;
                continue;
            }
            match CsgMesh::from_binary(&mesh) {
                Some(m) if m.is_not_empty() && !m.bad_mesh => meshes += 1,
                Some(m) => problems.push(format!(
                    "{} {}: mesh bad={} verts={}",
                    f.display(),
                    dm.full_name(id),
                    m.bad_mesh,
                    m.vertices.len()
                )),
                None if physics_ok => {}
                None => problems.push(format!(
                    "{} {}: no usable physics or mesh ({} bytes)",
                    f.display(),
                    dm.full_name(id),
                    mesh.len()
                )),
            }
        }
    }
    eprintln!("unions={unions} hulls={hulls} blocks={blocks} meshes={meshes} absent={absent}");
    assert!(unions > 0);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
