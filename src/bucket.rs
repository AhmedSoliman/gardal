use std::num::NonZeroU32;
use std::time::Duration;

use likely_stable::unlikely;

use crate::clock::Nanos;
use crate::error::{ExceededBurstCapacity, RateLimited};
use crate::storage::atomic::AtomicStorage;
use crate::storage::padded_atomic::PaddedAtomicStorage;
use crate::storage::{TimeStorage, TokenAcquisition, TokenBucketStorage};
use crate::{Clock, RateLimit, StdClock, Tokens};

/// Dynamic token bucket using pluggable storage strategy
#[derive(Clone)]
pub struct TokenBucket<S = PaddedAtomicStorage, C = StdClock> {
    bucket: TokenBucketStorage<S>,
    clock: C,
    limit: RateLimit,
}

impl TokenBucket<AtomicStorage, StdClock> {
    /// Create a token bucket with a fixed rate and burst size using the given storage policy.
    pub fn new(limit: RateLimit) -> Self {
        TokenBucket::<AtomicStorage>::from_parts(limit, StdClock::default())
    }
}

impl<C: Clock> TokenBucket<PaddedAtomicStorage, C> {
    /// Create a token bucket with a fixed rate and burst size using the given clock implementation
    pub fn with_clock(limit: RateLimit, clock: C) -> Self {
        let storage = PaddedAtomicStorage::new(clock.now());
        Self {
            bucket: TokenBucketStorage::<PaddedAtomicStorage>::new(storage),
            clock,
            limit,
        }
    }
}

impl<S: TimeStorage, C: Clock> TokenBucket<S, C> {
    /// Create a token bucket with a fixed rate and burst size using the given clock implementation
    pub fn from_parts(limit: RateLimit, clock: C) -> Self {
        // let storage = S::new(clock.now());
        let storage = S::new(0.0);
        Self {
            bucket: TokenBucketStorage::new(storage),
            clock,
            limit,
        }
    }

    /// Change rate and burst size.
    pub fn reset(&mut self, limit: RateLimit) {
        let now = self.clock.now();
        let available = self
            .bucket
            .balance(self.limit.rate, self.limit.burst, now)
            .max(0.0);
        self.limit = limit;
        self.set_capacity(available, now);
    }

    /// Consume self and resets the bucket to the given rate limit
    pub fn update_limit(self, limit: RateLimit) -> Self {
        let now = self.clock.now();
        let available = self
            .bucket
            .balance(self.limit.rate, self.limit.burst, now)
            .max(0.0);
        let mut new = Self {
            bucket: self.bucket,
            clock: self.clock,
            limit,
        };
        new.set_capacity(available, now);
        new
    }

    /// Set the number of tokens currently available in the bucket.
    fn set_capacity(&mut self, tokens: f64, now: f64) {
        self.bucket.reset(now - tokens / self.limit.rate);
    }

    /// Attempt to consume exactly `to_consume` tokens. The fastest way to consume tokens from the
    /// bucket.
    ///
    /// If you also need an estimate of how long you'll need to wait to consume the tokens if the
    /// bucket is already full (or in debt), use [`Self::try_consume()`] instead.
    pub fn consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        let now = self.clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;

        let consumed = self
            .bucket
            .consume(self.limit.rate, self.limit.burst, now, |avail| {
                if avail < to_consume { 0.0 } else { to_consume }
            });
        Tokens::new_checked(consumed)
    }

    /// Consume one token
    pub fn consume_one(&self) -> Option<Tokens> {
        self.consume(NonZeroU32::new(1u32).unwrap())
    }

    /// Try consume one token
    pub fn try_consume_one(&self) -> Result<Tokens, RateLimited> {
        self.try_consume(NonZeroU32::new(1u32).unwrap())
    }

    /// Attempt to consume tokens.
    pub fn try_consume(&self, to_consume: impl Into<NonZeroU32>) -> Result<Tokens, RateLimited> {
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;
        let now = self.clock.now();
        let consumed = self
            .bucket
            .consume2(self.limit.rate, self.limit.burst, now, |avail| {
                if avail < to_consume { 0.0 } else { to_consume }
            });
        match consumed {
            TokenAcquisition::Acquired(consumed) => Ok(Tokens::new_unchecked(consumed)),
            TokenAcquisition::ZeroedAt(zero_time) => {
                let est_time = zero_time - now + to_consume / self.limit.rate;
                debug_assert!(est_time >= 0.0);
                Err(RateLimited {
                    earliest_retry_time: Nanos::from_secs_f64_unchecked(est_time),
                })
            }
        }
    }

    /// Consume or drain available tokens.
    pub fn saturating_consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        let now = self.clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let to_consume: f64 = to_consume.get() as f64;
        Tokens::new_checked(self.saturating_consume_inner(to_consume, now))
    }

    /// Return unused tokens or manually add tokens to the bucket.
    ///
    /// This will not cause the bucket to overflow. The bucket's capacity (burst size)
    /// is maintained.
    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        let tokens = tokens.into();
        debug_assert!(tokens > 0.0);
        self.bucket.return_tokens(tokens, self.limit.rate);
    }

    /// Borrow from the future.
    ///
    /// The returns value is the number of seconds the consumer needs to wait before
    /// the reservation becomes valid.
    pub fn consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> Result<Option<Nanos>, ExceededBurstCapacity> {
        let now = self.clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let mut to_consume: f64 = to_consume.get() as f64;
        if unlikely(self.limit.burst < to_consume) {
            return Err(ExceededBurstCapacity);
        }
        while to_consume > 0.0 {
            let consumed = self.saturating_consume_inner(to_consume, now);
            if consumed > 0.0 {
                to_consume -= consumed;
            } else {
                self.bucket.return_tokens(-to_consume, self.limit.rate);
                let debt_paid = self.bucket.time_when_bucket(self.limit.rate, 0.0);
                let nap_time = (debt_paid - now).max(0.0);
                return Ok(Nanos::new_checked(nap_time));
            }
        }
        Ok(None)
    }

    /// Saturating borrow from the future
    ///
    /// Returns the tokens drained and the needed wait time before the reservation becomes valid.
    pub fn saturating_consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> (Option<Tokens>, Duration) {
        let now = self.clock.now();
        let to_consume: NonZeroU32 = to_consume.into();
        let mut to_consume: f64 = to_consume.get() as f64;
        to_consume = to_consume.min(self.limit.burst);
        let actual_to_be_consumed = to_consume;
        while to_consume > 0.0 {
            let consumed = self.saturating_consume_inner(to_consume, now);
            if consumed > 0.0 {
                to_consume -= consumed;
            } else {
                self.bucket.return_tokens(-to_consume, self.limit.rate);
                let debt_paid = self.bucket.time_when_bucket(self.limit.rate, 0.0);
                let nap_time = (debt_paid - now).max(0.0);
                return (
                    Tokens::new_checked(actual_to_be_consumed),
                    Duration::from_secs_f64(nap_time),
                );
            }
        }
        (None, Duration::ZERO)
    }

    /// Tokens available now (zero if in debt).
    pub fn available(&self) -> f64 {
        self.balance().max(0.0)
    }

    /// Token balance (may be negative if in debt).
    pub fn balance(&self) -> f64 {
        self.bucket
            .balance(self.limit.rate, self.limit.burst, self.clock.now())
    }

    pub fn limit(&self) -> &RateLimit {
        &self.limit
    }

    fn saturating_consume_inner(&self, to_consume: f64, now: f64) -> f64 {
        self.bucket
            .consume(self.limit.rate, self.limit.burst, now, |avail| {
                avail.max(0.0).min(to_consume)
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nonzero_ext::nonzero;

    use crate::clock::ManualClock;
    use crate::storage::padded_atomic::PaddedAtomicStorage;

    use super::*;

    #[test]
    fn basics() {
        let clock = Arc::new(ManualClock::default());
        // initially, the bucket is empty
        let limit = RateLimit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // initially empty
        assert!(tb.consume(nonzero!(1u32)).is_none());
        // one seconds later, the bucket is filled up by 10 tokens/s with burst capacity of 20
        // so we should see 10
        clock.advance(1.0);
        assert_eq!(10.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        // one second later
        clock.advance(1.0);
        // after two seconds bucket should be full
        assert_eq!(20.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        // another second wouldn't matter, we have accumulated the burst capacity (bucket is full)
        clock.advance(1.0);
        assert_eq!(20.0, tb.available());
        assert_eq!(tb.balance(), tb.available());

        assert!(tb.consume(nonzero!(5u32)).is_some());
        assert_eq!(tb.balance(), tb.available());
        assert_eq!(15.0, tb.available());
    }

    #[test]
    fn basics_dynamic() {
        let clock = Arc::new(ManualClock::default());
        // initially, the bucket is empty
        let limit = RateLimit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
        let mut tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // initially empty
        assert!(tb.consume(nonzero!(1u32)).is_none());
        // one seconds later, the bucket is filled up by 10 tokens/s with burst capacity of 20
        // so we should see 10
        clock.advance(1.0);
        assert_eq!(10.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        // change the rate, burst, and reset the available tokens in the bucket
        tb.reset(RateLimit::per_second_and_burst(
            nonzero!(1u32),
            nonzero!(50u32),
        ));
        assert_eq!(10.0, tb.available());
        assert_eq!(10.0, tb.balance());
        assert_eq!(1.0, tb.limit().rate_per_second());
        assert_eq!(nonzero!(50u32), tb.limit().burst());
        clock.advance(1.0);
        assert_eq!(11.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
    }

    #[test]
    fn fractional() {
        let clock = Arc::new(ManualClock::default());
        // initially, the bucket is empty
        let limit = RateLimit::per_minute(nonzero!(30u32)).with_burst(nonzero!(20u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // initially empty
        assert!(tb.consume(nonzero!(1u32)).is_none());
        // two seconds later
        clock.advance(2.0);
        assert_eq!(1.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        clock.set(20.0);
        assert_eq!(10.0, tb.available());
        clock.set(40.0);
        assert_eq!(20.0, tb.available());
        clock.set(50.0);
        assert_eq!(20.0, tb.available());
        assert_eq!(tb.balance(), tb.available());

        let tokens = tb.consume(nonzero!(20u32));
        assert!(tokens.is_some());
        assert_eq!(20, tokens.unwrap().as_u64());
        assert_eq!(tb.balance(), tb.available());
        assert_eq!(0.0, tb.available());

        clock.set(53.0);
        assert_eq!(1.5, tb.available());
    }

    #[test]
    fn saturating_consume() {
        let clock = Arc::new(ManualClock::default());
        let limit = RateLimit::per_second_and_burst(nonzero!(5u32), nonzero!(10u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        clock.set(1.0);
        // after one second, we have 5 tokens available, so that's what we can consume, but we can
        // use saturating consume when requesting more than we can
        let drained = tb.saturating_consume(nonzero!(8u32));
        assert_eq!(drained.unwrap().as_u64(), 5);
        assert_eq!(0.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        let drained = tb.saturating_consume(nonzero!(1u32));
        assert!(drained.is_none());
        assert_eq!(0.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
    }

    #[test]
    fn wait_to_consume() {
        let clock = Arc::new(ManualClock::default());
        let limit = RateLimit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // at t=0 bucket empty; borrow 5 tokens should require waiting for 10.0 seconds. Note that
        // we didn't consume anything as a result.
        match tb.try_consume(nonzero!(5u32)) {
            Ok(_) => panic!("should not be able to consume"),
            Err(RateLimited {
                earliest_retry_time: est_wait_time,
            }) => {
                assert_eq!(10.0, est_wait_time.as_secs_f64());
            }
        }
    }

    #[test]
    fn borrow_future() {
        let clock = Arc::new(ManualClock::default());
        let limit = RateLimit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // at t=0 bucket empty; borrow 5 tokens should require waiting for 10.0 seconds
        let maybe_wait = tb.consume_with_borrow(nonzero!(5u32));
        assert_eq!(10.0, maybe_wait.unwrap().unwrap().as_secs_f64());
        assert_eq!(0.0, tb.available());
        // balance is negative because we borrowed 5 from the future already. We are in debt!
        assert_eq!(-5.0, tb.balance());
        // at t=3, we have accumulated 1.5 tokens, but remember that we borrowed 5 tokens already, so
        // we can't consume anything, our balance though goes to -3.5
        clock.set(3.0);
        assert_eq!(0.0, tb.available());
        assert_eq!(-3.5, tb.balance());
        // can't consume anything
        assert!(tb.consume(nonzero!(1u32)).is_none());
        assert_eq!(0.0, tb.available());
        assert_eq!(-3.5, tb.balance());
        // can we consume from the future again? yes, yes as long as we are borrowing less than
        // burst capacity.
        let maybe_wait = tb.consume_with_borrow(nonzero!(5u32));
        assert_eq!(0.0, tb.available());
        // balance is -3.5, we consume 5 more, it goes to -8.5
        assert_eq!(-8.5, tb.balance());
        // we need 17 seconds for the 8.5 tokens to be valid
        assert_eq!(17.0, maybe_wait.unwrap().unwrap().as_secs_f64());
        // we can't borrow more than our burst capacity at a time
        assert!(tb.consume_with_borrow(nonzero!(11u32)).is_err());
        assert_eq!(-8.5, tb.balance());
        // how far can we borrow from the future, but slowly?
        // indefinitely!
        let maybe_wait = tb.consume_with_borrow(nonzero!(5u32));
        assert_eq!(27.0, maybe_wait.unwrap().unwrap().as_secs_f64());
        assert_eq!(0.0, tb.available());
        assert_eq!(-13.5, tb.balance());
        // but we can return the tokens we borrowed if we didn't use them
        tb.add_tokens(10.0);
        assert_eq!(0.0, tb.available());
        assert_eq!(-3.5, tb.balance());
        tb.add_tokens(8.0);
        assert_eq!(4.5, tb.available());
        assert_eq!(4.5, tb.balance());
        tb.add_tokens(100.0);
        // can we return more than burst capacity? Nope! the bucket can't overflow.
        assert_eq!(10.0, tb.available());
        assert_eq!(10.0, tb.balance());
    }

    #[test]
    fn concurrent_consume_owned() {
        // shared with arc and atomic (owned)
        let clock = Arc::new(ManualClock::default());
        let limit = RateLimit::per_second_and_burst(nonzero!(1000u32), nonzero!(10_000u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        clock.set(10.0);
        let tb = std::sync::Arc::new(tb);
        std::thread::scope(|s| {
            // 4 threads, each consuming 2000 tokens, bursting to 8000 out of the 10k burst
            // capacity
            for _ in 0..4 {
                let tb = tb.clone();
                s.spawn(move || {
                    for _ in 0..2000 {
                        assert_eq!(1, tb.consume(nonzero!(1u32)).unwrap().as_u64());
                    }
                });
            }
        });
        let remaining = tb.available();
        assert!((remaining - 2000.0).abs() < 1e-9);
    }

    #[test]
    fn concurrent_consume() {
        // shared with atomic (reference)
        let clock = Arc::new(ManualClock::default());
        let limit = RateLimit::per_second_and_burst(nonzero!(1000u32), nonzero!(10_000u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        std::thread::scope(|s| {
            clock.set(10.0);
            // 4 threads, each consuming 2000 tokens, bursting to 8000 out of the 10k burst
            // capacity
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..2000 {
                        assert_eq!(1, tb.consume(nonzero!(1u32)).unwrap().as_u64());
                    }
                });
            }
        });
        let remaining = tb.available();
        dbg!(remaining);
        assert!((remaining - 2000.0).abs() < 1e-9);
    }
}
