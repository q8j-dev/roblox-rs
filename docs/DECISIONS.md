# Decisions

Decisions made during the port that `docs/GOAL.md` doesn't cover.

| # | Decision | Reason |
|---|---|---|
| 1 | Rust 1.99.1 pinned in `rust-toolchain.toml` | latest stable at the start of the port |
| 2 | The web build uses `RUSTC_BOOTSTRAP=1` with `-Z build-std` instead of a separate nightly toolchain | wasm threads need std rebuilt with atomics; this keeps one pinned compiler |
| 3 | CI checks out the repo without `legacy/` | saves Actions minutes and disk while the C++ still exists |
| 4 | Docker runs through colima on this Mac | that's the installed Docker setup |
| 5 | The class, property, function, event and enum list in `crates/rbx-reflection/data/api.json` was generated once from the legacy C++ reflection declarations (descriptor templates, macros and enum definitions) and is checked in | the legacy tree gets deleted, and the data is what the engine's API is made of |
| 6 | `rbx-format` works on an untyped tree; `rbx-dom` converts values to the declared property types | the XML format stores BrickColor as `int` and the binary format stores every string kind the same way, so only the reflection data can tell them apart |
| 7 | XML saves write non-ASCII text as UTF-8 instead of one numeric entity per byte | both forms load the same; UTF-8 is readable and valid XML |
| 8 | `.gitattributes` marks place files, images, fonts and audio as binary | two R15 character models in the original repo were damaged by line-ending conversion |
| 9 | Default property values come from `crates/rbx-dom/data/defaults.json`, generated once from the legacy constructors by following each property getter to its member and initializer, plus a hand-written table in `defaults.rs` for values the generator couldn't resolve | about 370 defaults resolve automatically; the rest depend on physics primitives or helper functions |
| 10 | `rbx-dom` reaches physics through a `WorldHooks` trait that `rbx-physics` implements | part setters in the legacy code break and rebuild joints and nudge parts out of overlap, which needs the physics world, but the instance tree can't depend on physics |
| 11 | Instance events go into a log that each subsystem drains, instead of synchronous callbacks | Rust can't hand out a mutable instance tree to listeners in the middle of a change; scripts still get their event handlers run before control returns to them |
| 12 | `humanoidAnimateR15.rbxm` is left out of the place tests (listed in `tests/excluded_places.txt`) | it was already damaged in the original repository, so the legacy engine couldn't load it either |
| 13 | The service list in `rbx-dom` comes from the legacy classes that inherit the Service mixin | the reflection data doesn't record that mixin |
