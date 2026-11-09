use std::path::{Path, PathBuf};
use std::process::Command;

pub const WASI_SDK_VERSION: &str = "34";

fn host_archive() -> Result<String, String> {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => return Err(format!("no wasi-sdk build for {other}")),
    };
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => return Err(format!("no wasi-sdk build for {other}")),
    };
    Ok(format!("wasi-sdk-{WASI_SDK_VERSION}.0-{arch}-{os}"))
}

pub fn ensure_sdk(root: &Path) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("WASI_SDK_PATH") {
        return Ok(PathBuf::from(p));
    }
    let name = host_archive()?;
    let dir = root.join("target").join("wasi-sdk");
    let sdk = dir.join(&name);
    let clang = PathBuf::from(tool(&sdk, "clang++"));
    if clang.is_file() {
        return Ok(sdk);
    }
    let _ = std::fs::remove_dir_all(&sdk);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let archive = dir.join(format!("{name}.tar.gz"));
    let url = format!(
        "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{WASI_SDK_VERSION}/{name}.tar.gz"
    );
    println!("\n==> wasi-sdk: downloading {url}");
    let ok = Command::new("curl")
        .arg("-fL")
        .arg(&url)
        .arg("-o")
        .arg(&archive)
        .status()
        .map_err(|e| format!("could not start curl: {e}"))?
        .success();
    if !ok {
        return Err("wasi-sdk download failed".into());
    }
    let ok = Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&dir)
        .status()
        .map_err(|e| format!("could not start tar: {e}"))?
        .success();
    let _ = std::fs::remove_file(&archive);
    if !ok || !clang.is_file() {
        return Err("could not unpack wasi-sdk".into());
    }
    Ok(sdk)
}

fn tool(sdk: &Path, name: &str) -> String {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    sdk.join("bin").join(exe).to_string_lossy().into_owned()
}

pub fn env(sdk: &Path, c_target: &str, threads: bool) -> Vec<(String, String)> {
    let pthread = if threads { " -pthread" } else { "" };
    vec![
        ("CC_wasm32_unknown_unknown".into(), tool(sdk, "clang")),
        ("CXX_wasm32_unknown_unknown".into(), tool(sdk, "clang++")),
        ("AR_wasm32_unknown_unknown".into(), tool(sdk, "llvm-ar")),
        ("CXXSTDLIB_wasm32_unknown_unknown".into(), "c++".into()),
        (
            "CFLAGS_wasm32_unknown_unknown".into(),
            format!("--target={c_target}{pthread}"),
        ),
        (
            "LUAU_CXXFLAGS".into(),
            format!(
                "--target={c_target}{pthread} -fexceptions -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false"
            ),
        ),
    ]
}

pub fn link_flags(sdk: &Path, c_target: &str) -> Vec<String> {
    let lib = sdk
        .join("share")
        .join("wasi-sysroot")
        .join("lib")
        .join(c_target);
    vec![
        format!("-Lnative={}", lib.join("eh").display()),
        format!("-Lnative={}", lib.display()),
        "-lstatic=c++abi".into(),
        "-lstatic=unwind".into(),
        "-lstatic=c".into(),
        "-lstatic=wasi-emulated-process-clocks".into(),
    ]
}
