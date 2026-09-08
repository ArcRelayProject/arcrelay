use std::time::{Duration, Instant};

pub fn delay(failures: u32) -> Duration {
    Duration::from_secs(1_u64 << failures.min(7)).min(Duration::from_secs(120))
}

pub struct RetryBackoff {
    failures: u32,
    next: Instant,
}

impl RetryBackoff {
    pub fn failed(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.next = now + delay(self.failures);
    }

    pub fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            next: now,
        }
    }
    pub fn ready(&self, now: Instant) -> bool {
        now >= self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_peer_backs_off_without_being_reset_by_discovery() {
        let now = Instant::now();
        let mut retry = RetryBackoff::new(now);
        retry.failed(now);
        assert!(!retry.ready(now + Duration::from_secs(1)));
        assert!(retry.ready(now + Duration::from_secs(2)));
        for _ in 0..100 {
            retry.failed(now);
        }
        assert!(!retry.ready(now + Duration::from_secs(119)));
        assert!(retry.ready(now + Duration::from_secs(120)));
    }
}
