#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android")))]
fn main() {
    player::desktop_main();
}

#[cfg(target_os = "ios")]
fn main() {
    player::mobile::ios_main();
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
