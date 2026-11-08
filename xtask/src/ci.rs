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

pub const WEB_RUSTFLAGS: &[&str] = &[
    "-Ctarget-feature=+atomics,+bulk-memory,+mutable-globals",
    "-Clink-arg=--shared-memory",
    "-Clink-arg=--max-memory=4294967296",
    "-Clink-arg=--import-memory",
    "-Clink-arg=--export=__wasm_init_tls",
    "-Clink-arg=--export=__tls_size",
    "-Clink-arg=--export=__tls_align",
    "-Clink-arg=--export=__tls_base",
];

pub fn build_web(root: &Path, release: bool) -> Result<(), String> {
    let sdk = crate::wasi::ensure_sdk(root)?;
    let mut args = vec![
        "build",
        "-p",
        "player",
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
        "-Z",
        "build-std=std,panic_abort",
    ];
    if release {
        args.push("--release");
    }
    let mut flags: Vec<String> = WEB_RUSTFLAGS.iter().map(|s| s.to_string()).collect();
    flags.extend(crate::wasi::link_flags(&sdk));
    let encoded = flags.join("\u{1f}");
    let mut env: Vec<(String, String)> = crate::wasi::env(&sdk);
    env.push(("CARGO_ENCODED_RUSTFLAGS".into(), encoded));
    env.push(("RUSTC_BOOTSTRAP".into(), "1".into()));
    env.push(("CARGO_TARGET_DIR".into(), "target/web".into()));
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    step_env("web build", root, "cargo", &args, &env_refs)?;
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
    write_content_index(root)?;
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

pub const WEB_CONTENT_DIRS: &[&str] = &[
    "assets/content",
    "assets/platform-content/pc",
    "tests/places",
];

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

pub fn write_content_index(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for d in WEB_CONTENT_DIRS {
        collect_files(root, &root.join(d), &mut files);
    }
    files.sort();
    let out = root.join("dist/web");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(out.join("content-index.txt"), files.join("\n")).map_err(|e| e.to_string())
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
