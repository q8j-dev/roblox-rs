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
