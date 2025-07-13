use std::cell::Cell;

use super::TimeStorage;

/// Non-atomic storage implementation for single-threaded use.
///
/// Uses [`Cell`] internally for efficient single-threaded access.
/// This storage type is not thread-safe and should only be used
/// when the token bucket will be accessed from a single thread.
///
/// # Performance
///
/// Offers the best performance for single-threaded scenarios as it
/// avoids the overhead of atomic operations.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit, LocalStorage, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, StdClock::default());
/// ```
#[derive(Debug)]
pub struct LocalStorage(Cell<f64>);

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
