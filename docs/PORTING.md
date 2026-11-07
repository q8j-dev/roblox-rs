# Porting map

Status values: not started, in progress, ported, removed.

Read `docs/GOAL.md` and this file first after any context reset.

## Current phase

Phases 1 and 2 are done and tagged. Phase 3 (simulation) is mostly done with some test places still failing; phase 4 (server and networking) works end to end; phase 5 (rendering and player) and phase 6 (audio) are done. Phases 7 (web), 8 (Studio) and 9 (finish) are next.

## Legacy to Rust

| Legacy | Target | Status | Notes |
|---|---|---|---|
| `Base/` (RbxAssert, RbxFormat, HardwareInfo, cpucount, rbx/, util/, microprofile) | `rbx-base` | in progress | asserts become `debug_assert!` and `tracing`; microprofile becomes `tracing` spans |
| `Base/include/simd` | `rbx-math` | ported | glam handles SIMD |
| `App/v8kernel` (math kernel, bodies, connectors) | `rbx-physics` | done | constants, units, joint K table and mass properties ported; spring connectors replaced by PGS (decision 14) |
| `App/v8world` (World, Primitive, Assembly, Contact, Joints) | `rbx-physics` | in progress | assemblies, joints, auto-join, contacts, touch, sleep, fallen parts, movers done; humanoid stage and terrain contacts left |
| `App/solver` (PGS solver) | `rbx-physics` | done | `solver.rs` is a line-by-line port of Constraint, SolverKernel and Solver |
| `App.BulletPhysics/` | `rbx-physics` via parry3d | done | parry3d contact manifolds and ray casts |
| `App/v8tree` (Instance, Service, reflection glue) | `rbx-dom` | ported | |
| `App/reflection`, `Base/include/rbx` reflection | `rbx-reflection` | ported | |
| `App/v8datamodel` | `rbx-dom` (classes), `rbx-format` (serializers), `rbx-script` (bindings) | in progress | |
| `App/v8xml` | `rbx-format` | ported | XML place format |
| `App/lz4` | `rbx-format` via the lz4_flex crate | ported | binary place chunks |
| `App/script`, `App/Lua-5.1.4` | `rbx-script` (Luau via mlua) | in progress | scheduler, security identities, instance bindings, events, core scripts and most services done; remaining gaps show up as failing test places |
| `App/humanoid` | `rbx-physics` (controller) and `rbx-dom` (class) | in progress | state table, floor and ladder search, all state forces, death, MoveTo, events done; animation, tools and seats left |
| `App/tool` | `rbx-dom` and `apps/studio` | not started | dragger, mouse commands |
| `App/gui` | `rbx-dom` (layout), `rbx-render` (drawing), `rbx-script` (input) | ported | ScreenGui, BillboardGui, SurfaceGui, text, 9-slice, scrolling, tweens, button and TextBox input |
| `App/voxel`, `App/voxel2` | `rbx-terrain`, `rbx-physics`, `rbx-render` | ported | legacy and smooth terrain storage, contacts, meshing, water and light grid voxelization |
| `App/util` | `rbx-base`, `rbx-dom` | not started | |
| `App/security` | `rbx-script` | not started | keep script identity levels only |
| `CSG/` (sgCore) | `rbx-csg` | not started | |
| `Network/` (replicators, players, filtering) | `rbx-net`, `rbx-script/src/net*.rs` | in progress | replicator, basic and strict filters, remote events and functions, network ownership and physics replication done; stats items and streaming left |
| `Network/raknet` | `rbx-net` (quinn, WebTransport, WebSocket) | in progress | new wire format; QUIC over quinn with pinned self-signed certificates done, WebTransport comes with the web player |
| `Rendering/g3d`, `Rendering/RbxG3D` | `rbx-math` | in progress | |
| `Rendering/GfxCore` (D3D9, D3D11, GL) | `rbx-render` (wgpu) | ported | DX12, Metal, Vulkan and WebGPU through wgpu; 4x MSAA; offscreen capture on a software adapter |
| `Rendering/GfxRender` | `rbx-render` | ported | parts, meshes, decals, materials, terrain, water, light grid, shadow map, sky, particles, character compositing, adorns; per-light shadow masks are not drawn (decision 92) |
| `Rendering/GfxBase`, `Rendering/AppDraw` | `rbx-render` | ported | adorn renderer, DrawAdorn handles and selection, billboarders, text via rbx-text |
| `Rendering/ShaderCompiler`, `shaders/` | `rbx-render` WGSL | ported | default, smooth terrain, water, sky, particle, GUI, adorn, composite and shadow shaders |
| `ClientBase/`, `ClientShared/` | `rbx-base`, `apps/player` | not started | InfluxDB and analytics removed |
| `WindowsClient/`, `Mac/`, `RobloxMac/` | `apps/player`, `rbx-platform` | in progress | desktop player done (winit window, input, camera and control scripts, audio); web build is phase 7 |
| `RCCService/` | `apps/server` | in progress | SOAP operations are HTTP/JSON routes under `/v1`; jobs run in their own threads; local asset and content server |
| `Roblox.RccServiceArbiter/` | `apps/arbiter` | done | fronts the server API, keeps a warm process pool, routes each job to a process, recycles processes after their job, restarts processes that stop answering, sweeps expired jobs; the thread pool config and client settings fetch were tied to Roblox web services and are not ported |
| `RobloxStudio/` | `apps/studio`, `rbx-studio-ui` | not started | |
| `content/` | `assets/content` | in progress | moved; scripts not yet updated | |
| `PlatformContent/` | `assets/platform-content` | in progress | moved; scripts not yet updated | |
| `BuiltInPlugins/` | `assets/plugins/builtin` | in progress | moved; scripts not yet updated | |
| `StudioPlugins/` | `assets/plugins/studio` | in progress | moved; scripts not yet updated | |
| `RobloxTest/`, `App.UnitTest/`, `Base.UnitTest/`, `RCCService.Test/`, `RCCService.Thumb.Test/` | `tests/` | in progress | places and `.clienttest` files in `tests/places`; App.UnitTest Lua suites ported to `crates/rbx-script/tests` (LuaDebugger waits for Studio, network suites for phase 4) | |
| `fmod/` | `rbx-audio` (Kira) | ported | decisions 107 and 108 |
| `Win/` (video capture, DirectShow) | `apps/player` | not started | |

## Removed

| Legacy | Status | Notes |
|---|---|---|
| Network packet memory check, golden hash checker, VMProtect, client hooks, release patcher, junk code, fuzzy tokens | removed | files deleted from `legacy/` in phase 1. Do not port: the challenge and response items in the replicators, the hash fields in Players and Client, `CheatHandlingServerReplicator` hash logic, hash fast flags |
| `ScriptSigner/` | removed | |
| `XboxClient/`, `GameChat/`, `Microsoft.Xbox.Samples.NetworkMesh/` | removed | |
| `iOS/`, `Android/`, `RobloxHybrid/`, `RobloxMobileTest/` | removed | |
| `Rendering/LibOVR`, `Rendering/OpenVR`, `Rendering/VrApi` | removed | |
| `Installer/`, `*.PrepForUpload/`, `PrepAllForUpload/`, `RefreshPolicies/`, `SettingsComparisonTool/` | removed | |
| `QTitanRibbon/`, `Library/Qt` | removed | replaced by egui |
| `Library/` (boost, curl, cpp-netlib, w3c-libwww, xulrunner, Mesa, cabsdk, DSBaseClasses, glsl-optimizer, hlsl2glslfork, SDL2, zlib, SDK) | removed | replaced by crates |
| `boostlibs/`, `cmake/`, `PropertySheets/`, `IncludeChecker/`, `CoreScriptConverter2/`, `RobloxModelAnalyzer/`, `SimulationTestUtility/`, `RbxTestHooks/` | removed | build tooling and tools tied to the old toolchain |
