use rbx_dom::rbx_terrain::smooth::Region;
use rbx_dom::rbx_terrain::{LegacyGrid, SmoothGrid};
use rbx_dom::{DataModel, Variant};
use rbx_math::IVec3;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(here)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && matches!(ext.to_ascii_lowercase().as_str(), "rbxl" | "rbxlx")
        {
            out.push(path);
        }
    }
}

fn same_legacy(a: &LegacyGrid, b: &LegacyGrid) -> Result<(), String> {
    if a.non_empty_cell_count() != b.non_empty_cell_count() {
        return Err(format!(
            "cell count {} vs {}",
            a.non_empty_cell_count(),
            b.non_empty_cell_count()
        ));
    }
    if a.non_empty_chunks() != b.non_empty_chunks() {
        return Err("chunk sets differ".into());
    }
    for id in a.non_empty_chunks() {
        let min = rbx_dom::rbx_terrain::legacy::chunk_min(id);
        let max = rbx_dom::rbx_terrain::legacy::chunk_max(id);
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let p = IVec3::new(x, y, z);
                    if a.get_cell(p) != b.get_cell(p) || a.get_material(p) != b.get_material(p) {
                        return Err(format!("cell {p} differs"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn same_smooth(a: &SmoothGrid, b: &SmoothGrid) -> Result<(), String> {
    let ra = a.non_empty_regions();
    if ra != b.non_empty_regions() {
        return Err("region sets differ".into());
    }
    for r in ra {
        let ba = a.read(&r, 0);
        let bb = b.read(&r, 0);
        let s = r.size();
        for y in 0..s.y {
            for z in 0..s.z {
                for x in 0..s.x {
                    if ba.get(x, y, z) != bb.get(x, y, z) {
                        return Err(format!("cell {:?} differs", r.begin + IVec3::new(x, y, z)));
                    }
                }
            }
        }
    }
    if a.non_empty_cell_count_approx() != b.non_empty_cell_count_approx() {
        return Err("volume differs".into());
    }
    Ok(())
}

#[test]
fn terrain_round_trips() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["tests", "assets"] {
        collect(&root.join(dir), &mut files);
    }
    files.sort();
    let mut with_terrain = 0;
    let mut failures = Vec::new();
    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let mut dm = DataModel::new();
        if dm.load_place(&data).is_err() {
            continue;
        }
        let ids: Vec<_> = dm.terrain_ids().collect();
        for id in ids {
            let t = dm.terrain(id).expect("terrain");
            if !t.errors.is_empty() {
                failures.push(format!("{}: {:?}", f.display(), t.errors));
                continue;
            }
            if t.non_empty_cell_count() == 0 {
                continue;
            }
            with_terrain += 1;
            let r = if let Some(g) = &t.legacy {
                let mut back = LegacyGrid::new();
                back.decode_v3(&g.encode_v3())
                    .map_err(|e| e.to_string())
                    .and_then(|_| same_legacy(g, &back))
            } else if let Some(g) = &t.smooth {
                let mut back = SmoothGrid::new();
                back.deserialize(&g.serialize())
                    .map_err(|e| e.to_string())
                    .and_then(|_| same_smooth(g, &back))
            } else {
                Ok(())
            };
            if let Err(e) = r {
                failures.push(format!("{}: {e}", f.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(with_terrain > 0);
    eprintln!("places with terrain: {with_terrain}");
}

#[test]
fn smooth_write_and_read() {
    let mut g = SmoothGrid::new();
    let region = Region::new(IVec3::new(-3, 0, 5), IVec3::new(40, 3, 9));
    let mut b = rbx_dom::rbx_terrain::smooth::CellBox::sized(region.size());
    let s = region.size();
    for y in 0..s.y {
        for z in 0..s.z {
            for x in 0..s.x {
                b.set(
                    x,
                    y,
                    z,
                    rbx_dom::rbx_terrain::smooth::Cell::new(3, (x * 7 % 256) as u8),
                );
            }
        }
    }
    g.write(&region, &b);
    assert_eq!(g.get_cell(-3, 0, 5).material(), 3);
    assert_eq!(g.get_cell(39, 2, 8).occupancy(), (42 * 7 % 256) as u8);
    let mut back = SmoothGrid::new();
    back.deserialize(&g.serialize()).expect("decode");
    same_smooth(&g, &back).expect("same");
    let _ = Variant::Bool(true);
}
