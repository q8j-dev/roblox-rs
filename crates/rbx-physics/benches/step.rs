use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use rbx_dom::DataModel;
use rbx_physics::PhysicsWorld;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PLACES: &[(&str, &str)] = &[
    ("awake_stack", "RobloxTest/PGS/AwakeStack6144.rbxl"),
    (
        "welded_cluster",
        "RobloxTest/PhysicsTests/Collisions/CollisionPerformance/ArbitrarilyComplexGeometryWeldedPartCluster.rbxl",
    ),
    (
        "crossroads",
        "RobloxTest/PhysicsTests/Collisions/CollisionPerformance/CrossroadsBroadPhaseTest.rbxl",
    ),
    (
        "terrain_blocks",
        "RobloxTest/PhysicsTests/Collisions/CollisionPerformance/Terrain/1050WeldedBlocksOntoTerrain.rbxl",
    ),
    (
        "glue_fall",
        "RobloxTest/PhysicsTests/General/800glueFall.rbxl",
    ),
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

fn load(path: &Path) -> (DataModel, PhysicsWorld) {
    let data = std::fs::read(path).expect("place file");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    let mut world = PhysicsWorld::new();
    world.attach(&mut dm);
    world.rebuild_assemblies();
    (dm, world)
}

fn step_places(c: &mut Criterion) {
    let mut group = c.benchmark_group("world_step_240");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    for (name, rel) in PLACES {
        let path = place_path(rel);
        group.bench_function(*name, |b| {
            b.iter_batched(
                || load(&path),
                |(dm, mut world)| {
                    for _ in 0..240 {
                        world.world_step(&dm);
                    }
                    (dm, world)
                },
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, step_places);
criterion_main!(benches);
