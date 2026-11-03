// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Metrics on atomics: [`Counter`], [`Gauge`] and [`Histogram`], updated
//! without locks from any thread, and a [`Registry`] that names them and
//! takes a [`Snapshot`] of all of them.
//!
//! ```
//! use softshell_core::metrics::{MetricValue, Registry};
//!
//! let registry = Registry::new();
//! let commits = registry.counter("storage.commits").unwrap();
//! let latency = registry.histogram("storage.commit_us").unwrap();
//! commits.inc();
//! latency.record(250);
//! let snapshot = registry.snapshot();
//! assert_eq!(snapshot.get("storage.commits"), Some(&MetricValue::Counter(1)));
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// A count that only goes up, such as commits or bytes written.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    /// A counter at 0.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Adds 1.
    pub fn inc(&self) {
        self.add(1);
    }

    /// Adds `n`, wrapping at `u64::MAX`.
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// The current count.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A value that goes up and down, such as open cursors or memtable bytes.
#[derive(Debug, Default)]
pub struct Gauge(AtomicI64);

impl Gauge {
    /// A gauge at 0.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicI64::new(0))
    }

    /// Sets the value.
    pub fn set(&self, value: i64) {
        self.0.store(value, Ordering::Relaxed);
    }

    /// Adds `delta` (which may be negative), wrapping at the ends of `i64`.
    pub fn add(&self, delta: i64) {
        self.0.fetch_add(delta, Ordering::Relaxed);
    }

    /// Subtracts `delta`, wrapping at the ends of `i64`.
    pub fn sub(&self, delta: i64) {
        self.0.fetch_sub(delta, Ordering::Relaxed);
    }

    /// The current value.
    #[must_use]
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Bits of each value's mantissa that pick its bucket: 16 linear buckets
/// per power of two.
const SUB_BITS: u32 = 4;
const SUB_BUCKETS: usize = 1 << SUB_BITS;
/// Values 0 to 15 have a bucket each; each of the 60 powers of two from
/// 2^4 to 2^63 has 16.
const BUCKETS: usize = SUB_BUCKETS + (64 - SUB_BITS as usize) * SUB_BUCKETS;

/// The bucket holding `value`.
const fn bucket_of(value: u64) -> usize {
    if value < SUB_BUCKETS as u64 {
        return value as usize;
    }
    // The top bit is bit `top`, 4 to 63; the next 4 bits pick the
    // sub-bucket.
    let top = 63 - value.leading_zeros();
    let shift = top - SUB_BITS;
    let sub = (value >> shift) as usize & (SUB_BUCKETS - 1);
    SUB_BUCKETS + shift as usize * SUB_BUCKETS + sub
}

/// The smallest and largest values in `bucket`.
const fn bucket_bounds(bucket: usize) -> (u64, u64) {
    if bucket < SUB_BUCKETS {
        return (bucket as u64, bucket as u64);
    }
    let shift = ((bucket - SUB_BUCKETS) / SUB_BUCKETS) as u32;
    let sub = ((bucket - SUB_BUCKETS) % SUB_BUCKETS) as u64;
    let low = (SUB_BUCKETS as u64 + sub) << shift;
    (low, low + ((1 << shift) - 1))
}

/// A distribution of `u64` values, such as latencies in microseconds or
/// sizes in bytes, in log-linear buckets.
///
/// Values 0 to 15 are counted exactly. Above that, each power of two is
/// split into 16 equal buckets, so a quantile is reported to within 1/16
/// (6.25%) of the true value. 976 buckets cover all of `u64`, in 7.6 KiB.
pub struct Histogram {
    buckets: [AtomicU64; BUCKETS],
    sum: AtomicU64,
    max: AtomicU64,
}

impl Histogram {
    /// An empty histogram.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buckets: [const { AtomicU64::new(0) }; BUCKETS],
            sum: AtomicU64::new(0),
            max: AtomicU64::new(0),
        }
    }

    /// Counts one value.
    pub fn record(&self, value: u64) {
        // `max` and `sum` first, released by the bucket's increment, so a
        // snapshot that sees the count also sees them.
        self.max.fetch_max(value, Ordering::Relaxed);
        self.sum.fetch_add(value, Ordering::Relaxed);
        // The index is below `BUCKETS` for every `u64`.
        if let Some(bucket) = self.buckets.get(bucket_of(value)) {
            bucket.fetch_add(1, Ordering::Release);
        }
    }

    /// The counts so far. A value recorded while the snapshot is taken may
    /// be in its sum and not its counts, but every counted value is at most
    /// its `max`.
    #[must_use]
    pub fn snapshot(&self) -> HistogramSnapshot {
        let counts: Vec<u64> = self
            .buckets
            .iter()
            .map(|b| b.load(Ordering::Acquire))
            .collect();
        HistogramSnapshot {
            count: counts.iter().fold(0, |total, &n| total.saturating_add(n)),
            counts,
            sum: self.sum.load(Ordering::Relaxed),
            max: self.max.load(Ordering::Relaxed),
        }
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Histogram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = self.snapshot();
        f.debug_struct("Histogram")
            .field("count", &snapshot.count())
            .field("max", &snapshot.max())
            .finish_non_exhaustive()
    }
}

/// The counts of a [`Histogram`] at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistogramSnapshot {
    counts: Vec<u64>,
    count: u64,
    sum: u64,
    max: u64,
}

impl HistogramSnapshot {
    /// How many values were recorded.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// The sum of the values, wrapping at `u64::MAX`.
    #[must_use]
    pub const fn sum(&self) -> u64 {
        self.sum
    }

    /// The largest value, or `None` if there are none.
    #[must_use]
    pub const fn max(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// The mean, or `None` if there are no values. Wrong if the sum
    /// wrapped.
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum as f64 / self.count as f64)
    }

    /// The `q`-quantile, for `q` from 0 to 1 (0.99 is the 99th
    /// percentile): the largest value of the bucket holding the value of
    /// rank `ceil(q × count)` (at least 1), but at most [`max`](Self::max).
    /// It is never below the true quantile, and above it by less than
    /// 1/16. `None` if there are no values or `q` is NaN; `q` is clamped to
    /// 0..=1.
    ///
    /// `q` is taken as the decimal it was written as: `0.55` is a little
    /// above 0.55 in binary, so `0.55 × 100` is a little above 55, and a
    /// product within a few parts in 10^16 of a whole number counts as that
    /// number. `quantile(0.55)` of 100 values is the 55th.
    #[must_use]
    pub fn quantile(&self, q: f64) -> Option<u64> {
        if self.count == 0 || q.is_nan() {
            return None;
        }
        let product = q.clamp(0.0, 1.0) * self.count as f64;
        // Exact for counts below 2^53; the clamp keeps the rank in
        // 1..=count either way.
        let rank = ((product - product * 4.0 * f64::EPSILON).ceil() as u64).clamp(1, self.count);
        let mut seen = 0u64;
        for (bucket, &n) in self.counts.iter().enumerate() {
            seen = seen.saturating_add(n);
            if seen >= rank {
                return Some(bucket_bounds(bucket).1.min(self.max));
            }
        }
        Some(self.max)
    }

    /// The median.
    #[must_use]
    pub fn p50(&self) -> Option<u64> {
        self.quantile(0.50)
    }

    /// The 95th percentile.
    #[must_use]
    pub fn p95(&self) -> Option<u64> {
        self.quantile(0.95)
    }

    /// The 99th percentile.
    #[must_use]
    pub fn p99(&self) -> Option<u64> {
        self.quantile(0.99)
    }
}

/// One metric's value in a [`Snapshot`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetricValue {
    /// A [`Counter`]'s count.
    Counter(u64),
    /// A [`Gauge`]'s value.
    Gauge(i64),
    /// A [`Histogram`]'s counts.
    Histogram(HistogramSnapshot),
}

/// Every metric in a [`Registry`] at one moment, in name order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    metrics: BTreeMap<String, MetricValue>,
}

impl Snapshot {
    /// The value of the metric called `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&MetricValue> {
        self.metrics.get(name)
    }

    /// Every metric, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &MetricValue)> {
        self.metrics
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// How many metrics there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.metrics.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.metrics.is_empty()
    }
}

#[derive(Clone)]
enum Metric {
    Counter(Arc<Counter>),
    Gauge(Arc<Gauge>),
    Histogram(Arc<Histogram>),
}

impl Metric {
    const fn kind(&self) -> &'static str {
        match self {
            Self::Counter(_) => "counter",
            Self::Gauge(_) => "gauge",
            Self::Histogram(_) => "histogram",
        }
    }
}

/// Named metrics. Asking for a name returns the metric registered under it,
/// creating it the first time, so each component can look up its own
/// metrics once and keep the `Arc`.
#[derive(Default)]
pub struct Registry {
    metrics: Mutex<BTreeMap<String, Metric>>,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            metrics: Mutex::new(BTreeMap::new()),
        }
    }

    /// The counter called `name`.
    ///
    /// # Errors
    ///
    /// [`MetricsError::KindMismatch`] if `name` is another kind of metric.
    pub fn counter(&self, name: &str) -> Result<Arc<Counter>, MetricsError> {
        match self.get_or_insert(name, || Metric::Counter(Arc::default()))? {
            Metric::Counter(counter) => Ok(counter),
            other => Err(mismatch(name, &other, "counter")),
        }
    }

    /// The gauge called `name`.
    ///
    /// # Errors
    ///
    /// [`MetricsError::KindMismatch`] if `name` is another kind of metric.
    pub fn gauge(&self, name: &str) -> Result<Arc<Gauge>, MetricsError> {
        match self.get_or_insert(name, || Metric::Gauge(Arc::default()))? {
            Metric::Gauge(gauge) => Ok(gauge),
            other => Err(mismatch(name, &other, "gauge")),
        }
    }

    /// The histogram called `name`.
    ///
    /// # Errors
    ///
    /// [`MetricsError::KindMismatch`] if `name` is another kind of metric.
    pub fn histogram(&self, name: &str) -> Result<Arc<Histogram>, MetricsError> {
        match self.get_or_insert(name, || Metric::Histogram(Arc::default()))? {
            Metric::Histogram(histogram) => Ok(histogram),
            other => Err(mismatch(name, &other, "histogram")),
        }
    }

    /// The current value of every metric.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let metrics = self.lock();
        let metrics = metrics
            .iter()
            .map(|(name, metric)| {
                let value = match metric {
                    Metric::Counter(c) => MetricValue::Counter(c.get()),
                    Metric::Gauge(g) => MetricValue::Gauge(g.get()),
                    Metric::Histogram(h) => MetricValue::Histogram(h.snapshot()),
                };
                (name.clone(), value)
            })
            .collect();
        Snapshot { metrics }
    }

    fn get_or_insert(
        &self,
        name: &str,
        make: impl FnOnce() -> Metric,
    ) -> Result<Metric, MetricsError> {
        let mut metrics = self.lock();
        Ok(metrics.entry(name.to_owned()).or_insert_with(make).clone())
    }

    /// The map. A panic while it was held cannot leave it half-changed, so
    /// a poisoned lock is still usable.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Metric>> {
        self.metrics.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("metrics", &self.lock().len())
            .finish()
    }
}

fn mismatch(name: &str, existing: &Metric, requested: &'static str) -> MetricsError {
    MetricsError::KindMismatch {
        name: name.to_owned(),
        existing: existing.kind(),
        requested,
    }
}

/// A [`Registry`] lookup that cannot be served.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetricsError {
    /// The name is already registered as another kind of metric.
    KindMismatch {
        /// The metric's name.
        name: String,
        /// The kind it was registered as: "counter", "gauge" or
        /// "histogram".
        existing: &'static str,
        /// The kind asked for.
        requested: &'static str,
    },
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch {
                name,
                existing,
                requested,
            } => write!(f, "metric `{name}` is a {existing}, not a {requested}"),
        }
    }
}

/// A leaf error: it has no source.
impl Error for MetricsError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;

    #[test]
    fn buckets_tile_every_u64() {
        assert_eq!(BUCKETS, 976);
        assert_eq!(bucket_bounds(0), (0, 0));
        assert_eq!(bucket_bounds(15), (15, 15));
        assert_eq!(bucket_bounds(16), (16, 16));
        assert_eq!(bucket_bounds(31), (31, 31));
        assert_eq!(bucket_bounds(32), (32, 33));
        assert_eq!(bucket_bounds(BUCKETS - 1), (31 << 59, u64::MAX));
        // Consecutive buckets cover 0..=u64::MAX with no gap or overlap,
        // and every bound maps back to its bucket.
        let mut next = 0u64;
        for bucket in 0..BUCKETS {
            let (low, high) = bucket_bounds(bucket);
            assert_eq!(low, next, "bucket {bucket}");
            assert!(high >= low);
            assert_eq!(bucket_of(low), bucket);
            assert_eq!(bucket_of(high), bucket);
            // Width at most 1/16 of the bucket's smallest value.
            assert!(high - low <= low / 16, "bucket {bucket}");
            next = high.wrapping_add(1);
        }
        assert_eq!(next, 0, "the last bucket ends at u64::MAX");
        for value in [0, 1, 15, 16, 17, 255, 256, 1 << 40, (1 << 40) - 1, u64::MAX] {
            let (low, high) = bucket_bounds(bucket_of(value));
            assert!((low..=high).contains(&value), "{value}");
        }
    }

    #[test]
    fn quantiles_are_within_one_sixteenth_above_the_exact_value() {
        let mut rng = Pcg32::new(3, 4);
        let histogram = Histogram::new();
        // Log-uniform values from 0 to 2^40.
        let mut values: Vec<u64> = (0..20_000)
            .map(|_| {
                let bits = rng.range(0..41) as u32;
                rng.next_u64() >> (64 - bits).min(63) >> u32::from(bits == 0)
            })
            .collect();
        for &v in &values {
            histogram.record(v);
        }
        values.sort_unstable();
        let snapshot = histogram.snapshot();
        assert_eq!(snapshot.count(), 20_000);
        assert_eq!(snapshot.max(), values.last().copied());
        assert_eq!(snapshot.sum(), values.iter().sum::<u64>());
        for q in [0.0, 0.001, 0.25, 0.5, 0.9, 0.95, 0.99, 0.999, 1.0] {
            let rank = ((q * values.len() as f64).ceil() as usize).max(1);
            let exact = values[rank - 1];
            let reported = snapshot.quantile(q).unwrap();
            assert!(reported >= exact, "q {q}: {reported} < {exact}");
            assert!(
                reported - exact <= exact / 16,
                "q {q}: {reported} vs {exact}"
            );
        }
        assert_eq!(snapshot.quantile(1.0), snapshot.max());
        assert_eq!(snapshot.quantile(7.0), snapshot.quantile(1.0));
        assert_eq!(snapshot.quantile(-1.0), snapshot.quantile(0.0));
        assert_eq!(snapshot.quantile(f64::NAN), None);
    }

    #[test]
    fn small_histograms() {
        let histogram = Histogram::new();
        let empty = histogram.snapshot();
        assert_eq!((empty.count(), empty.sum(), empty.max()), (0, 0, None));
        assert_eq!((empty.p50(), empty.mean()), (None, None));
        for v in [3, 1, 2] {
            histogram.record(v);
        }
        let s = histogram.snapshot();
        assert_eq!((s.p50(), s.p95(), s.p99()), (Some(2), Some(3), Some(3)));
        assert_eq!(s.quantile(0.0), Some(1));
        assert_eq!(s.mean(), Some(2.0));
        // The top bucket is capped at the largest value seen.
        histogram.record(1000);
        assert_eq!(histogram.snapshot().quantile(1.0), Some(1000));
        histogram.record(u64::MAX);
        assert_eq!(histogram.snapshot().quantile(1.0), Some(u64::MAX));
        assert_eq!(histogram.snapshot().max(), Some(u64::MAX));
        // A recorded 0 is not an empty histogram.
        let zero = Histogram::new();
        zero.record(0);
        assert_eq!(
            (zero.snapshot().max(), zero.snapshot().p99()),
            (Some(0), Some(0))
        );
        assert_eq!(zero.snapshot().mean(), Some(0.0));
        assert_eq!(
            format!("{zero:?}"),
            "Histogram { count: 1, max: Some(0), .. }"
        );
        assert_eq!(
            format!("{:?}", Histogram::new()),
            "Histogram { count: 0, max: None, .. }"
        );
    }

    #[test]
    fn named_percentiles_use_their_own_ranks() {
        let histogram = Histogram::new();
        for v in 1..=100 {
            histogram.record(v);
        }
        let s = histogram.snapshot();
        // Ranks 50, 95 and 99, reported as their buckets' highest values:
        // 50 is in 50..=51, 95 in 92..=95, 99 in 96..=99.
        assert_eq!((s.p50(), s.p95(), s.p99()), (Some(51), Some(95), Some(99)));
        assert_eq!(s.quantile(0.9), Some(91));
    }

    /// Two-digit quantiles use the rank an exact decimal computation gives,
    /// not one more because of binary rounding.
    #[test]
    fn decimal_quantiles_get_the_decimal_rank() {
        for count in [7u64, 100, 101, 999, 1000, 4096] {
            let histogram = Histogram::new();
            for v in 1..=count {
                histogram.record(v);
            }
            let s = histogram.snapshot();
            for k in 1..100u64 {
                // ceil(k × count / 100) in integers; value v has rank v.
                let rank = (k * count).div_ceil(100).max(1);
                let expected = bucket_bounds(bucket_of(rank)).1.min(count);
                assert_eq!(
                    s.quantile(k as f64 / 100.0),
                    Some(expected),
                    "{k}% of {count}"
                );
            }
        }
        // 55 ones and 45 thousands: the 55th value is 1.
        let histogram = Histogram::new();
        (0..55).for_each(|_| histogram.record(1));
        (0..45).for_each(|_| histogram.record(1000));
        assert_eq!(histogram.snapshot().quantile(0.55), Some(1));
        // The 56th is 1000, in bucket 992..=1023, capped at the maximum.
        assert_eq!(histogram.snapshot().quantile(0.56), Some(1000));
    }

    #[test]
    fn counters_and_gauges() {
        let counter = Counter::new();
        counter.inc();
        counter.add(41);
        assert_eq!(counter.get(), 42);
        let gauge = Gauge::new();
        gauge.add(10);
        gauge.sub(15);
        assert_eq!(gauge.get(), -5);
        gauge.set(7);
        assert_eq!(gauge.get(), 7);
    }

    #[test]
    fn concurrent_updates_are_all_counted() {
        let registry = Registry::new();
        std::thread::scope(|scope| {
            for t in 0..8u64 {
                let registry = &registry;
                scope.spawn(move || {
                    let counter = registry.counter("ops").unwrap();
                    let histogram = registry.histogram("sizes").unwrap();
                    for i in 0..10_000 {
                        counter.inc();
                        histogram.record(t * 10_000 + i);
                    }
                });
            }
        });
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.get("ops"), Some(&MetricValue::Counter(80_000)));
        let Some(MetricValue::Histogram(sizes)) = snapshot.get("sizes") else {
            panic!("sizes is a histogram");
        };
        assert_eq!(sizes.count(), 80_000);
        assert_eq!(sizes.max(), Some(79_999));
        assert_eq!(sizes.sum(), (0..80_000u64).sum::<u64>());
    }

    #[test]
    fn registry_returns_the_same_metric_and_rejects_another_kind() {
        let registry = Registry::new();
        let a = registry.counter("txn.commits").unwrap();
        let b = registry.counter("txn.commits").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        registry.gauge("cache.bytes").unwrap().set(-3);
        registry.histogram("compaction.ms").unwrap().record(9);
        let err = registry.gauge("txn.commits").unwrap_err();
        assert_eq!(
            err,
            MetricsError::KindMismatch {
                name: "txn.commits".to_owned(),
                existing: "counter",
                requested: "gauge"
            }
        );
        assert_eq!(
            err.to_string(),
            "metric `txn.commits` is a counter, not a gauge"
        );
        assert!(registry.histogram("cache.bytes").is_err());
        assert!(registry.counter("compaction.ms").is_err());
        a.add(2);
        let snapshot = registry.snapshot();
        let names: Vec<&str> = snapshot.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["cache.bytes", "compaction.ms", "txn.commits"]);
        assert_eq!(snapshot.get("cache.bytes"), Some(&MetricValue::Gauge(-3)));
        assert_eq!(snapshot.get("txn.commits"), Some(&MetricValue::Counter(2)));
        assert_eq!(snapshot.len(), 3);
        assert!(Registry::new().snapshot().is_empty());
        assert_eq!(format!("{registry:?}"), "Registry { metrics: 3 }");
    }
}
