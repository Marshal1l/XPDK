//! Time utilities for high-performance timestamping and timing

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// High-resolution timestamp
pub type Timestamp = u64;

/// Timestamp source
#[derive(Debug, Clone, Copy)]
pub enum TimestampSource {
    SystemClock,
    MonotonicClock,
    TscClock,
}

/// High-resolution timer
pub struct HighResTimer {
    /// Timestamp source
    source: TimestampSource,
    /// TSC frequency (if using TSC)
    tsc_frequency: u64,
    /// TSC offset calibration
    tsc_offset: AtomicU64,
}

impl HighResTimer {
    /// Create a new high-resolution timer
    pub fn new(source: TimestampSource) -> Self {
        let tsc_frequency = if matches!(source, TimestampSource::TscClock) {
            calibrate_tsc()
        } else {
            0
        };

        Self {
            source,
            tsc_frequency,
            tsc_offset: AtomicU64::new(0),
        }
    }

    /// Get current timestamp in nanoseconds
    pub fn now(&self) -> Timestamp {
        match self.source {
            TimestampSource::SystemClock => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as Timestamp,
            TimestampSource::MonotonicClock => {
                // Use a base instant to ensure monotonic increasing values
                static BASE_INSTANT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
                let base = BASE_INSTANT.get_or_init(Instant::now);
                base.elapsed().as_nanos() as Timestamp
            }
            TimestampSource::TscClock => self.tsc_to_nanos(read_tsc()),
        }
    }

    /// Convert timestamp to Duration
    pub fn to_duration(&self, timestamp: Timestamp) -> Duration {
        Duration::from_nanos(timestamp)
    }

    /// Convert Duration to timestamp
    pub fn from_duration(&self, duration: Duration) -> Timestamp {
        duration.as_nanos() as Timestamp
    }

    /// Measure elapsed time between two timestamps
    pub fn elapsed(&self, start: Timestamp, end: Timestamp) -> Duration {
        Duration::from_nanos(end.saturating_sub(start))
    }

    /// Calibrate TSC clock
    pub fn calibrate(&mut self) -> Result<(), crate::Error> {
        if matches!(self.source, TimestampSource::TscClock) {
            self.tsc_frequency = calibrate_tsc();
            Ok(())
        } else {
            Err(crate::Error::InvalidConfig(
                "TSC calibration not applicable".to_string(),
            ))
        }
    }

    /// Get TSC frequency
    pub fn tsc_frequency(&self) -> u64 {
        self.tsc_frequency
    }

    /// Convert TSC cycles to nanoseconds
    fn tsc_to_nanos(&self, tsc: u64) -> Timestamp {
        let freq = self.tsc_frequency;
        if freq > 0 {
            // Convert TSC cycles to nanoseconds
            let offset = self.tsc_offset.load(Ordering::Relaxed);
            ((tsc.wrapping_sub(offset)) as u128 * 1_000_000_000 / freq as u128) as Timestamp
        } else {
            // Fallback to monotonic clock
            Instant::now().elapsed().as_nanos() as Timestamp
        }
    }
}

impl Default for HighResTimer {
    fn default() -> Self {
        Self::new(TimestampSource::MonotonicClock)
    }
}

/// Read TSC (Time Stamp Counter)
#[inline]
fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        unsafe {
            let mut low: u32;
            let mut high: u32;
            core::arch::asm!(
                "rdtsc",
                out("eax") low,
                out("edx") high,
            );
            ((high as u64) << 32) | (low as u64)
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // Fallback for non-x86_64 architectures
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }
}

/// Calibrate TSC frequency
fn calibrate_tsc() -> u64 {
    // Use CPUID to get TSC frequency if available
    #[cfg(target_arch = "x86_64")]
    {
        if let Some(freq) = get_tsc_frequency_from_cpuid() {
            return freq;
        }
    }

    // Fallback: measure using system clock
    let iterations = 1_000_000;
    let start = read_tsc();
    let start_time = SystemTime::now();

    for _ in 0..iterations {
        // Busy wait
    }

    let end = read_tsc();
    let elapsed = start_time.elapsed().unwrap_or_default();

    if elapsed.as_nanos() > 0 {
        ((end - start) as u128 * 1_000_000_000 / elapsed.as_nanos()) as u64
    } else {
        2_400_000_000 // Default to 2.4 GHz
    }
}

/// Get TSC frequency from CPUID (simplified)
#[cfg(target_arch = "x86_64")]
fn get_tsc_frequency_from_cpuid() -> Option<u64> {
    // This is a simplified implementation
    // In a real implementation, you would use CPUID leaf 0x15
    None
}

/// Latency tracker
pub struct LatencyTracker {
    /// Timer for timestamping
    timer: HighResTimer,
    /// Samples
    samples: Vec<u64>,
    /// Maximum samples
    max_samples: usize,
    /// Current index
    index: usize,
    /// Total count
    count: u64,
    /// Minimum latency
    min_latency: AtomicU64,
    /// Maximum latency
    max_latency: AtomicU64,
}

impl LatencyTracker {
    /// Create a new latency tracker
    pub fn new(max_samples: usize) -> Self {
        Self {
            timer: HighResTimer::new(TimestampSource::TscClock),
            samples: vec![0; max_samples],
            max_samples,
            index: 0,
            count: 0,
            min_latency: AtomicU64::new(u64::MAX),
            max_latency: AtomicU64::new(0),
        }
    }

    /// Record a latency measurement
    pub fn record(&mut self, start: Timestamp) {
        let now = self.timer.now();
        let latency = now.saturating_sub(start);

        // Update min/max
        self.min_latency.fetch_min(latency, Ordering::Relaxed);
        self.max_latency.fetch_max(latency, Ordering::Relaxed);

        // Store sample
        self.samples[self.index] = latency;
        self.index = (self.index + 1) % self.max_samples;
        self.count += 1;
    }

    /// Get latency statistics
    pub fn stats(&self) -> LatencyStats {
        let mut sorted_samples = self.samples.clone();
        if self.count < self.max_samples as u64 {
            sorted_samples.truncate(self.count as usize);
        }
        sorted_samples.sort_unstable();

        let min = self.min_latency.load(Ordering::Relaxed);
        let max = self.max_latency.load(Ordering::Relaxed);

        let mean = if self.count > 0 {
            sorted_samples.iter().sum::<u64>() / self.count as u64
        } else {
            0
        };

        let p50 = percentile(&sorted_samples, 0.5);
        let p95 = percentile(&sorted_samples, 0.95);
        let p99 = percentile(&sorted_samples, 0.99);
        let p999 = percentile(&sorted_samples, 0.999);

        LatencyStats {
            count: self.count,
            min,
            max,
            mean,
            p50,
            p95,
            p99,
            p999,
        }
    }

    /// Reset the tracker
    pub fn reset(&mut self) {
        self.samples.fill(0);
        self.index = 0;
        self.count = 0;
        self.min_latency.store(u64::MAX, Ordering::Relaxed);
        self.max_latency.store(0, Ordering::Relaxed);
    }
}

/// Latency statistics
#[derive(Debug)]
pub struct LatencyStats {
    pub count: u64,
    pub min: u64,
    pub max: u64,
    pub mean: u64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub p999: u64,
}

/// Calculate percentile from sorted samples
fn percentile(sorted_samples: &[u64], percentile: f64) -> u64 {
    if sorted_samples.is_empty() {
        return 0;
    }

    let index = ((sorted_samples.len() as f64 - 1.0) * percentile) as usize;
    sorted_samples[index.min(sorted_samples.len() - 1)]
}

/// Rate limiter
pub struct RateLimiter {
    /// Timer
    timer: HighResTimer,
    /// Rate in operations per second
    rate: u64,
    /// Time per operation in nanoseconds
    time_per_op: u64,
    /// Next allowed operation time
    next_allowed: AtomicU64,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(rate: u64) -> Self {
        let time_per_op = if rate > 0 { 1_000_000_000 / rate } else { 0 };

        Self {
            timer: HighResTimer::new(TimestampSource::MonotonicClock),
            rate,
            time_per_op,
            next_allowed: AtomicU64::new(0),
        }
    }

    /// Check if operation is allowed
    pub fn try_acquire(&self) -> bool {
        if self.rate == 0 {
            return true; // Unlimited rate
        }

        let now = self.timer.now();
        let next_allowed = self.next_allowed.load(Ordering::Relaxed);

        if now >= next_allowed {
            let new_next = next_allowed.saturating_add(self.time_per_op);
            if self
                .next_allowed
                .compare_exchange_weak(next_allowed, new_next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                true
            } else {
                // Another thread updated, try again
                self.try_acquire()
            }
        } else {
            false
        }
    }

    /// Wait until operation is allowed (blocking)
    pub fn acquire(&self) {
        while !self.try_acquire() {
            // Busy wait or yield
            std::hint::spin_loop();
        }
    }

    /// Get current rate
    pub fn rate(&self) -> u64 {
        self.rate
    }

    /// Update rate
    pub fn set_rate(&mut self, rate: u64) {
        self.rate = rate;
        self.time_per_op = if rate > 0 { 1_000_000_000 / rate } else { 0 };
    }
}

/// Time window counter
pub struct TimeWindowCounter {
    /// Timer
    timer: HighResTimer,
    /// Window duration in nanoseconds
    window_duration: u64,
    /// Buckets
    buckets: Vec<AtomicU64>,
    /// Current bucket index
    current_bucket: AtomicU64,
    /// Last update time
    last_update: AtomicU64,
}

impl TimeWindowCounter {
    /// Create a new time window counter
    pub fn new(window_duration: Duration, num_buckets: usize) -> Self {
        let window_duration_ns = window_duration.as_nanos() as u64;

        Self {
            timer: HighResTimer::new(TimestampSource::MonotonicClock),
            window_duration: window_duration_ns,
            buckets: (0..num_buckets).map(|_| AtomicU64::new(0)).collect(),
            current_bucket: AtomicU64::new(0),
            last_update: AtomicU64::new(0),
        }
    }

    /// Increment counter
    pub fn increment(&self) {
        self.add(1);
    }

    /// Add value to counter
    pub fn add(&self, value: u64) {
        self.update_buckets();
        let current = self.current_bucket.load(Ordering::Relaxed) as usize;
        self.buckets[current].fetch_add(value, Ordering::Relaxed);
    }

    /// Get count in current window
    pub fn count(&self) -> u64 {
        self.update_buckets();

        let mut total = 0;
        for bucket in &self.buckets {
            total += bucket.load(Ordering::Relaxed);
        }

        total
    }

    /// Reset counter
    pub fn reset(&self) {
        for bucket in &self.buckets {
            bucket.store(0, Ordering::Relaxed);
        }
    }

    /// Update buckets based on current time
    fn update_buckets(&self) {
        let now = self.timer.now();
        let last = self.last_update.load(Ordering::Relaxed);

        if now >= last {
            let elapsed = now - last;
            let bucket_duration = self.window_duration / self.buckets.len() as u64;

            if elapsed >= bucket_duration {
                let buckets_to_advance = (elapsed / bucket_duration) as usize;
                let current = self.current_bucket.load(Ordering::Relaxed) as usize;

                for i in 0..buckets_to_advance.min(self.buckets.len()) {
                    let bucket_index = (current + i + 1) % self.buckets.len();
                    self.buckets[bucket_index].store(0, Ordering::Relaxed);
                }

                let new_current = (current + buckets_to_advance) % self.buckets.len();
                self.current_bucket
                    .store(new_current as u64, Ordering::Relaxed);
                self.last_update.store(now, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_res_timer() {
        let timer = HighResTimer::new(TimestampSource::MonotonicClock);
        let start = timer.now();
        std::thread::sleep(Duration::from_millis(1));
        let end = timer.now();

        assert!(end > start);

        let elapsed = timer.elapsed(start, end);
        assert!(elapsed.as_millis() >= 1);
    }

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new(100);
        let timer = HighResTimer::new(TimestampSource::MonotonicClock);

        let start = timer.now();
        std::thread::sleep(Duration::from_millis(1));
        tracker.record(start);

        let stats = tracker.stats();
        assert_eq!(stats.count, 1);
        assert!(stats.min > 0);
        assert!(stats.max > 0);
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(1000); // 1000 ops/sec

        // Should allow some operations
        let mut allowed = 0;
        for _ in 0..10 {
            if limiter.try_acquire() {
                allowed += 1;
            }
        }

        assert!(allowed > 0);
    }

    #[test]
    fn test_time_window_counter() {
        let counter = TimeWindowCounter::new(Duration::from_secs(1), 10);

        for _ in 0..100 {
            counter.increment();
        }

        assert_eq!(counter.count(), 100);
    }
}
