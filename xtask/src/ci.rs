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

fn build_web_variant(root: &Path, sdk: &Path, release: bool, threads: bool) -> Result<(), String> {
    let (name, c_target, target_dir) = if threads {
        ("mt", "wasm32-wasip1-threads", "target/web")
    } else {
        ("st", "wasm32-wasip1", "target/web-st")
    };
    let mut args = vec![
        "build",
        "-p",
        "player",
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
    ];
    if threads {
        args.extend(["-Z", "build-std=std,panic_abort"]);
    }
    if release {
        args.push("--release");
    }
    let mut flags: Vec<String> = if threads {
        WEB_RUSTFLAGS.iter().map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };
    flags.extend(crate::wasi::link_flags(sdk, c_target));
    let mut env: Vec<(String, String)> = crate::wasi::env(sdk, c_target, threads);
    env.push(("CARGO_ENCODED_RUSTFLAGS".into(), flags.join("\u{1f}")));
    if threads {
        env.push(("RUSTC_BOOTSTRAP".into(), "1".into()));
    }
    env.push(("CARGO_TARGET_DIR".into(), target_dir.into()));
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    step_env(
        &format!("web build ({name})"),
        root,
        "cargo",
        &args,
        &env_refs,
    )?;
    let profile = if release { "release" } else { "debug" };
    let wasm = format!("{target_dir}/wasm32-unknown-unknown/{profile}/player.wasm");
    let out = format!("dist/web/{name}");
    step(
        "wasm-bindgen",
        root,
        "wasm-bindgen",
        &[
            &wasm,
            "--out-dir",
            &out,
            "--target",
            "web",
            "--no-typescript",
        ],
    )?;
    fix_worker_imports(&root.join(&out).join("snippets"))
}

fn fix_worker_imports(dir: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            fix_worker_imports(&path)?;
        } else if path.extension().is_some_and(|e| e == "js") {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            if text.contains("import('../../..')") {
                let fixed = text.replace("import('../../..')", "import('../../../player.js')");
                std::fs::write(&path, fixed).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn build_web(root: &Path, release: bool) -> Result<(), String> {
    let sdk = crate::wasi::ensure_sdk(root)?;
    build_web_variant(root, &sdk, release, true)?;
    build_web_variant(root, &sdk, release, false)?;
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

fn clean_target(root: &Path, target: &str) -> Result<(), String> {
    step(
        &format!("clean {target}"),
        root,
        "cargo",
        &["clean", "--target", target],
    )
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
        clean_target(root, "x86_64-apple-darwin")?;
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
        clean_target(root, "x86_64-unknown-linux-gnu")?;
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
        clean_target(root, "x86_64-pc-windows-msvc")?;
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
