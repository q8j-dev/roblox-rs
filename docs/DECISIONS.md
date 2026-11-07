# Decisions

Decisions made during the port that `docs/GOAL.md` doesn't cover.

| # | Decision | Reason |
|---|---|---|
| 1 | Rust 1.99.1 pinned in `rust-toolchain.toml` | latest stable at the start of the port |
| 2 | The web build uses `RUSTC_BOOTSTRAP=1` with `-Z build-std` instead of a separate nightly toolchain | wasm threads need std rebuilt with atomics; this keeps one pinned compiler |
| 3 | CI checks out the repo without `legacy/` | saves Actions minutes and disk while the C++ still exists |
| 4 | Docker runs through colima on this Mac | that's the installed Docker setup |
