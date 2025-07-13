use std::sync::atomic::Ordering;

use super::TimeStorage;
use super::atomic::AtomicF64;
use super::cache_padded::CachePadded;

/// Cache-line padded atomic storage to prevent false sharing.
///
/// This is the default storage implementation that provides thread-safe access
/// while avoiding false sharing between CPU cores. The padding ensures that
/// the atomic variable occupies its own cache line.
///
/// # Performance
///
/// Offers the best performance for multi-threaded scenarios by preventing
/// false sharing, which can significantly impact performance when multiple
/// cores are accessing nearby memory locations.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit};
/// use std::num::NonZeroU32;
///
/// // PaddedAtomicStorage is the default storage type
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket = TokenBucket::new(limit);
/// ```
pub struct PaddedAtomicStorage(CachePadded<AtomicF64>);

impl TimeStorage for PaddedAtomicStorage {
    fn new(zero_time: f64) -> Self {
        Self(CachePadded::new(AtomicF64::new(zero_time)))
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
