use std::sync::atomic::Ordering;

use super::TimeStorage;
use super::atomic::AtomicF64;
use super::cache_padded::CachePadded;

/// Atomic implementation of [`TimeStorage`] padded to one cache line to
/// avoid false sharing.
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
