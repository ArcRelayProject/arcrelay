//! Bounded, all-event timing summaries. Only monotonic, process-local clocks
//! belong here; wall-clock differences between peers are not latency samples.
use std::sync::Mutex;
use std::time::Duration;

const SUB_BUCKETS: usize = 16;
const BUCKETS: usize = 1 + 64 * SUB_BUCKETS;

#[derive(Debug)]
struct Histogram {
    buckets: [u64; BUCKETS],
    count: u64,
    max: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: [0; BUCKETS],
            count: 0,
            max: 0,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct TimingHistogram(Mutex<Histogram>);

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TimingSummary {
    pub count: u64,
    /// Quantiles are bucket upper bounds (at most 6.25% above a sample).
    pub p50_upper_micros: u64,
    pub p95_upper_micros: u64,
    pub p99_upper_micros: u64,
    pub max_micros: u64,
}

pub(crate) fn micros(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn bucket(value: u64) -> usize {
    if value == 0 {
        return 0;
    }
    let exponent = 63 - value.leading_zeros() as usize;
    let base = 1_u128 << exponent;
    1 + exponent * SUB_BUCKETS
        + (((u128::from(value) - base) * SUB_BUCKETS as u128) / base) as usize
}

fn upper(index: usize) -> u64 {
    if index == 0 {
        return 0;
    }
    let exponent = (index - 1) / SUB_BUCKETS;
    let sub = (index - 1) % SUB_BUCKETS;
    let base = 1_u128 << exponent;
    (base + (base * (sub + 1) as u128).div_ceil(SUB_BUCKETS as u128) - 1).min(u128::from(u64::MAX))
        as u64
}

impl TimingHistogram {
    pub fn record(&self, duration: Duration) {
        let value = micros(duration);
        let mut data = self.0.lock().unwrap_or_else(|error| error.into_inner());
        data.buckets[bucket(value)] += 1;
        data.count += 1;
        data.max = data.max.max(value);
    }

    pub fn take(&self) -> TimingSummary {
        let data = std::mem::take(&mut *self.0.lock().unwrap_or_else(|error| error.into_inner()));
        let quantile = |percent: u64| {
            let rank = (u128::from(data.count) * u128::from(percent)).div_ceil(100) as u64;
            let mut cumulative = 0;
            for (index, count) in data.buckets.iter().enumerate() {
                cumulative += count;
                if cumulative >= rank {
                    return upper(index).min(data.max);
                }
            }
            0
        };
        TimingSummary {
            count: data.count,
            p50_upper_micros: quantile(50),
            p95_upper_micros: quantile(95),
            p99_upper_micros: quantile(99),
            max_micros: data.max,
        }
    }
}

impl TimingSummary {
    pub fn emit(&self, stage: &'static str) {
        if self.count == 0 {
            return;
        }
        tracing::info!(
            event = "input.pipeline.timing",
            stage,
            count = self.count,
            p50_upper_micros = self.p50_upper_micros,
            p95_upper_micros = self.p95_upper_micros,
            p99_upper_micros = self.p99_upper_micros,
            max_micros = self.max_micros,
            "Arc Input local monotonic timing interval"
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct PipelineTimings {
    pub native_queue: TimingHistogram,
    pub router_queue: TimingHistogram,
    pub capture_handler: TimingHistogram,
    pub capture_total: TimingHistogram,
    pub receive_queue: TimingHistogram,
    pub receive_handler: TimingHistogram,
    pub receive_total: TimingHistogram,
}

impl PipelineTimings {
    pub fn emit(&self) {
        for (name, histogram) in [
            ("native_queue", &self.native_queue),
            ("router_queue", &self.router_queue),
            ("capture_handler", &self.capture_handler),
            ("capture_total", &self.capture_total),
            ("receive_queue", &self.receive_queue),
            ("receive_handler", &self.receive_handler),
            ("receive_total", &self.receive_total),
        ] {
            histogram.take().emit(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_bound_samples_including_zero_and_u64_max_without_a_runtime() {
        for value in (0..100_000).chain([u64::MAX / 2, u64::MAX]) {
            let bound = upper(bucket(value));
            assert!(bound >= value);
            assert!(u128::from(bound) <= u128::from(value) * 17 / 16);
        }
    }

    #[test]
    fn summaries_include_all_fast_and_slow_events_and_reset_atomically() {
        let histogram = TimingHistogram::default();
        for _ in 0..95 {
            histogram.record(Duration::from_micros(10));
        }
        for _ in 0..5 {
            histogram.record(Duration::from_millis(100));
        }
        let summary = histogram.take();
        assert_eq!(summary.count, 100);
        assert_eq!(summary.p95_upper_micros, 10);
        assert_eq!(summary.p99_upper_micros, 100_000);
        assert_eq!(summary.max_micros, 100_000);
        assert_eq!(histogram.take(), TimingSummary::default());
    }
}
