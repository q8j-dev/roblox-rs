pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
        self.start.elapsed().as_secs_f64()
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

pub fn unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
