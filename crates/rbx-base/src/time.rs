#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
struct Instant(f64);

#[cfg(target_arch = "wasm32")]
impl Instant {
    fn now() -> Instant {
        Instant(wasm_now())
    }

    fn elapsed_secs(&self) -> f64 {
        (wasm_now() - self.0) / 1000.0
    }
}

#[cfg(target_arch = "wasm32")]
static WASM_CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_arch = "wasm32")]
pub fn set_wasm_now_ms(ms: f64) {
    WASM_CLOCK.store(ms.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(target_arch = "wasm32")]
fn wasm_now() -> f64 {
    f64::from_bits(WASM_CLOCK.load(std::sync::atomic::Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    start: Instant,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Stopwatch::new()
    }
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_secs_f64()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.start.elapsed_secs()
        }
    }

    pub fn reset(&mut self) -> f64 {
        let e = self.elapsed();
        self.start = Instant::now();
        e
    }
}

#[derive(Clone, Debug, Default)]
pub struct Clock {
    pub game_time: f64,
    pub frame: u64,
}

impl Clock {
    pub fn advance(&mut self, dt: f64) {
        self.game_time += dt;
        self.frame += 1;
    }
}
