mod atomic;
mod cache_padded;
mod clock;
mod error;
#[cfg(feature = "async")]
pub mod futures;
mod limit;
mod raw;
mod storage;
mod tokens;

#[cfg(feature = "tokio")]
pub use clock::TokioClock;
pub use clock::{Clock, ManualClock, StdClock};
#[cfg(feature = "quanta")]
pub use clock::{FastClock, QuantaClock};
pub use error::*;
pub use limit::RateLimit;
pub use raw::RawTokenBucket;
pub use tokens::Tokens;

use std::cell::Cell;
use std::sync::atomic::Ordering;

use crate::atomic::AtomicF64;
use crate::cache_padded::CachePadded;

/// Storage policy abstraction used by [`RawTokenBucket`].
///
/// Implementations can provide either atomic or non-atomic access to the
/// underlying timestamp depending on the desired level of concurrency.
pub trait TimeStorage {
    /// Create a new storage policy with the provided zero time.
    fn new(zero_time: f64) -> Self;
    /// Load the current zero time.
    fn load(&self) -> f64;
    /// Store the zero time.
    fn store(&self, value: f64);
    /// Compare and exchange the zero time.
    fn compare_exchange_weak(&self, current: f64, new: f64) -> Result<(), f64>;
}

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

/// Non atomic implementation of [`TimeStorage`]. This is intended for
/// single threaded scenarios and uses [`Cell`] internally.
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
