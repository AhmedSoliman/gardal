use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::TimeStorage;
use super::atomic::AtomicF64;
use super::cache_padded::CachePadded;

/// Thread-safe storage on its own cache line. The default.
///
/// When many threads use a bucket, it helps to keep its state away from unrelated data: CPUs move
/// memory around in cache lines, and anything sharing the line would slow the bucket down (and be
/// slowed down by it). The padding costs a cache line of memory (64 to 128 bytes, depending on
/// the CPU) instead of 8 bytes; see [`AtomicStorage`](crate::AtomicStorage) when that matters.
///
/// ```rust
/// use gardal::{Limit, StdClock, TokenBucket};
/// use std::num::NonZeroU32;
///
/// // `TokenBucket` uses `PaddedAtomicStorage` unless told otherwise.
/// let bucket: TokenBucket = TokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), StdClock);
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

/// Thread-safe storage on its own cache line, shared by clones.
///
/// The padded version of [`AtomicSharedStorage`](crate::AtomicSharedStorage), and the storage
/// behind [`SharedTokenBucket`](crate::SharedTokenBucket): cloning the bucket gives you another
/// handle to the same tokens.
///
/// ```rust
/// use gardal::{Limit, SharedTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), StdClock);
/// let handle = bucket.clone(); // same tokens, usable from another thread
/// std::thread::spawn(move || handle.consume_one()).join().unwrap();
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
