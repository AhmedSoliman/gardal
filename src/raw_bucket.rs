use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::time::Duration;

use likely_stable::unlikely;

use crate::clock::Nanos;
use crate::error::{ExceededBurstCapacity, RateLimited, TryConsumeError};
use crate::storage::{TimeStorage, TokenAcquisition, TokenBucketStorage};
use crate::{Clock, Limit, Tokens};

/// The token bucket state, without the clock and the limit.
///
/// This is the building block behind [`TokenBucket`](crate::TokenBucket). It only stores
/// the bucket's zero time; the clock and the [`Limit`] are passed to every call. Use it
/// to embed many buckets that share a clock and a limit without storing them per bucket.
#[derive(Clone)]
pub struct RawTokenBucket<S, C> {
    bucket: TokenBucketStorage<S>,
    _clock: PhantomData<C>,
}

impl<S: TimeStorage, C> std::fmt::Debug for RawTokenBucket<S, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawTokenBucket")
            .field("zero_time", &self.bucket.inner.load())
            .finish()
    }
}

impl<S: TimeStorage, C: Clock> RawTokenBucket<S, C> {
    /// Creates a token bucket from custom storage and clock implementations.
    ///
    /// This provides maximum flexibility for advanced use cases requiring specific
    /// storage strategies or clock implementations.
    ///
    /// # Arguments
    ///
    /// * `clock` - The clock implementation to use for setting the origin datum time
    pub fn new(clock: &C) -> Self {
        let storage = S::new(clock.datum());
        Self {
            bucket: TokenBucketStorage::new(storage),
            _clock: PhantomData,
        }
    }

    /// Returns the internal timepoint of the bucket.
    ///
    /// In combination with [`with_zero_time`](Self::with_zero_time), this can be used to
    /// create a new bucket with a different origin time. For instance, if you'd like
    /// to reconstruct a bucket with the exact same state as another.
    pub fn get_zero_time(&self) -> f64 {
        self.bucket.inner.load()
    }

    /// Creates a token bucket whose zero time is `time_point`.
    ///
    /// The zero time is the moment at which the bucket held zero tokens. Pass a value
    /// obtained from [`get_zero_time`](Self::get_zero_time) to restore a bucket's state.
    pub fn with_zero_time(time_point: f64) -> Self {
        let storage = S::new(time_point);
        Self {
            bucket: TokenBucketStorage::new(storage),
            _clock: PhantomData,
        }
    }

    /// Updates the rate limit configuration while preserving available tokens.
    ///
    /// The current token balance is maintained proportionally when changing limits.
    ///
    /// # Arguments
    ///
    /// * `clock` - The clock used originally to create the bucket
    /// * `limit` - The new rate and burst configuration
    pub fn reset(&mut self, clock: &C, limit: &Limit) {
        let now = clock.now();
        let available = self.bucket.balance(limit.rate, limit.burst, now).max(0.0);
        self.set_capacity(available, now, limit.rate);
    }

    /// Set the number of tokens currently available in the bucket.
    pub(crate) fn set_capacity(&mut self, tokens: f64, now: f64, rate: f64) {
        self.bucket.reset(now - tokens / rate);
    }

    /// Attempts to consume exactly the specified number of tokens.
    ///
    /// This is the fastest consumption method. Returns `Some(tokens)` if successful,
    /// or `None` if insufficient tokens are available.
    ///
    /// For wait time estimates when tokens are unavailable, use [`try_consume`](Self::try_consume).
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume
    /// * `clock` - Clock implementation to use for timing
    /// * `limit` - Rate and burst configuration for the bucket
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Successfully consumed tokens
    /// * `None` - Insufficient tokens available
    #[must_use]
    pub fn consume(
        &self,
        to_consume: impl Into<NonZeroU32>,
        clock: &C,
        limit: &Limit,
    ) -> Option<Tokens> {
        let now = clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;

        let consumed = self.bucket.consume(limit.rate, limit.burst, now, |avail| {
            if avail < to_consume { 0.0 } else { to_consume }
        });
        Tokens::new_checked(consumed)
    }

    /// Attempts to consume exactly one token.
    ///
    /// Convenience method equivalent to `consume(1)`.
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Successfully consumed one token
    /// * `None` - No tokens available
    #[must_use]
    pub fn consume_one(&self, clock: &C, limit: &Limit) -> Option<Tokens> {
        self.consume(NonZeroU32::MIN, clock, limit)
    }

    /// Attempts to consume one token with wait time information.
    ///
    /// Convenience method equivalent to `try_consume(1)`.
    ///
    /// # Returns
    ///
    /// * `Ok(Tokens)` - Successfully consumed one token
    /// * `Err(RateLimited)` - Rate limited with suggested wait time
    pub fn try_consume_one(&self, clock: &C, limit: &Limit) -> Result<Tokens, RateLimited> {
        // The burst capacity is always at least one token, so a single token can always
        // be granted eventually.
        self.try_consume_inner(1.0, clock.now(), limit)
    }

    /// Attempts to consume tokens with detailed rate limiting information.
    ///
    /// Unlike [`consume`](Self::consume), this method provides an estimate of how long
    /// to wait before retrying when tokens are unavailable.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume
    /// * `clock` - Clock implementation to use for timing
    /// * `limit` - Rate and burst configuration for the bucket
    ///
    /// # Returns
    ///
    /// * `Ok(Tokens)` - Successfully consumed tokens
    /// * `Err(TryConsumeError::RateLimited(_))` - Rate limited with suggested retry time
    /// * `Err(TryConsumeError::ExceededBurstCapacity(_))` - More tokens were requested than
    ///   the burst capacity, so the request can never succeed
    pub fn try_consume(
        &self,
        to_consume: impl Into<NonZeroU32>,
        clock: &C,
        limit: &Limit,
    ) -> Result<Tokens, TryConsumeError> {
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;
        match self.try_consume_inner(to_consume, clock.now(), limit) {
            Ok(tokens) => Ok(tokens),
            // Checked only once the request failed, so successful calls don't pay for it. A
            // request above the burst capacity always fails since the bucket never holds more.
            Err(_) if unlikely(to_consume > limit.burst) => Err(
                TryConsumeError::ExceededBurstCapacity(ExceededBurstCapacity),
            ),
            Err(rate_limited) => Err(TryConsumeError::RateLimited(rate_limited)),
        }
    }

    #[inline]
    fn try_consume_inner(
        &self,
        to_consume: f64,
        now: f64,
        limit: &Limit,
    ) -> Result<Tokens, RateLimited> {
        let consumed = self.bucket.consume2(limit.rate, limit.burst, now, |avail| {
            if avail < to_consume { 0.0 } else { to_consume }
        });
        match consumed {
            TokenAcquisition::Acquired(consumed) => Ok(Tokens::new_unchecked(consumed)),
            TokenAcquisition::ZeroedAt(zero_time) => {
                // Strictly positive when `to_consume` fits the burst capacity, but rounding can
                // bring it down to zero (whose bit pattern `Nanos` cannot hold) or below; that
                // means the tokens are due now.
                let est_time = (zero_time - now + to_consume / limit.rate).max(f64::MIN_POSITIVE);
                Err(RateLimited {
                    earliest_retry_time: Nanos::from_secs_f64_unchecked(est_time),
                })
            }
        }
    }

    /// Consumes up to the requested number of tokens, returning whatever is available.
    ///
    /// This method will consume as many whole tokens as possible up to the requested amount,
    /// without waiting. A fractional remainder stays in the bucket. Returns `None` if less
    /// than one token is available.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Maximum number of tokens to consume
    /// * `clock` - Clock implementation to use for timing
    /// * `limit` - Rate and burst configuration for the bucket
    ///
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Number of tokens actually consumed (may be less than requested)
    /// * `None` - Less than one token available
    #[must_use]
    pub fn saturating_consume(
        &self,
        to_consume: impl Into<NonZeroU32>,
        clock: &C,
        limit: &Limit,
    ) -> Option<Tokens> {
        let now = clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;
        let consumed = self.bucket.consume(limit.rate, limit.burst, now, |avail| {
            avail.max(0.0).min(to_consume).floor()
        });
        Tokens::new_checked(consumed)
    }

    /// Returns unused tokens to the bucket or manually adds tokens.
    ///
    /// This operation respects the bucket's burst capacity and will not cause overflow.
    /// Useful for returning tokens from cancelled operations.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Number of tokens to add back to the bucket
    /// * `limit` - Rate and burst configuration for the bucket
    pub fn add_tokens(&self, tokens: impl Into<f64>, limit: &Limit) {
        let tokens = tokens.into();
        debug_assert!(tokens > 0.0);
        self.bucket.return_tokens(tokens, limit.rate);
    }

    /// Consumes tokens by borrowing from future capacity.
    ///
    /// This allows consuming more tokens than currently available by going into "debt".
    /// The bucket will need time to replenish before more tokens can be consumed.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume
    /// * `clock` - Clock implementation to use for timing
    /// * `limit` - Rate and burst configuration for the bucket
    ///
    /// # Returns
    ///
    /// * `Ok(None)` - Tokens consumed immediately without borrowing
    /// * `Ok(Some(duration))` - Tokens consumed with borrowing; wait time until debt is paid
    /// * `Err(ExceededBurstCapacity)` - Cannot borrow more than burst capacity
    pub fn consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
        clock: &C,
        limit: &Limit,
    ) -> Result<Option<Duration>, ExceededBurstCapacity> {
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;
        if unlikely(limit.burst < to_consume) {
            return Err(ExceededBurstCapacity);
        }
        let nap_time = self.borrow_inner(to_consume, clock.now(), limit);
        Ok(nap_time
            .filter(|nap_time| *nap_time > 0.0)
            .map(Duration::from_secs_f64))
    }

    /// Consumes tokens with borrowing, limited to burst capacity.
    ///
    /// Similar to [`consume_with_borrow`](Self::consume_with_borrow) but automatically
    /// limits the request to the burst capacity instead of returning an error.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume (capped at burst capacity)
    /// * `clock` - Clock implementation to use for timing
    /// * `limit` - Rate and burst configuration for the bucket
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// * `Option<Tokens>` - Number of tokens consumed (None if no borrowing occurred)
    /// * `Duration` - Wait time until the debt is paid (zero if no borrowing)
    #[must_use]
    pub fn saturating_consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
        clock: &C,
        limit: &Limit,
    ) -> (Option<Tokens>, Duration) {
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = (to_consume.get() as f64).min(limit.burst);
        match self.borrow_inner(to_consume, clock.now(), limit) {
            None => (None, Duration::ZERO),
            Some(nap_time) => (
                Tokens::new_checked(to_consume),
                Duration::from_secs_f64(nap_time),
            ),
        }
    }

    /// Like [`saturating_consume_with_borrow`](Self::saturating_consume_with_borrow), but
    /// only returns the time to wait, in seconds (`0.0` if there is nothing to wait for).
    ///
    /// Used by the throttled streams, which only need the wait time and would rather not
    /// build a [`Duration`] for every item.
    #[cfg(feature = "async")]
    #[inline]
    pub(crate) fn saturating_borrow_nap(
        &self,
        to_consume: NonZeroU32,
        clock: &C,
        limit: &Limit,
    ) -> f64 {
        let to_consume: f64 = (to_consume.get() as f64).min(limit.burst);
        self.borrow_inner(to_consume, clock.now(), limit)
            .unwrap_or(0.0)
    }

    /// Returns the number of tokens currently available for consumption.
    ///
    /// This value is always non-negative. If the bucket is in debt from borrowing,
    /// this returns zero.
    ///
    /// # Returns
    ///
    /// Number of tokens available for immediate consumption
    pub fn available(&self, clock: &C, limit: &Limit) -> f64 {
        self.balance(clock, limit).max(0.0)
    }

    /// Returns the current token balance, which may be negative if in debt.
    ///
    /// Unlike [`available`](Self::available), this can return negative values
    /// when tokens have been borrowed from future capacity.
    ///
    /// # Returns
    ///
    /// Current token balance (negative indicates debt)
    pub fn balance(&self, clock: &C, limit: &Limit) -> f64 {
        self.bucket.balance(limit.rate, limit.burst, clock.now())
    }

    /// Returns the token balance at `time` (in the clock's seconds), which may be
    /// negative if the bucket is in debt.
    pub fn balance_at(&self, time: f64, limit: &Limit) -> f64 {
        self.bucket.balance(limit.rate, limit.burst, time)
    }

    /// Consumes `to_consume` tokens (at most the burst capacity), going into debt for
    /// whatever is not available.
    ///
    /// Returns `None` if no borrowing was needed, otherwise the time in seconds (`>= 0.0`)
    /// until the debt is paid.
    #[inline]
    fn borrow_inner(&self, mut to_consume: f64, now: f64, limit: &Limit) -> Option<f64> {
        while to_consume > 0.0 {
            let consumed = self.saturating_consume_inner(to_consume, now, limit);
            if consumed > 0.0 {
                to_consume -= consumed;
            } else {
                self.bucket.return_tokens(-to_consume, limit.rate);
                let debt_paid = self.bucket.time_when_bucket(limit.rate, 0.0);
                return Some((debt_paid - now).max(0.0));
            }
        }
        None
    }

    #[inline]
    fn saturating_consume_inner(&self, to_consume: f64, now: f64, limit: &Limit) -> f64 {
        self.bucket.consume(limit.rate, limit.burst, now, |avail| {
            avail.max(0.0).min(to_consume)
        })
    }
}
