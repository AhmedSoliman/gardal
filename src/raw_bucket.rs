use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::time::Duration;

use likely_stable::unlikely;

use crate::clock::Nanos;
use crate::error::{ExceededBurstCapacity, RateLimited, TryConsumeError};
use crate::storage::{TimeStorage, TokenAcquisition, TokenBucketStorage};
use crate::{Clock, Limit, Tokens};

/// A token bucket without its clock and limit: only the bucket's state.
///
/// [`TokenBucket`](crate::TokenBucket) bundles a `RawTokenBucket` with a clock and a
/// [`Limit`]. Use the raw version when you keep many buckets that share the same clock and
/// limit, for example one bucket per user: each bucket then costs only its state (a single
/// `f64`), and you pass the clock and the limit to every call.
///
/// Always pass the same clock, and the limit the bucket is currently used with. Otherwise the
/// methods behave like their [`TokenBucket`](crate::TokenBucket) counterparts.
///
/// ```rust
/// use std::collections::HashMap;
/// use std::num::NonZeroU32;
///
/// use gardal::{Limit, LocalStorage, RawTokenBucket, StdClock};
///
/// let clock = StdClock;
/// let limit = Limit::per_second(NonZeroU32::new(5).unwrap());
/// let mut per_user: HashMap<&str, RawTokenBucket<LocalStorage, StdClock>> = HashMap::new();
///
/// let alice = per_user
///     .entry("alice")
///     .or_insert_with(|| RawTokenBucket::new(&clock, &limit));
/// assert!(alice.consume_one(&clock, &limit).is_some());
/// ```
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
    /// Creates a **full** bucket, like [`TokenBucket::new`](crate::TokenBucket::new).
    pub fn new(clock: &C, limit: &Limit) -> Self {
        // As if the bucket had been idle for twice the time it takes to fill up: the balance
        // is capped at the burst, and the extra margin keeps rounding from leaving the bucket a
        // hair short of full.
        Self::with_zero_time(clock.now() - 2.0 * limit.burst / limit.rate)
    }

    /// Creates an **empty** bucket that fills up at the limit's rate, like
    /// [`TokenBucket::new_empty`](crate::TokenBucket::new_empty).
    pub fn new_empty(clock: &C) -> Self {
        Self::with_zero_time(clock.now())
    }

    /// The bucket's complete state, as a single number.
    ///
    /// See [`TokenBucket::get_zero_time`](crate::TokenBucket::get_zero_time).
    pub fn get_zero_time(&self) -> f64 {
        self.bucket.inner.load()
    }

    /// Creates a bucket from a saved zero time.
    ///
    /// Pass a value from [`get_zero_time`](Self::get_zero_time) to restore a bucket.
    pub fn with_zero_time(time_point: f64) -> Self {
        let storage = S::new(time_point);
        Self {
            bucket: TokenBucketStorage::new(storage),
            _clock: PhantomData,
        }
    }

    /// Switches the bucket from `current_limit` to `new_limit`, keeping the tokens (or the
    /// debt) it currently has.
    ///
    /// See [`TokenBucket::reset`](crate::TokenBucket::reset). `current_limit` must be the limit
    /// the bucket was used with until now; it's needed to know how many tokens the bucket holds.
    pub fn reset(&mut self, clock: &C, current_limit: &Limit, new_limit: &Limit) {
        let now = clock.now();
        let balance = self
            .bucket
            .balance(current_limit.rate, current_limit.burst, now);
        self.set_capacity(balance, now, new_limit.rate);
    }

    /// Sets the bucket's balance to `tokens` at time `now`.
    pub(crate) fn set_capacity(&mut self, tokens: f64, now: f64, rate: f64) {
        self.bucket.reset(now - tokens / rate);
    }

    /// Takes exactly `to_consume` tokens, or nothing.
    ///
    /// See [`TokenBucket::consume`](crate::TokenBucket::consume).
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

    /// Takes one token, or nothing. See [`TokenBucket::consume_one`](crate::TokenBucket::consume_one).
    #[must_use]
    pub fn consume_one(&self, clock: &C, limit: &Limit) -> Option<Tokens> {
        self.consume(NonZeroU32::MIN, clock, limit)
    }

    /// Takes one token, or returns how long to wait.
    ///
    /// See [`TokenBucket::try_consume_one`](crate::TokenBucket::try_consume_one).
    pub fn try_consume_one(&self, clock: &C, limit: &Limit) -> Result<Tokens, RateLimited> {
        // The burst capacity is always at least one token, so a single token can always
        // be granted eventually.
        self.try_consume_inner(1.0, clock.now(), limit)
    }

    /// Takes exactly `to_consume` tokens, or returns how long to wait.
    ///
    /// See [`TokenBucket::try_consume`](crate::TokenBucket::try_consume).
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

    /// Takes as many whole tokens as the bucket has, up to `to_consume`.
    ///
    /// See [`TokenBucket::saturating_consume`](crate::TokenBucket::saturating_consume).
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

    /// Puts tokens back into the bucket.
    ///
    /// See [`TokenBucket::add_tokens`](crate::TokenBucket::add_tokens).
    pub fn add_tokens(&self, tokens: impl Into<f64>, limit: &Limit) {
        let tokens = tokens.into();
        debug_assert!(tokens > 0.0);
        self.bucket.return_tokens(tokens, limit.rate);
    }

    /// Takes `to_consume` tokens, borrowing if needed, and returns how long to wait.
    ///
    /// See [`TokenBucket::consume_with_borrow`](crate::TokenBucket::consume_with_borrow).
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

    /// Like [`consume_with_borrow`](Self::consume_with_borrow), but caps the request at the burst.
    ///
    /// See [`TokenBucket::saturating_consume_with_borrow`](crate::TokenBucket::saturating_consume_with_borrow).
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

    /// The number of tokens that can be taken right now (never negative).
    ///
    /// See [`TokenBucket::available`](crate::TokenBucket::available).
    pub fn available(&self, clock: &C, limit: &Limit) -> f64 {
        self.balance(clock, limit).max(0.0)
    }

    /// The bucket's balance, negative while in debt.
    ///
    /// See [`TokenBucket::balance`](crate::TokenBucket::balance).
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
