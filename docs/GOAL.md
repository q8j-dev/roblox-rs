# Goal: port the engine to Rust

Port this repository (a 2016 C++ game engine: player, Studio, game server, tools) to Rust and reorganize the whole repo. When you're done, the repo holds one Cargo workspace that builds and runs on Windows, Linux, macOS and the web. Strip the anti-cheat and anti-tamper code, replace Qt and FMOD, and fix bugs and slow paths you find along the way.

Work autonomously. Don't stop to ask questions. When a decision comes up that this file doesn't cover, make it, log it in `docs/DECISIONS.md` and keep going.

## Ground rules

- Use the latest stable Rust. Check the current release with `rustup` at the start, pin it in `rust-toolchain.toml` and use edition 2024. The one exception is the web threads build, which may pin its own toolchain if wasm atomics need it.
- Use the latest version of every crate as of the start. Look versions up on crates.io instead of going from memory.
- Port behavior, not syntax. The goal is idiomatic Rust that behaves the same as the C++. Don't run a transpiler and don't keep C++ structure that fights the borrow checker.
- No stubs. Leave no `todo!()` or `unimplemented!()`, no functions that return a default just so a test passes, and no feature quietly dropped. If something can't be ported, log it in `docs/DECISIONS.md` with the reason. Only the items listed under "Removed" below may be dropped.
- `unsafe` is allowed only in the `rbx-platform` and `rbx-script` crates. Every other crate starts with `#![forbid(unsafe_code)]`.
- Every crate passes `cargo clippy -- -D warnings` and `cargo fmt --check`.
- Third-party code comes in through crates.io or git dependencies. Vendored C or C++ is allowed only in `third_party/`, only as unmodified upstream source, and only when no crate works (for example, if Luau needs a custom wasm build). Log each case in `docs/DECISIONS.md`.
- Keep `docs/PORTING.md` current. It maps every legacy folder and module to its crate and status (not started, in progress, ported, removed). It's the resume point after a context reset, so read it and this file first whenever you lose track.
- **Getting stuck:** if something is blocked by a limit in a browser, an OS or a crate (an API Safari lacks, a crate that won't build for wasm), try at most three approaches. Then ship the best fallback, log the blocker in `docs/DECISIONS.md` and move on. Don't loop on the same problem.
- **Disk space:** the Mac has about 60 GB free. Use one shared `target/` folder. After a cross-compile target passes, delete its build output with `cargo clean --target <target>`. Keep at least 15 GB free at all times.

## GitHub

- The current `origin` remote points to someone else's public repo (`Artifaqt/ROBLOX2016`). Remove it before doing anything else, and never push there.
- Create a private repo with `gh repo create q8j-dev/roblox-rs --private --source . --remote origin`. Before the first push, confirm with `gh repo view q8j-dev/roblox-rs --json visibility` that it's private. If it isn't, don't push: make it private first. Never push this code to a public repo.
- Before any Rust work, add `/target`, `*.wasm` build output and `/dist` to `.gitignore`. Then push the existing history so there's a backup of the untouched C++.
- Commit and push to `main` as you go: after every milestone and every large passing change, and at least once per working session. Never commit build output, secrets or files over 100 MB.
- When a phase is done, tag it `phase-<n>` and push the tag.
- Add `.github/workflows/ci.yml`. It runs on `phase-*` tags and manual dispatch, on `ubuntu-latest` and `windows-latest`: fmt, clippy, the full test suite and the web build. Use `Swatinem/rust-cache` and cancel runs that a newer one replaces. It doesn't run on every push, because private repos have limited free Actions minutes.

## Layout

The whole repo gets reorganized. During the port the C++ sits in `legacy/`; when you're done the root looks like this:

```
.cargo/config.toml      per-target settings, including the wasm threads flags
.github/workflows/      CI
Cargo.toml              workspace
Cargo.lock
rust-toolchain.toml
README.md               what it is, how to build, run and test each app
LICENSE
apps/
  player/               desktop and web client; the browser page and JS glue live in player/web/
  studio/               editor
  server/               headless game server, admin HTTP API, local asset server
  arbiter/              starts, monitors and restarts server processes
crates/
  rbx-base/             logging (tracing), config and fast flags, task scheduler, profiling
  rbx-math/             replaces G3D, built on glam
  rbx-reflection/       class and property metadata that drives the API, serialization and Studio
  rbx-dom/              Instance tree, services, DataModel
  rbx-format/           .rbxl/.rbxm XML and binary, meshes, textures
  rbx-physics/          the engine's own solver, ported, with parry3d for collision
  rbx-script/           Luau VM and the API bindings
  rbx-net/              replication, transport and the wire format
  rbx-render/           wgpu renderer and WGSL shaders
  rbx-audio/            Kira-based sound
  rbx-csg/              solid modeling (unions and negates)
  rbx-platform/         windowing, input, file system, OS integration
  rbx-studio-ui/        Studio panels and widgets
assets/
  content/              from content/
  platform-content/     from PlatformContent/
  plugins/builtin/      from BuiltInPlugins/
  plugins/studio/       from StudioPlugins/
tests/
  places/               every .rbxl and .rbxm test place
  lua/                  Lua API and client tests
  physics/              physics tests and their expected results
  integration/          multiplayer, Studio save and reload, web smoke tests
docs/
  GOAL.md               this file, moved here in phase 1
  PORTING.md  DECISIONS.md  BUGS.md  PERF.md
xtask/                  build, test, audit and packaging commands (cargo xtask ...)
third_party/            only if needed (see Ground rules)
```

## Technology choices

| Area | Legacy | Replacement |
|---|---|---|
| Math | G3D | glam |
| Physics | in-house solver plus Bullet 2.82 | port the in-house solver for matching behavior; parry3d replaces Bullet's collision detection |
| Scripting | modified Lua 5.1.4 | Luau through mlua. It's Lua 5.1 compatible, sandboxed and maintained. It must also work on the web: if mlua's Luau doesn't build for `wasm32-unknown-unknown`, compile Luau for wasm another way. Don't drop scripting on the web. |
| Networking | RakNet 4.036 | QUIC through quinn on desktop, WebTransport for browsers, WebSocket fallback. New wire format, no compatibility with the old protocol. Keep the replication model (replicators, filtering, ownership). |
| Rendering | D3D9, D3D11, OpenGL/GLES, Mesa | wgpu: Vulkan, Metal, DX12 and WebGPU. Port every shader to WGSL. Thumbnails render headless through wgpu on a software adapter. |
| Windowing and input | Win32, Cocoa, SDL2 | winit, gilrs for gamepads, touch input on the web player |
| Audio | FMOD 1.07 | Kira for mixing, spatial audio and effects, Symphonia for decoding. Must work on the web. |
| Studio UI | Qt 4.8.5 plus QTitanRibbon | egui with egui_dock: docking panels, explorer tree, property grid, toolbox, output, command bar, a Luau script editor with syntax highlighting, and the 3D viewport as a wgpu render target. Port the Studio plugin API (toolbars, buttons, plugin GUIs) to it. Studio saves in both the binary and XML place formats. |
| Server control | gSOAP | HTTP/JSON admin API on axum |
| Arbiter | C# Windows service | Rust binary on tokio, cross-platform |
| Logging and analytics | FastLog, Google Analytics, InfluxDB, crash upload | tracing with local log files only. No telemetry. |
| Web endpoints | hardcoded roblox.com URLs | one configurable base URL, plus a local asset and content server inside `apps/server` for development |

## Scripts in the assets

The Lua files in `assets/` (core scripts, platform content, built-in and Studio plugins) are part of the port, not just data:

- 26 of them hardcode roblox.com URLs. Point them at the configurable base URL.
- Update every script that calls a removed or replaced API, so all of them run without errors on the new engine.
- Keep the script security levels. Core scripts and plugins keep their elevated access, and place scripts stay sandboxed.
- Place scripts get no file system or OS access, and HttpService is off unless the server config turns it on.

## Removed

Remove these completely from the code, build output, assets and config:

- **NetPmc**: `Network/NetPmc.cpp`, `network/NetPmc.h` and every use in the replicators, the ping job, `Replicator.RockyItem` and `WindowsClient/ReleasePatcher.cpp`
- **VMProtect**: every macro, marker and include (34 files in App and WindowsClient), `Library/VMProtect`, and every build setting that references it
- **The golden hash checker**: `ProgramMemoryChecker` on every platform, the golden hash fields and messages in `Players`, `Client`, `ClientReplicator` and `ServerReplicator`, the hash checks in `RCCServiceSoapServiceImpl.cpp`, and the related fast flags
- Everything that exists only to support those three, such as `WindowsClient/robloxHooks.cpp`, `functionHooks.cpp` and `ReleasePatcher.cpp`
- Script signing (`ScriptSigner` and signed join scripts). The client gets its launch settings as typed config or CLI arguments, never as a Lua script downloaded from a URL.
- Platforms: Xbox (`XboxClient`, `GameChat`, `Microsoft.Xbox.Samples.NetworkMesh`), iOS, Android, and the legacy VR SDKs (LibOVR, OpenVR, VrApi)
- Packaging and deployment: `Installer`, the bootstrappers, every `*.PrepForUpload` project, `RefreshPolicies`, `SettingsComparisonTool`
- All Visual Studio, Xcode and CMake files. Delete these together with `legacy/` at the end, not earlier: until then, they're the record of which source files and defines belong to which target.

Delete the first four items from `legacy/` in phase 1, so nothing gets ported by accident.

In place of the client-side checks, the server is authoritative. Filtering is on by default and can be turned off per place. The server validates movement, ownership and remote calls.

## Platforms

- **Desktop:** `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` and `x86_64-apple-darwin`. Cross-compile from this Mac with cargo-xwin for Windows and cargo-zigbuild for Linux. Docker is installed: run the Linux test suite in a container. Windows tests run in CI.
- **Web:** `wasm32-unknown-unknown` with wasm-bindgen and the WebGPU backend of wgpu. The player is required on the web; Studio on the web is a bonus.
  - **Threads:** wasm atomics, SharedArrayBuffer and web workers (wasm-bindgen-rayon or similar). The dev server sends the COOP/COEP headers. If the page isn't cross-origin isolated, fall back to one thread at runtime.
  - **JSPI:** use it only where it fixes a real blocking problem, such as synchronous asset or HTTP calls that scripts expect to block. Detect support at runtime and fall back to yielding the Luau coroutine.
  - Assets load asynchronously over HTTP from the configured base URL.

## Order of work

Each phase ends with something that builds and passes its tests, then gets committed, pushed and tagged.

1. **Setup.** Set up GitHub and push the untouched history (see "GitHub"). Move the C++ tree to `legacy/` with `git mv` so its history carries over, and move this file to `docs/GOAL.md`. Delete the anti-cheat items from `legacy/`. Create the workspace, `xtask`, toolchains, CI and `docs/PORTING.md`.
2. **Foundation.** rbx-base, rbx-math, rbx-reflection, rbx-dom, rbx-format. Every test place loads and saves back with no loss. The repo has 527 of them, in both XML and binary format.
3. **Simulation.** rbx-physics, rbx-csg, rbx-script and the Lua API bindings. Port the Lua API tests, the physics tests and the 48 `.clienttest` files, and make them pass.
4. **Server and networking.** rbx-net, `apps/server`, `apps/arbiter`. A headless server hosts a place, two headless clients join, and each one sees the other's changes replicate.
5. **Rendering and player.** rbx-render, rbx-platform, `apps/player` on desktop. The player renders the test places.
6. **Audio.** rbx-audio, with every Sound feature the legacy API exposes.
7. **Web.** The player runs in the browser with threads, WebGPU, audio and touch input.
8. **Studio.** `apps/studio` opens a place, edits it (select, move, scale, rotate, properties, explorer, scripts, play solo, plugins), saves, and reopens it with no changes lost.
9. **Finish.** Delete `legacy/`, run the profiling and optimization pass, write the README, run the final audit.

## Bugs and performance

- **Bugs:** when you find one in the legacy code (crashes, undefined behavior, races, leaks, wrong results), fix it in the port, add a regression test, and log it in `docs/BUGS.md`. A behavior change that old places depend on is a decision: log it in `docs/DECISIONS.md`.
- **Performance:** profile before optimizing. Use criterion benchmarks for physics stepping, replication encoding, place loading, script calls and frame time on the reference places. Record before and after numbers for every optimization in `docs/PERF.md`. Don't keep a change that doesn't measurably help.

## Definitions

- **Saves back with no loss:** loading a place, saving it and loading the result gives the same instance tree with the same property values. The files don't need to match byte for byte. A place that's corrupt or that the legacy engine couldn't load either is logged in `docs/DECISIONS.md` with the reason and left out. If more than 10 places end up excluded, that's a bug in the loader, not in the places.
- **The audit** (`cargo xtask audit`) searches every text file in the repo and skips binary files. It skips `docs/` and its own pattern list; everywhere else it needs zero hits for:
  - Whole words, any case: `NetPmc`, `VMProtect`, `ProgramMemoryChecker`, `goldhash`, `golden hash`, `golden_hash`, `QTitan`, `RakNet`, `gSOAP`, `ScriptSigner`
  - Exact case only: `Qt`, `Q_OBJECT`, `FMOD`. A lowercase `fmod` is fine, because Luau's `math.fmod` is a different thing.
  - `todo!(` and `unimplemented!(`

## Done when

All of the following are true, and your final message shows the evidence: the command output pasted in, and screenshots where noted.

1. `cargo xtask ci` passes. It runs fmt and clippy, the full test suite on this Mac, the Linux tests in Docker, builds all four desktop targets and the web target, and runs the web tests in a headless browser.
2. The GitHub Actions run for the final `phase-9` tag passed on both Linux and Windows (show `gh run view` output).
3. `cargo xtask audit` passes with zero hits.
4. No C, C++, Objective-C, C#, Visual Studio, Xcode or CMake files are left outside `third_party/`, and every file in `third_party/` has an entry in `docs/DECISIONS.md`.
5. Every test place loads, saves back without loss, and simulates 600 physics steps on the server with no panic.
6. The ported Lua API, physics and client tests all pass, and every script in `assets/` loads without errors.
7. The multiplayer test passes: one server, two clients, changes replicate both ways.
8. A screenshot shows the desktop player rendering a test place on this Mac.
9. A screenshot shows the web player rendering a test place in the browser over WebGPU, with threads on.
10. A screenshot shows Studio with a place open, and a test confirms that editing, saving and reopening loses nothing.
11. `docs/PORTING.md` shows every module as ported or removed. `docs/DECISIONS.md`, `docs/BUGS.md` and `docs/PERF.md` are filled in.
12. Everything is committed and pushed to the private `q8j-dev/roblox-rs` repo, and `git status` is clean.
