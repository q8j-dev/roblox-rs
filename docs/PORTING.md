# Porting map

Status values: not started, in progress, ported, removed.

Read `docs/GOAL.md` and this file first after any context reset.

## Current phase

Phases 1 to 8 are done. Phase 3's test-place run passes 242 of 281 places with tests (and runs all 217 places without tests); the remaining failures and their causes are listed in `docs/BUGS.md`. Phase 9 (finish) is in progress: the audit is clean outside `legacy/`, the optimization pass is recorded in `docs/PERF.md`, and `legacy/` is deleted last.

## Legacy to Rust

| Legacy | Target | Status | Notes |
|---|---|---|---|
| `Base/` (RbxAssert, RbxFormat, HardwareInfo, cpucount, rbx/, util/, microprofile) | `rbx-base` | ported | fast flags, logging, task scheduler, jobs, time and virtual file system in `rbx-base`; asserts are `debug_assert!`, microprofile is `tracing` spans |
| `Base/include/simd` | `rbx-math` | ported | glam handles SIMD |
| `App/v8kernel` (math kernel, bodies, connectors) | `rbx-physics` | ported | constants, units, joint K table and mass properties ported; spring connectors replaced by PGS (decision 14) |
| `App/v8world` (World, Primitive, Assembly, Contact, Joints) | `rbx-physics` | ported | assemblies, joints, auto-join, contacts, touch, sleep, fallen parts, movers, humanoid stage, buoyancy and terrain contacts |
| `App/solver` (PGS solver) | `rbx-physics` | ported | `solver.rs` is a line-by-line port of Constraint, SolverKernel and Solver |
| `App.BulletPhysics/` | `rbx-physics` via parry3d | ported | parry3d contact manifolds and ray casts |
| `App/v8tree` (Instance, Service, reflection glue) | `rbx-dom` | ported | |
| `App/reflection`, `Base/include/rbx` reflection | `rbx-reflection` | ported | |
| `App/v8datamodel` | `rbx-dom` (classes), `rbx-format` (serializers), `rbx-script` (bindings) | ported |  |
| `App/v8xml` | `rbx-format` | ported | XML place format |
| `App/lz4` | `rbx-format` via the lz4_flex crate | ported | binary place chunks |
| `App/script`, `App/Lua-5.1.4` | `rbx-script` (Luau via mlua) | ported | scheduler, security identities, instance bindings, events, core scripts and services; test places that still fail are listed in `docs/BUGS.md` |
| `App/humanoid` | `rbx-physics` (controller) and `rbx-dom` (class) | ported | state table, floor and ladder search, state forces, death, MoveTo, events, seats and tools; animation runs through the Animate scripts |
| `App/tool` | `rbx-dom` and `apps/studio` | ported | Tool instances in `rbx-dom`; Studio drag, scale and rotate tools in `apps/studio` |
| `App/gui` | `rbx-dom` (layout), `rbx-render` (drawing), `rbx-script` (input) | ported | ScreenGui, BillboardGui, SurfaceGui, text, 9-slice, scrolling, tweens, button and TextBox input |
| `App/voxel`, `App/voxel2` | `rbx-terrain`, `rbx-physics`, `rbx-render` | ported | legacy and smooth terrain storage, contacts, meshing, water and light grid voxelization |
| `App/util` | `rbx-base`, `rbx-dom` | ported | spread over `rbx-math` (BrickColor, Region3, UDim, Axes, Faces), `rbx-script` (content providers, HTTP, LuaWebService), `rbx-audio` (sounds) and `rbx-base` (thread pool, profiling); analytics and machine-id upload removed |
| `App/security` | `rbx-script` | ported | script identity levels in `rbx-script/src/security.rs` |
| `CSG/` (sgCore) | `rbx-csg` | ported | union and negate parts load, render from their CSG mesh (vertex colours, part colour, negate tint) and collide (physics data, decomposition); Union, Negate and Separate (Studio ribbon and the plugin API) use boolmesh instead of sgCore (decision 138) |
| `Network/` (replicators, players, filtering) | `rbx-net`, `rbx-script/src/net*.rs` | ported | replicator, basic and strict filters, remote events and functions, network ownership and physics replication; streaming is not ported |
| `Network/raknet` | `rbx-net` (quinn, WebTransport, WebSocket) | ported | new wire format over QUIC (quinn) with pinned self-signed certificates; WebSocket for the web player |
| `Rendering/g3d`, `Rendering/RbxG3D` | `rbx-math` | ported |  |
| `Rendering/GfxCore` (D3D9, D3D11, GL) | `rbx-render` (wgpu) | ported | DX12, Metal, Vulkan and WebGPU through wgpu; 4x MSAA; offscreen capture on a software adapter |
| `Rendering/GfxRender` | `rbx-render` | ported | parts, meshes, decals, materials, terrain, water, light grid, shadow map, sky, particles, character compositing, adorns; per-light shadow masks are not drawn (decision 92) |
| `Rendering/GfxBase`, `Rendering/AppDraw` | `rbx-render` | ported | adorn renderer, DrawAdorn handles and selection, billboarders, text via rbx-text |
| `Rendering/ShaderCompiler`, `shaders/` | `rbx-render` WGSL | ported | default, smooth terrain, water, sky, particle, GUI, adorn, composite and shadow shaders |
| `ClientBase/`, `ClientShared/` | `rbx-base`, `apps/player` | ported | `apps/player` and `rbx-base`; InfluxDB and analytics removed |
| `WindowsClient/`, `Mac/`, `RobloxMac/` | `apps/player`, `rbx-platform` | ported | desktop player (winit window, input, camera and control scripts, audio) and the web player (WebGPU, worker threads, Web Audio) |
| `RCCService/` | `apps/server` | ported | SOAP operations are HTTP/JSON routes under `/v1`; jobs run in their own threads; local asset and content server |
| `Roblox.RccServiceArbiter/` | `apps/arbiter` | ported | fronts the server API, keeps a warm process pool, routes each job to a process, recycles processes after their job, restarts processes that stop answering, sweeps expired jobs; the thread pool config and client settings fetch were tied to Roblox web services and are not ported |
| `RobloxStudio/` | `apps/studio`, `rbx-studio-ui` | ported | 2016 ribbon, docks, Explorer, Properties, Luau editor, select, move, scale and rotate tools, command bar, output, play solo and run, local test servers, plugin API, binary and XML save; Team Create, publishing and the debugger are left out (decision 132); Union, Negate and Separate are in (decision 138) |
| `content/` | `assets/content` | ported | `assets/content` |
| `PlatformContent/` | `assets/platform-content` | ported | `assets/platform-content` |
| `BuiltInPlugins/` | `assets/plugins/builtin` | ported | loaded by Studio with hidden toolbars (decision 133) |
| `StudioPlugins/` | `assets/plugins/studio` | ported | loaded by Studio through the plugin API in `rbx-script` |
| `RobloxTest/`, `App.UnitTest/`, `Base.UnitTest/`, `RCCService.Test/`, `RCCService.Thumb.Test/` | `tests/` | ported | places and `.clienttest` files in `tests/places`, run by `cargo run -p rbx-script --example run_places`; App.UnitTest Lua suites in `crates/rbx-script/tests` |
| `fmod/` | `rbx-audio` (Kira) | ported | decisions 107 and 108 |
| `Win/` (video capture, DirectShow) | `apps/player` | ported | the Record Video verb writes an MJPEG AVI to Videos/Roblox (Movies/Roblox on macOS) at 30 fps; no audio track (decision 137) |

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
