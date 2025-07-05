use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};

use likely_stable::LikelyResult;

pub struct AtomicF64 {
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
