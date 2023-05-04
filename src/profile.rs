use std::time::{Duration, Instant};

pub struct ProfileSet {
    runs: Vec<Duration>,
    start: Option<Instant>,
}
impl ProfileSet {
    pub fn new() -> Self {
        Self {
            runs: Vec::with_capacity(1024),
            start: None,
        }
    }

    pub fn start(&mut self) {
        self.start = Some(Instant::now());
    }
    pub fn stop(&mut self) {
        if let Some(start) = self.start.take() {
            self.runs.push(start.elapsed());
        }
    }

    pub fn scoped_run<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.start();
        let rval = f();
        self.stop();
        rval
    }

    pub fn clear(&mut self) {
        self.runs.clear();
        self.start = None;
    }

    pub fn mean(&self) -> Duration {
        let sum: Duration = self.runs.iter().sum();
        sum / self.runs.len() as u32
    }

    pub fn median(&self) -> Duration {
        self.runs[self.runs.len() / 2]
    }

    pub fn variance(&self) -> Duration {
        let mean = self.mean().as_micros() as i128;
        let top_term: i128 = self
            .runs
            .iter()
            .map(|r| {
                let tmp = r.as_micros() as i128 - mean;
                tmp * tmp
            })
            .sum();
        let bot_term = self.runs.len() as i128 - 1;

        Duration::from_micros((top_term / bot_term) as u64)
    }

    pub fn std_dev(&self) -> Duration {
        Duration::from_micros((self.variance().as_micros() as f64).sqrt() as u64)
    }

    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub fn last(&self) -> Option<Duration> {
        self.runs.last().map(|r| *r)
    }
}
