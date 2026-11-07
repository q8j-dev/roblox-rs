use rbx_dom::DataModel;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: terrain_bounds <place>");
    let data = std::fs::read(&path).expect("read place");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    let Some(t) = dm.workspace_terrain().and_then(|t| dm.terrain(t)) else {
        println!("no terrain");
        return;
    };
    if let Some(g) = &t.smooth {
        let regions = g.non_empty_regions();
        let mut lo = rbx_math::IVec3::splat(i32::MAX);
        let mut hi = rbx_math::IVec3::splat(i32::MIN);
        for r in &regions {
            lo = lo.min(r.begin);
            hi = hi.max(r.end);
        }
        let mut counts = [0u32; 256];
        for r in &regions {
            for y in r.begin.y..r.end.y {
                for z in r.begin.z..r.end.z {
                    for x in r.begin.x..r.end.x {
                        counts[g.get_cell(x, y, z).material() as usize] += 1;
                    }
                }
            }
        }
        println!("smooth chunks {} cells {:?}..{:?}", regions.len(), lo, hi);
        for (m, c) in counts.iter().enumerate() {
            if *c > 0 && m > 0 {
                println!("  material {m}: {c}");
            }
        }
    }
    if let Some(g) = &t.legacy {
        println!("legacy cells {}", g.non_empty_cell_count());
    }
}
