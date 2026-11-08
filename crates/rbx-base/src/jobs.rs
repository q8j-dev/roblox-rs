pub fn threaded() -> bool {
    cfg!(any(not(target_arch = "wasm32"), target_feature = "atomics"))
}

pub fn spawn<F: FnOnce() + Send + 'static>(f: F) {
    #[cfg(any(not(target_arch = "wasm32"), target_feature = "atomics"))]
    rayon::spawn(f);
    #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
    f();
}
