use std::sync::Arc;
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
/// use gardal::{TokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// // PaddedAtomicStorage is the default storage type
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket: TokenBucket = TokenBucket::new(limit, StdClock);
/// ```
#[derive(Debug)]
pub struct PaddedAtomicStorage(CachePadded<AtomicF64>);

impl crate::private::Sealed for PaddedAtomicStorage {}

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

/// Shared, cache-line padded atomic storage.
///
/// Like [`PaddedAtomicStorage`], but the padded atomic lives behind an [`Arc`], so
/// cloning a token bucket that uses this storage yields a handle to the *same*
/// token state. This is the storage used by [`SharedTokenBucket`](crate::SharedTokenBucket).
///
/// # Examples
///
/// ```rust
/// use gardal::{SharedTokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket1 = SharedTokenBucket::new(limit, StdClock);
/// let bucket2 = bucket1.clone(); // Shares the same token state
/// ```
#[derive(Debug, Clone)]
pub struct PaddedAtomicSharedStorage(Arc<CachePadded<AtomicF64>>);

impl crate::private::Sealed for PaddedAtomicSharedStorage {}

impl TimeStorage for PaddedAtomicSharedStorage {
    fn new(zero_time: f64) -> Self {
        Self(Arc::new(CachePadded::new(AtomicF64::new(zero_time))))
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
