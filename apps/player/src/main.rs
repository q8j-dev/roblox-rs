#[cfg(not(target_arch = "wasm32"))]
fn main() {
    player::desktop_main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
