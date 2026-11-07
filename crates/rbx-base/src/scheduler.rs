#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u32);

#[derive(Clone, Debug)]
pub struct Job {
    pub name: String,
    pub hz: f64,
    pub accumulated: f64,
    pub enabled: bool,
    pub runs: u64,
}

#[derive(Debug, Default)]
pub struct Scheduler {
    jobs: Vec<Job>,
    pub throttle: bool,
}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler::default()
    }

    pub fn add(&mut self, name: &str, hz: f64) -> JobId {
        self.jobs.push(Job {
            name: name.to_string(),
            hz,
            accumulated: 0.0,
            enabled: true,
            runs: 0,
        });
        JobId(self.jobs.len() as u32 - 1)
    }

    pub fn set_enabled(&mut self, id: JobId, enabled: bool) {
        if let Some(j) = self.jobs.get_mut(id.0 as usize) {
            j.enabled = enabled;
        }
    }

    pub fn set_hz(&mut self, id: JobId, hz: f64) {
        if let Some(j) = self.jobs.get_mut(id.0 as usize) {
            j.hz = hz;
        }
    }

    pub fn advance(&mut self, dt: f64) -> Vec<(JobId, u32)> {
        let mut due = Vec::new();
        for (i, job) in self.jobs.iter_mut().enumerate() {
            if !job.enabled || job.hz <= 0.0 {
                continue;
            }
            job.accumulated += dt;
            let period = 1.0 / job.hz;
            let mut count = 0u32;
            while job.accumulated >= period && count < 8 {
                job.accumulated -= period;
                count += 1;
            }
            if count == 8 {
                job.accumulated = job.accumulated.min(period);
            }
            if count > 0 {
                job.runs += count as u64;
                due.push((JobId(i as u32), count));
            }
        }
        due
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_at_rate() {
        let mut s = Scheduler::new();
        let physics = s.add("physics", 240.0);
        let heartbeat = s.add("heartbeat", 60.0);
        let mut physics_runs = 0;
        let mut heartbeat_runs = 0;
        for _ in 0..60 {
            for (id, n) in s.advance(1.0 / 60.0) {
                if id == physics {
                    physics_runs += n;
                } else if id == heartbeat {
                    heartbeat_runs += n;
                }
            }
        }
        assert!((239..=241).contains(&physics_runs));
        assert!((59..=61).contains(&heartbeat_runs));
    }
}
