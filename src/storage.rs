pub mod atomic;
mod cache_padded;
pub mod local;
pub mod padded_atomic;

/// Where a token bucket keeps its state.
///
/// A bucket's whole state is one `f64`: the time at which it was (or will be) empty. The storage
/// decides how that number is kept, which decides whether the bucket can be used from several
/// threads and whether clones share it.
///
/// | Storage | Use from many threads | Clones share tokens | Notes |
/// |---|---|---|---|
/// | [`PaddedAtomicStorage`](crate::PaddedAtomicStorage) | yes | can't be cloned | The default. Best for busy buckets used by many threads |
/// | [`AtomicStorage`](crate::AtomicStorage) | yes | can't be cloned | Smaller; fine when there are many buckets that aren't very busy |
/// | [`PaddedAtomicSharedStorage`](crate::PaddedAtomicSharedStorage) | yes | yes | Behind [`SharedTokenBucket`](crate::SharedTokenBucket) |
/// | [`AtomicSharedStorage`](crate::AtomicSharedStorage) | yes | yes | Smaller shared variant |
/// | [`LocalStorage`](crate::LocalStorage) | no | can't be cloned | Fastest; for a bucket used by one thread |
///
/// A bucket with non-shared storage can still be shared through a reference or an `Arc`; the
/// shared storages are convenient when you want to hand out owned copies of the same bucket.
///
/// This trait is sealed: the storages above are the only implementations.
pub trait TimeStorage: crate::private::Sealed {
    /// Creates the storage holding `zero_time`.
    fn new(zero_time: f64) -> Self;
    /// Reads the stored zero time.
    fn load(&self) -> f64;
    /// Replaces the stored zero time.
    fn store(&self, value: f64);
    /// Replaces the zero time with `new` if it is still `current`; otherwise returns the
    /// value it found. May fail spuriously, so callers retry in a loop.
    fn compare_exchange_weak(&self, current: f64, new: f64) -> Result<(), f64>;
}

pub(crate) enum TokenAcquisition {
    Acquired(f64),
    // the zero time at which the bucket is empty
    ZeroedAt(f64),
}

/// Primitive storage for a token bucket using a policy provided timestamp.
/// The timestamp represents the "origin time" at which the bucket would have
/// zero tokens.
///
/// Heavily inspired by folly's TokenBucket algorithm.
#[derive(Debug, Clone)]
pub(crate) struct TokenBucketStorage<S> {
    pub(crate) inner: S,
}

impl<S: TimeStorage> TokenBucketStorage<S> {
    /// Create a new storage object with the provided zero time.
    pub fn new(storage: S) -> Self {
        Self { inner: storage }
    }

    /// Reset the bucket to a given origin time.
    ///
    /// The origin time is the time at which the token
    pub fn reset(&self, zero_time: f64) {
        self.inner.store(zero_time);
    }

    /// Current token balance given a rate, burst size and current time.
    pub fn balance(&self, rate: f64, burst_size: f64, now: f64) -> f64 {
        debug_assert!(rate > 0.0);
        debug_assert!(burst_size > 0.0);
        let zt = self.inner.load();
        ((now - zt) * rate).min(burst_size)
    }

    /// The callback is given the currently available tokens and returns the
    /// number it would like to consume. The function returns the amount
    /// actually consumed.
    pub fn consume<F>(&self, rate: f64, burst_size: f64, now: f64, f: F) -> f64
    where
        F: Fn(f64) -> f64,
    {
        debug_assert!(rate > 0.0);
        debug_assert!(burst_size > 0.0);
        let mut zero_time_old = self.inner.load();
        loop {
            let available = ((now - zero_time_old) * rate).min(burst_size);
            let consumed = f(available);
            if consumed == 0.0 {
                return 0.0;
            }
            let tokens_new = available - consumed;
            let zero_time_new = now - tokens_new / rate;
            match self
                .inner
                .compare_exchange_weak(zero_time_old, zero_time_new)
            {
                Ok(()) => return consumed,
                Err(actual_zero_time) => {
                    zero_time_old = actual_zero_time;
                }
            }
        }
    }

    /// The callback is given the currently available tokens and returns the
    /// number it would like to consume. The function returns the amount
    /// actually consumed.
    pub fn consume2<F>(&self, rate: f64, burst_size: f64, now: f64, f: F) -> TokenAcquisition
    where
        F: Fn(f64) -> f64,
    {
        debug_assert!(rate > 0.0);
        debug_assert!(burst_size > 0.0);
        let mut zero_time_old = self.inner.load();
        loop {
            let available = ((now - zero_time_old) * rate).min(burst_size);
            let consumed = f(available);
            if consumed == 0.0 {
                return TokenAcquisition::ZeroedAt(zero_time_old);
            }
            let tokens_new = available - consumed;
            let zero_time_new = now - tokens_new / rate;
            match self
                .inner
                .compare_exchange_weak(zero_time_old, zero_time_new)
            {
                Ok(()) => return TokenAcquisition::Acquired(consumed),
                Err(actual_zero_time) => {
                    zero_time_old = actual_zero_time;
                }
            }
        }
    }

    /// Return tokens back to the bucket (tokens can be negative to borrow
    /// from the future). The resulting zero time is returned in fractional seconds.
    pub fn return_tokens(&self, tokens: f64, rate: f64) -> f64 {
        debug_assert!(rate > 0.0);
        let mut zero_time_old = self.inner.load();
        loop {
            let zero_time_new = zero_time_old - tokens / rate;

            match self
                .inner
                .compare_exchange_weak(zero_time_old, zero_time_new)
            {
                Ok(()) => return zero_time_new,
                Err(actual_zero_time) => {
                    zero_time_old = actual_zero_time;
                }
            }
        }
    }

    /// Time when the bucket will have `target` tokens available.
    pub fn time_when_bucket(&self, rate: f64, target: f64) -> f64 {
        self.inner.load() + target / rate
    }
}
