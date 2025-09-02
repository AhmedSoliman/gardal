use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use likely_stable::LikelyResult;

use super::TimeStorage;

pub(super) struct AtomicF64 {
    storage: AtomicU64,
}
impl AtomicF64 {
    pub fn new(value: f64) -> Self {
        let as_u64 = value.to_bits();
        Self {
            storage: AtomicU64::new(as_u64),
        }
    }

    pub fn store(&self, value: f64, ordering: Ordering) {
        let as_u64 = value.to_bits();
        self.storage.store(as_u64, ordering)
    }

    pub fn load(&self, ordering: Ordering) -> f64 {
        let as_u64 = self.storage.load(ordering);
        f64::from_bits(as_u64)
    }

    pub fn compare_exchange_weak(
        &self,
        current: f64,
        new: f64,
        success: Ordering,
        failure: Ordering,
    ) -> Result<(), f64> {
        self.storage
            .compare_exchange_weak(current.to_bits(), new.to_bits(), success, failure)
            .map_likely(|_| ())
            .map_err_likely(f64::from_bits)
    }
}

impl Debug for AtomicF64 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&f64::from_bits(self.storage.load(Ordering::Relaxed)), f)
    }
}

/// Thread-safe atomic storage implementation.
///
/// Uses atomic operations for concurrent access to token bucket state.
/// Suitable for multi-threaded scenarios where multiple threads need
/// to access the same token bucket.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit, AtomicStorage, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket = TokenBucket::<AtomicStorage, _>::from_parts(limit, StdClock::default());
/// ```
#[derive(Debug)]
pub struct AtomicStorage(AtomicF64);

impl crate::private::Sealed for AtomicStorage {}

impl TimeStorage for AtomicStorage {
    fn new(zero_time: f64) -> Self {
        Self(AtomicF64::new(zero_time))
    }

    fn load(&self) -> f64 {
        self.0.load(Ordering::Relaxed)
    }

    fn store(&self, value: f64) {
        self.0.store(value, Ordering::Relaxed);
    }

    fn compare_exchange_weak(&self, current: f64, new: f64) -> Result<(), f64> {
        self.0
            .compare_exchange_weak(current, new, Ordering::Relaxed, Ordering::Relaxed)
    }
}

/// Shared atomic storage that can be cloned and shared across multiple token buckets.
///
/// This allows multiple token bucket instances to share the same underlying
/// token state, useful for implementing shared rate limiting across different
/// components.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit, AtomicSharedStorage, ManualClock};
/// use std::num::NonZeroU32;
/// use std::sync::Arc;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = Arc::new(ManualClock::new(0.0));
/// let bucket1 = TokenBucket::<AtomicSharedStorage, _>::from_parts(limit, Arc::clone(&clock));
/// let bucket2 = bucket1.clone(); // Shares the same token state
/// ```
#[derive(Debug, Clone)]
pub struct AtomicSharedStorage(Arc<AtomicF64>);

impl crate::private::Sealed for AtomicSharedStorage {}

impl TimeStorage for AtomicSharedStorage {
    fn new(zero_time: f64) -> Self {
        Self(Arc::new(AtomicF64::new(zero_time)))
    }

    fn load(&self) -> f64 {
        self.0.load(Ordering::Relaxed)
    }

    fn store(&self, value: f64) {
        self.0.store(value, Ordering::Relaxed);
    }

    fn compare_exchange_weak(&self, current: f64, new: f64) -> Result<(), f64> {
        self.0
            .compare_exchange_weak(current, new, Ordering::Relaxed, Ordering::Relaxed)
    }
}
