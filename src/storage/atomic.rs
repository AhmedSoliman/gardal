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

/// Atomic implementation of [`TimeStorage`]
#[derive(Debug)]
pub struct AtomicStorage(AtomicF64);

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

/// Atomic shared storage of [`TimeStorage`]
#[derive(Debug, Clone)]
pub struct AtomicSharedStorage(Arc<AtomicF64>);

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
