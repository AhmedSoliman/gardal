use std::cell::Cell;

use super::TimeStorage;

/// Single-threaded storage: the fastest option.
///
/// Uses a plain [`Cell`] instead of an atomic, so a bucket with this storage can't be shared
/// between threads (it isn't `Sync`), but it can still be moved to another thread. Use it for
/// buckets owned by one thread or one task, such as a per-connection limit.
///
/// ```rust
/// use gardal::{Limit, LocalTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// // `LocalTokenBucket` is `TokenBucket<LocalStorage, _>`.
/// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), StdClock);
/// ```
#[derive(Debug)]
pub struct LocalStorage(Cell<f64>);

impl crate::private::Sealed for LocalStorage {}

impl TimeStorage for LocalStorage {
    fn new(zero_time: f64) -> Self {
        Self(Cell::new(zero_time))
    }

    fn load(&self) -> f64 {
        self.0.get()
    }

    fn store(&self, value: f64) {
        self.0.set(value);
    }

    fn compare_exchange_weak(&self, _current: f64, new: f64) -> Result<(), f64> {
        self.0.set(new);
        Ok(())
    }
}
