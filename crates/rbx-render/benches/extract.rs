use criterion::{Criterion, criterion_group, criterion_main};
use rbx_dom::{DataModel, Variant};
use rbx_render::scene::{PartCache, extract_cached};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PLACES: &[(&str, &str)] = &[
    ("crossroads", "RobloxTest/PhysicsQuickTests/Collisions/CollisionPerformance/StandardParts/CrossroadsBroadPhaseTest.rbxl"),
    ("torture2500", "RobloxTest/PhysicsQuickTests/Collisions/CollisionPerformance/StandardParts/torture2500_v3.rbxl"),
];

fn place_path(rel: &str) -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(here)
        .join("tests")
        .join("places")
        .join(rel)
}

fn load(rel: &str) -> DataModel {
    let data = std::fs::read(place_path(rel)).expect("place file");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    dm
}

fn move_camera(dm: &mut DataModel, i: u32) {
    let Some(ws) = dm.find_service("Workspace") else { return };
    let Some(cam) = dm.get_ref(ws, "CurrentCamera") else { return };
    let cf = rbx_math::CFrame::from_translation(rbx_math::Vec3::new(i as f32 * 0.01, 20.0, 40.0));
    dm.store_silent(cam, "CFrame", Variant::CFrame(cf));
}

fn extract(c: &mut Criterion) {
    let mut group = c.benchmark_group("scene_extract");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));
    for (name, rel) in PLACES {
        let mut dm = load(rel);
        let mut i = 0;
        group.bench_function(format!("{name}_uncached"), |b| {
            b.iter(|| {
                i += 1;
                move_camera(&mut dm, i);
                extract_cached(&dm, 1.0, &mut PartCache::default())
            })
        });
        let mut cache = PartCache::default();
        group.bench_function(format!("{name}_cached"), |b| {
            b.iter(|| {
                i += 1;
                move_camera(&mut dm, i);
                extract_cached(&dm, 1.0, &mut cache)
            })
        });
    }
    group.finish();
}

criterion_group!(benches, extract);
criterion_main!(benches);
