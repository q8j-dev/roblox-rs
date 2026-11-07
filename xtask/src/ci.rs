use std::path::Path;
use std::process::Command;

fn step(name: &str, root: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    println!("\n==> {name}: {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("{name}: could not start {program}: {e}"))?;
    if status.success() {
        println!("<== {name}: ok");
        Ok(())
    } else {
        Err(format!("{name}: failed with {status}"))
    }
}

fn step_env(
    name: &str,
    root: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<(), String> {
    println!("\n==> {name}: {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .current_dir(root)
        .status()
        .map_err(|e| format!("{name}: could not start {program}: {e}"))?;
    if status.success() {
        println!("<== {name}: ok");
        Ok(())
    } else {
        Err(format!("{name}: failed with {status}"))
    }
}

pub const WEB_RUSTFLAGS: &str = "-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--max-memory=4294967296 -C link-arg=--import-memory -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base";

pub fn build_web(root: &Path, release: bool) -> Result<(), String> {
    let mut args = vec![
        "build",
        "-p",
        "player",
        "--target",
        "wasm32-unknown-unknown",
        "-Z",
        "build-std=std,panic_abort",
    ];
    if release {
        args.push("--release");
    }
    step_env(
        "web build",
        root,
        "cargo",
        &args,
        &[
            ("RUSTFLAGS", WEB_RUSTFLAGS),
            ("RUSTC_BOOTSTRAP", "1"),
            ("CARGO_TARGET_DIR", "target/web"),
        ],
    )?;
    let profile = if release { "release" } else { "debug" };
    let wasm = format!("target/web/wasm32-unknown-unknown/{profile}/player.wasm");
    step(
        "wasm-bindgen",
        root,
        "wasm-bindgen",
        &[
            &wasm,
            "--out-dir",
            "dist/web",
            "--target",
            "web",
            "--no-typescript",
        ],
    )?;
    let page = root.join("apps/player/web");
    if page.is_dir() {
        let out = root.join("dist/web");
        for entry in std::fs::read_dir(&page)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                std::fs::copy(entry.path(), out.join(entry.file_name()))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn run(root: &Path, options: &[String]) -> Result<(), String> {
    let has = |flag: &str| options.iter().any(|o| o == flag);
    let quick = has("--quick");

    step("fmt", root, "cargo", &["fmt", "--all", "--check"])?;
    step(
        "clippy",
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    step("test", root, "cargo", &["test", "--workspace"])?;

    if quick {
        return Ok(());
    }

    if !has("--skip-cross") {
        step(
            "build macos x86_64",
            root,
            "cargo",
            &["build", "--workspace", "--target", "x86_64-apple-darwin"],
        )?;
        step(
            "build linux x86_64",
            root,
            "cargo",
            &[
                "zigbuild",
                "--workspace",
                "--target",
                "x86_64-unknown-linux-gnu",
            ],
        )?;
        step(
            "build windows x86_64",
            root,
            "cargo",
            &[
                "xwin",
                "build",
                "--workspace",
                "--target",
                "x86_64-pc-windows-msvc",
            ],
        )?;
    }

    if !has("--skip-docker") {
        let mount = format!("{}:/src", root.display());
        step(
            "linux tests in docker",
            root,
            "docker",
            &[
                "run",
                "--rm",
                "-v",
                &mount,
                "-v",
                "rbx-cargo-registry:/usr/local/cargo/registry",
                "-v",
                "rbx-linux-target:/src/target",
                "-w",
                "/src",
                "rust:latest",
                "bash",
                "-c",
                "apt-get update -qq && apt-get install -y -qq libasound2-dev libudev-dev pkg-config > /dev/null && cargo test --workspace",
            ],
        )?;
    }

    if !has("--skip-web") {
        build_web(root, false)?;
        step(
            "web tests",
            root,
            "cargo",
            &["run", "-p", "xtask", "--", "web-test"],
        )
        .or_else(|e| {
            if root.join("apps/player/web").is_dir() {
                Err(e)
            } else {
                Ok(())
            }
        })?;
    }

    println!("\nci: all steps passed");
    Ok(())
}
