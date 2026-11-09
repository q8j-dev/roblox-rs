# Performance

Every optimization with before and after numbers from the criterion benchmarks, measured on an Apple M4 in release mode. `cargo bench -p rbx-physics --bench step` steps each place 240 times (one second of simulation); `cargo bench -p rbx-render --bench extract` builds the render scene for one frame while the camera moves.

| # | Area | Change | Before | After |
|---|---|---|---|---|
| 1 | `rbx-render` scene extraction | Each part looked for a Humanoid among its parent's children every frame, which for parts sitting in Workspace meant scanning every Workspace child per part; the lookup is now cached per parent for the frame. The DataModel's instance, layout and behavior maps use FxHash instead of SipHash, and `is_a` reads a precomputed ancestor list instead of walking superclasses with string hashing | Crossroads 6.01 ms, torture2500 11.65 ms per frame | Crossroads 1.48 ms, torture2500 1.77 ms |
| 2 | `rbx-render` scene extraction | Part draws are cached between frames and rebuilt only when the part's, its parent's or its grandparent's revision counter changes (the DataModel bumps an instance and its parent on every property write, rename and reparent) | Crossroads 1.48 ms, torture2500 1.77 ms | Crossroads 139 µs, torture2500 135 µs |
| 3 | `rbx-physics` broadphase and maps | Every spatial query allocated a SipHash set to drop duplicates and looked each candidate up through SipHash; the query now de-duplicates by sorting its results and every map in the physics crate uses FxHash | AwakeStack6144 41.4 s, welded cluster 167 ms, Crossroads 2.15 s, 1050 blocks on terrain 1.28 s, 800 glue fall 1.07 s per 240 steps | 13.9 s, 49.1 ms, 1.49 s, 1.00 s, 791 ms |

With these changes a Studio frame on the ROBLOX World Headquarters place costs about 2.6 ms of UI, 0.3 ms of script step and 4.3 ms of GPU-inclusive rendering at 1080p (`cargo run -p rbx-render --release --example frame_bench`), down from a frame rate around 15 fps before change 1.
