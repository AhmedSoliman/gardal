use std::num::NonZeroU32;
use std::time::Duration;

use crate::clock::Nanos;
use crate::error::{ExceededBurstCapacity, RateLimited};
use crate::storage::TimeStorage;
use crate::storage::padded_atomic::PaddedAtomicStorage;
use crate::{Clock, Limit, RawTokenBucket, StdClock, Tokens};

pub const UNLIMITED_BUCKET: Option<TokenBucket> = const { None };

/// A token bucket rate limiter with configurable storage and clock implementations.
///
/// The token bucket algorithm allows for controlled rate limiting with burst capacity.
/// Tokens are added to the bucket at a steady rate, and operations consume tokens.
/// When the bucket is empty, operations are rate limited.
///
/// # Type Parameters
///
/// - `S`: Storage strategy (default: [`PaddedAtomicStorage`] for concurrent access)
/// - `C`: Clock implementation (use [`StdClock`] for standard library timing)
///
/// # Examples
///
/// ```rust
/// use gardal::{LocalTokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second_and_burst(
///     NonZeroU32::new(10).unwrap(),
///     NonZeroU32::new(20).unwrap()
/// );
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// // Try to consume 5 tokens
/// if let Some(tokens) = bucket.consume(NonZeroU32::new(5).unwrap()) {
///     println!("Successfully consumed {} tokens", tokens.as_u64());
/// }
/// ```
#[derive(Clone)]
pub struct TokenBucket<S = PaddedAtomicStorage, C = StdClock> {
    bucket: RawTokenBucket<S, C>,
    clock: C,
    limit: Limit,
}

impl<C: Clock> TokenBucket<PaddedAtomicStorage, C> {
    /// Creates a new token bucket with a custom clock implementation.
    ///
    /// Use this when you need a specific timing source, such as `FastClock` for high-performance
    /// scenarios or `ManualClock` for testing.
    ///
    /// # Arguments
    ///
    /// * `limit` - The rate and burst configuration for the bucket
    /// * `clock` - The clock implementation to use for timing
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{TokenBucket, Limit, ManualClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let clock = ManualClock::new(0.0);
    /// let bucket = TokenBucket::with_clock(limit, clock);
    /// ```
    pub fn with_clock(limit: Limit, clock: C) -> Self {
        Self {
            bucket: RawTokenBucket::<PaddedAtomicStorage, C>::new(&clock),
            clock,
            limit,
        }
    }
}

impl<S: TimeStorage, C: Clock> TokenBucket<S, C> {
    /// Creates a token bucket
    ///
    /// The time datum defaults to 0.0 which means that the bucket will be initially full
    /// in case you are using one of the standard clocks provided by this crate.
    ///
    /// # Arguments
    ///
    /// * `limit` - The rate and burst configuration for the bucket
    /// * `clock` - The clock implementation to use for timing.
    pub fn new(limit: Limit, clock: C) -> Self {
        Self {
            bucket: RawTokenBucket::with_zero_time(0.0),
            clock,
            limit,
        }
    }

    /// Creates a token bucket from custom storage and clock implementations.
    ///
    /// This provides maximum flexibility for advanced use cases requiring specific
    /// storage strategies or clock implementations.
    ///
    /// # Arguments
    ///
    /// * `limit` - The rate and burst configuration for the bucket
    /// * `clock` - The clock implementation to use for timing. It will use the clock's
    ///   datum point as the zero time. This means that the bucket will be considered
    ///   empty initially.
    pub fn with_datum(limit: Limit, clock: C) -> Self {
        Self {
            bucket: RawTokenBucket::new(&clock),
            clock,
            limit,
        }
    }

    /// Updates the rate limit configuration while preserving available tokens.
    ///
    /// The current token balance is maintained proportionally when changing limits.
    ///
    /// # Arguments
    ///
    /// * `limit` - The new rate and burst configuration
    pub fn reset(&mut self, limit: Limit) {
        let now = self.clock.now();
        let available = self.bucket.balance_at(now, &self.limit);
        self.limit = limit;
        self.bucket.set_capacity(available, now, limit.rate);
    }

    /// Consumes the bucket and returns a new one with updated rate limits.
    ///
    /// Similar to [`reset`](Self::reset) but takes ownership and returns a new bucket.
    ///
    /// # Arguments
    ///
    /// * `limit` - The new rate and burst configuration
    ///
    /// # Returns
    ///
    /// A new token bucket with the updated configuration
    pub fn update_limit(self, limit: Limit) -> Self {
        let now = self.clock.now();
        let available = self.bucket.balance_at(now, &self.limit);
        let mut new = Self {
            bucket: self.bucket,
            clock: self.clock,
            limit,
        };
        new.bucket.set_capacity(available, now, new.limit.rate);
        new
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
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Successfully consumed tokens
    /// * `None` - Insufficient tokens available
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{LocalTokenBucket, Limit, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    ///
    /// if let Some(tokens) = bucket.consume(NonZeroU32::new(5).unwrap()) {
    ///     println!("Consumed {} tokens", tokens.as_u64());
    /// } else {
    ///     println!("Not enough tokens available");
    /// }
    /// ```
    pub fn consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        self.bucket.consume(to_consume, &self.clock, &self.limit)
    }

    /// Attempts to consume exactly one token.
    ///
    /// Convenience method equivalent to `consume(1)`.
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Successfully consumed one token
    /// * `None` - No tokens available
    pub fn consume_one(&self) -> Option<Tokens> {
        self.bucket
            .consume(NonZeroU32::new(1u32).unwrap(), &self.clock, &self.limit)
    }

    /// Attempts to consume one token with wait time information.
    ///
    /// Convenience method equivalent to `try_consume(1)`.
    ///
    /// # Returns
    ///
    /// * `Ok(Tokens)` - Successfully consumed one token
    /// * `Err(RateLimited)` - Rate limited with suggested wait time
    pub fn try_consume_one(&self) -> Result<Tokens, RateLimited> {
        self.bucket
            .try_consume(NonZeroU32::new(1u32).unwrap(), &self.clock, &self.limit)
    }

    /// Attempts to consume tokens with detailed rate limiting information.
    ///
    /// Unlike [`consume`](Self::consume), this method provides an estimate of how long
    /// to wait before retrying when tokens are unavailable.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume
    ///
    /// # Returns
    ///
    /// * `Ok(Tokens)` - Successfully consumed tokens
    /// * `Err(RateLimited)` - Rate limited with suggested retry time
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{LocalTokenBucket, Limit, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    ///
    /// match bucket.try_consume(NonZeroU32::new(5).unwrap()) {
    ///     Ok(tokens) => println!("Consumed {} tokens", tokens.as_u64()),
    ///     Err(rate_limited) => {
    ///         let wait_time = rate_limited.earliest_retry_after();
    ///         println!("Rate limited, retry in {:?}", wait_time);
    ///     }
    /// }
    /// ```
    pub fn try_consume(&self, to_consume: impl Into<NonZeroU32>) -> Result<Tokens, RateLimited> {
        self.bucket
            .try_consume(to_consume, &self.clock, &self.limit)
    }

    /// Consumes up to the requested number of tokens, returning whatever is available.
    ///
    /// This method will consume as many tokens as possible up to the requested amount,
    /// without waiting. Returns `None` if no tokens are available.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Maximum number of tokens to consume
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` - Number of tokens actually consumed (may be less than requested)
    /// * `None` - No tokens available
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{LocalTokenBucket, Limit, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    ///
    /// // Request 100 tokens, but only get what's available
    /// if let Some(tokens) = bucket.saturating_consume(NonZeroU32::new(100).unwrap()) {
    ///     println!("Got {} tokens (may be less than 100)", tokens.as_u64());
    /// }
    /// ```
    pub fn saturating_consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        self.bucket
            .saturating_consume(to_consume, &self.clock, &self.limit)
    }

    /// Returns unused tokens to the bucket or manually adds tokens.
    ///
    /// This operation respects the bucket's burst capacity and will not cause overflow.
    /// Useful for returning tokens from cancelled operations.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Number of tokens to add back to the bucket
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{LocalTokenBucket, Limit, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    ///
    /// // Return 5 tokens to the bucket
    /// bucket.add_tokens(5.0);
    /// ```
    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        self.bucket.add_tokens(tokens, &self.limit)
    }

    /// Consumes tokens by borrowing from future capacity.
    ///
    /// This allows consuming more tokens than currently available by going into "debt".
    /// The bucket will need time to replenish before more tokens can be consumed.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume
    ///
    /// # Returns
    ///
    /// * `Ok(None)` - Tokens consumed immediately without borrowing
    /// * `Ok(Some(duration))` - Tokens consumed with borrowing; wait time until debt is paid
    /// * `Err(ExceededBurstCapacity)` - Cannot borrow more than burst capacity
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{LocalTokenBucket, Limit, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second_and_burst(
    ///     NonZeroU32::new(10).unwrap(),
    ///     NonZeroU32::new(20).unwrap()
    /// );
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    ///
    /// match bucket.consume_with_borrow(NonZeroU32::new(15).unwrap()) {
    ///     Ok(Some(wait_time)) => {
    ///         println!("Borrowed tokens, wait {:?} before next operation", wait_time);
    ///     }
    ///     Ok(None) => println!("Consumed without borrowing"),
    ///     Err(_) => println!("Cannot borrow more than burst capacity"),
    /// }
    /// ```
    pub fn consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> Result<Option<Nanos>, ExceededBurstCapacity> {
        self.bucket
            .consume_with_borrow(to_consume, &self.clock, &self.limit)
    }

    /// Consumes tokens with borrowing, limited to burst capacity.
    ///
    /// Similar to [`consume_with_borrow`](Self::consume_with_borrow) but automatically
    /// limits the request to the burst capacity instead of returning an error.
    ///
    /// # Arguments
    ///
    /// * `to_consume` - Number of tokens to consume (capped at burst capacity)
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// * `Option<Tokens>` - Number of tokens consumed (None if no borrowing occurred)
    /// * `Duration` - Wait time until the debt is paid (zero if no borrowing)
    pub fn saturating_consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> (Option<Tokens>, Duration) {
        self.bucket
            .saturating_consume_with_borrow(to_consume, &self.clock, &self.limit)
    }

    /// Returns the number of tokens currently available for consumption.
    ///
    /// This value is always non-negative. If the bucket is in debt from borrowing,
    /// this returns zero.
    ///
    /// # Returns
    ///
    /// Number of tokens available for immediate consumption
    pub fn available(&self) -> f64 {
        self.bucket.available(&self.clock, &self.limit)
    }

    /// Returns the current token balance, which may be negative if in debt.
    ///
    /// Unlike [`available`](Self::available), this can return negative values
    /// when tokens have been borrowed from future capacity.
    ///
    /// # Returns
    ///
    /// Current token balance (negative indicates debt)
    pub fn balance(&self) -> f64 {
        self.bucket.balance(&self.clock, &self.limit)
    }

    /// Returns a reference to the current rate limit configuration.
    ///
    /// # Returns
    ///
    /// Reference to the [`Limit`] configuration
    pub fn limit(&self) -> &Limit {
        &self.limit
    }

    /// Returns the internal timepoint of the bucket.
    pub fn get_zero_time(&self) -> f64 {
        self.bucket.get_zero_time()
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
        let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
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
        let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
        let mut tb = TokenBucket::<PaddedAtomicStorage, _>::with_clock(limit, Arc::clone(&clock));
        // initially empty
        assert!(tb.consume(nonzero!(1u32)).is_none());
        // one seconds later, the bucket is filled up by 10 tokens/s with burst capacity of 20
        // so we should see 10
        clock.advance(1.0);
        assert_eq!(10.0, tb.available());
        assert_eq!(tb.balance(), tb.available());
        // change the rate, burst, and reset the available tokens in the bucket
        tb.reset(Limit::per_second_and_burst(nonzero!(1u32), nonzero!(50u32)));
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
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(20u32));
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
        let limit = Limit::per_second_and_burst(nonzero!(5u32), nonzero!(10u32));
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
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
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
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
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
        let limit = Limit::per_second_and_burst(nonzero!(1000u32), nonzero!(10_000u32));
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
        let limit = Limit::per_second_and_burst(nonzero!(1000u32), nonzero!(10_000u32));
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
