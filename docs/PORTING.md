# Porting map

Status values: not started, in progress, ported, removed.

Read `docs/GOAL.md` and this file first after any context reset.

## Current phase

Phase 1 (setup).

## Legacy to Rust

| Legacy | Target | Status | Notes |
|---|---|---|---|
| `Base/` (RbxAssert, RbxFormat, HardwareInfo, cpucount, rbx/, util/, microprofile) | `rbx-base` | not started | asserts become `debug_assert!` and `tracing`; microprofile becomes `tracing` spans |
| `Base/include/simd` | `rbx-math` | not started | glam handles SIMD |
| `App/v8kernel` (math kernel, bodies, connectors) | `rbx-physics` | not started | |
| `App/v8world` (World, Primitive, Assembly, Contact, Joints) | `rbx-physics` | not started | |
| `App/solver` (PGS solver) | `rbx-physics` | not started | |
| `App.BulletPhysics/` | `rbx-physics` via parry3d | not started | only collision detection is used |
| `App/v8tree` (Instance, Service, reflection glue) | `rbx-dom` | not started | |
| `App/reflection`, `Base/include/rbx` reflection | `rbx-reflection` | not started | |
| `App/v8datamodel` | `rbx-dom` (classes), `rbx-format` (serializers), `rbx-script` (bindings) | not started | |
| `App/v8xml` | `rbx-format` | not started | XML place format |
| `App/lz4` | `rbx-format` via the lz4_flex crate | not started | binary place chunks |
| `App/script`, `App/Lua-5.1.4` | `rbx-script` (Luau via mlua) | not started | |
| `App/humanoid` | `rbx-physics` (controller) and `rbx-dom` (class) | not started | |
| `App/tool` | `rbx-dom` and `apps/studio` | not started | dragger, mouse commands |
| `App/gui` | `rbx-dom` and `rbx-render` | not started | ScreenGui and friends |
| `App/voxel`, `App/voxel2` | `rbx-physics` and `rbx-render` | not started | terrain |
| `App/util` | `rbx-base`, `rbx-dom` | not started | |
| `App/security` | `rbx-script` | not started | keep script identity levels only |
| `CSG/` (sgCore) | `rbx-csg` | not started | |
| `Network/` (replicators, players, filtering) | `rbx-net` | not started | |
| `Network/raknet` | `rbx-net` (quinn, WebTransport, WebSocket) | not started | new wire format |
| `Rendering/g3d`, `Rendering/RbxG3D` | `rbx-math` | not started | |
| `Rendering/GfxCore` (D3D9, D3D11, GL) | `rbx-render` (wgpu) | not started | |
| `Rendering/GfxRender` | `rbx-render` | not started | freetype becomes a Rust font crate |
| `Rendering/GfxBase`, `Rendering/AppDraw` | `rbx-render` | not started | |
| `Rendering/ShaderCompiler`, `shaders/` | `rbx-render` WGSL | not started | |
| `ClientBase/`, `ClientShared/` | `rbx-base`, `apps/player` | not started | InfluxDB and analytics removed |
| `WindowsClient/`, `Mac/`, `RobloxMac/` | `apps/player`, `rbx-platform` | not started | |
| `RCCService/` | `apps/server` | not started | SOAP becomes an HTTP/JSON API |
| `Roblox.RccServiceArbiter/` | `apps/arbiter` | not started | |
| `RobloxStudio/` | `apps/studio`, `rbx-studio-ui` | not started | |
| `content/` | `assets/content` | not started | |
| `PlatformContent/` | `assets/platform-content` | not started | |
| `BuiltInPlugins/` | `assets/plugins/builtin` | not started | |
| `StudioPlugins/` | `assets/plugins/studio` | not started | |
| `RobloxTest/`, `App.UnitTest/`, `Base.UnitTest/`, `RCCService.Test/`, `RCCService.Thumb.Test/` | `tests/` | not started | |
| `fmod/` | `rbx-audio` (Kira) | not started | |
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
