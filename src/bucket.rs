use std::num::NonZeroU32;
use std::time::Duration;

use crate::error::{ExceededBurstCapacity, RateLimited, TryConsumeError};
use crate::storage::TimeStorage;
use crate::storage::padded_atomic::PaddedAtomicStorage;
use crate::{Clock, Limit, RawTokenBucket, StdClock, Tokens};

/// "No bucket": pass this where an `Option<TokenBucket>` is expected to turn rate limiting
/// off.
///
/// For example, `stream.throttle(UNLIMITED_BUCKET)` (with the `async` feature) returns the
/// stream's items as fast as they come, with no token bucket overhead, while keeping the same
/// stream type as a throttled one.
pub const UNLIMITED_BUCKET: Option<TokenBucket> = const { None };

/// A token bucket rate limiter.
///
/// # How it works
///
/// Picture a bucket that holds tokens. Tokens drip in at a steady rate until the bucket is
/// full. Before doing a unit of work, you take a token out (or several, for bigger work). When
/// the bucket is empty, the work has to wait or be turned away. On average this allows the
/// limit's rate, while still letting short bursts through when the bucket is full. Both
/// numbers come from the [`Limit`].
///
/// Gardal keeps the whole state of a bucket in a single number, so taking tokens is one
/// atomic update: it never blocks and never allocates.
///
/// # Creating a bucket
///
/// You choose how a new bucket starts:
///
/// - [`new`](Self::new) starts **full**: the whole burst can be used right away, as if the
///   bucket had been idle. This is what you want most of the time.
/// - [`new_empty`](Self::new_empty) starts **empty** and fills up at the limit's rate, so
///   nothing gets through until the first token arrives. Use it when a burst right at startup
///   would hurt, for example when many instances of a service start at the same time and would
///   all hit a shared dependency at once.
/// - [`with_zero_time`](Self::with_zero_time) restores a bucket saved with
///   [`get_zero_time`](Self::get_zero_time).
///
/// The two type parameters choose where the bucket's state lives (`S`, see [`TimeStorage`])
/// and how it reads the time (`C`, see [`Clock`]). The defaults, [`PaddedAtomicStorage`] and
/// [`StdClock`], are a good starting point. The aliases
/// [`LocalTokenBucket`](crate::LocalTokenBucket), [`AtomicTokenBucket`](crate::AtomicTokenBucket)
/// and [`SharedTokenBucket`](crate::SharedTokenBucket) cover the common storage choices.
///
/// # Taking tokens
///
/// Pick the method by what should happen when there aren't enough tokens:
///
/// | Method | If tokens are short |
/// |---|---|
/// | [`consume`](Self::consume) | Take nothing, return `None` |
/// | [`try_consume`](Self::try_consume) | Take nothing, return how long to wait |
/// | [`saturating_consume`](Self::saturating_consume) | Take what is there |
/// | [`consume_with_borrow`](Self::consume_with_borrow) | Take them anyway, return how long to wait |
///
/// # Sharing a bucket
///
/// Every method takes `&self`, so a bucket can be used from many threads or tasks at once.
/// Share it with a reference or an [`Arc`](std::sync::Arc), or use
/// [`SharedTokenBucket`](crate::SharedTokenBucket), whose clones all draw from the same
/// tokens. A [`LocalTokenBucket`](crate::LocalTokenBucket) is faster but can only be used
/// from one thread.
///
/// # Example
///
/// Limiting the requests a service accepts, answering "429 Too Many Requests" when the limit
/// is reached:
///
/// ```rust
/// use gardal::{Limit, SharedTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// // 100 requests per second on average, at most 20 back to back.
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap())
///     .with_burst(NonZeroU32::new(20).unwrap());
/// let limiter = SharedTokenBucket::new(limit, StdClock);
///
/// fn handle_request(limiter: &SharedTokenBucket<StdClock>) -> String {
///     match limiter.try_consume_one() {
///         Ok(_) => "200 OK".to_string(),
///         Err(limited) => format!(
///             "429 Too Many Requests (retry after {:?})",
///             limited.earliest_retry_after()
///         ),
///     }
/// }
///
/// assert_eq!(handle_request(&limiter), "200 OK");
/// ```
#[derive(Clone)]
pub struct TokenBucket<S = PaddedAtomicStorage, C = StdClock> {
    bucket: RawTokenBucket<S, C>,
    clock: C,
    limit: Limit,
}

impl<S: TimeStorage, C: Clock> std::fmt::Debug for TokenBucket<S, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBucket")
            .field("limit", &self.limit)
            .field("balance", &self.balance())
            .finish()
    }
}

impl<S: TimeStorage, C: Clock> TokenBucket<S, C> {
    /// Creates a **full** bucket: the whole burst can be used right away.
    ///
    /// This is the usual way to create a bucket. Use [`new_empty`](Self::new_empty) if the
    /// bucket should start empty instead.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    /// assert_eq!(bucket.available(), 10.0);
    /// ```
    pub fn new(limit: Limit, clock: C) -> Self {
        Self {
            bucket: RawTokenBucket::new(&clock, &limit),
            clock,
            limit,
        }
    }

    /// Creates an **empty** bucket that fills up at the limit's rate.
    ///
    /// Nothing can be taken until the first token arrives: with a rate of 10 per second, that
    /// is after 0.1 seconds, and the full burst is only available after `burst / rate`
    /// seconds. Use it to avoid letting a burst through right at startup; use
    /// [`new`](Self::new) to start full.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, ManualClock};
    /// use std::num::NonZeroU32;
    /// use std::sync::Arc;
    ///
    /// let clock = Arc::new(ManualClock::new(0.0));
    /// let bucket = LocalTokenBucket::new_empty(
    ///     Limit::per_second(NonZeroU32::new(10).unwrap()),
    ///     Arc::clone(&clock),
    /// );
    /// assert!(bucket.consume_one().is_none());
    ///
    /// clock.advance(0.1);
    /// assert!(bucket.consume_one().is_some());
    /// ```
    pub fn new_empty(limit: Limit, clock: C) -> Self {
        Self {
            bucket: RawTokenBucket::new_empty(&clock),
            clock,
            limit,
        }
    }

    /// Restores a bucket from a zero time saved with [`get_zero_time`](Self::get_zero_time).
    ///
    /// With the built-in clocks the zero time is a point in wall-clock time, so a bucket can be
    /// saved before a restart (or sent to another process) and restored with the tokens it had,
    /// plus whatever it refilled in the meantime.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = LocalTokenBucket::new(limit, StdClock);
    /// let _ = bucket.consume(NonZeroU32::new(8).unwrap());
    ///
    /// let saved: f64 = bucket.get_zero_time();
    /// // ... later ...
    /// let restored = LocalTokenBucket::with_zero_time(limit, StdClock, saved);
    /// assert!(restored.available() < 3.0);
    /// ```
    pub fn with_zero_time(limit: Limit, clock: C, zero_time: f64) -> Self {
        Self {
            bucket: RawTokenBucket::with_zero_time(zero_time),
            clock,
            limit,
        }
    }

    /// Changes the bucket's limit, keeping the tokens (or the debt) it currently has.
    ///
    /// If the new burst is smaller than the tokens in the bucket, the bucket is capped at the
    /// new burst. See [`update_limit`](Self::update_limit) for a version that takes and
    /// returns the bucket by value.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let mut bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    /// bucket.reset(Limit::per_second(NonZeroU32::new(100).unwrap()));
    /// assert_eq!(bucket.limit().rate_per_second(), 100.0);
    /// ```
    pub fn reset(&mut self, limit: Limit) {
        self.bucket.reset(&self.clock, &self.limit, &limit);
        self.limit = limit;
    }

    /// Returns the bucket with a new limit, keeping the tokens (or the debt) it currently has.
    ///
    /// Same as [`reset`](Self::reset), but takes the bucket by value.
    pub fn update_limit(mut self, limit: Limit) -> Self {
        self.reset(limit);
        self
    }

    /// Takes exactly `to_consume` tokens if the bucket has them; otherwise takes nothing and
    /// returns `None`.
    ///
    /// This is the cheapest check. Use it when work that doesn't fit the limit can simply be
    /// skipped, and use [`try_consume`](Self::try_consume) when you want to know how long to
    /// wait instead.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// // Log at most 10 warnings per second; drop the rest.
    /// let log_limit = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    ///
    /// for i in 0..100 {
    ///     if log_limit.consume(NonZeroU32::MIN).is_some() {
    ///         println!("warning #{i}");
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        self.bucket.consume(to_consume, &self.clock, &self.limit)
    }

    /// Takes one token if the bucket has one. Same as [`consume`](Self::consume) with `1`.
    #[must_use]
    pub fn consume_one(&self) -> Option<Tokens> {
        self.bucket
            .consume(NonZeroU32::MIN, &self.clock, &self.limit)
    }

    /// Takes one token, or returns how long to wait until one is available.
    ///
    /// Same as [`try_consume`](Self::try_consume) with `1`, but the error is always a
    /// [`RateLimited`]: one token always fits in the bucket.
    pub fn try_consume_one(&self) -> Result<Tokens, RateLimited> {
        self.bucket.try_consume_one(&self.clock, &self.limit)
    }

    /// Takes exactly `to_consume` tokens if the bucket has them; otherwise takes nothing and
    /// tells you how long to wait.
    ///
    /// On failure, [`TryConsumeError::earliest_retry_after`] is the time until the bucket will
    /// have enough tokens, assuming nobody else takes them first. If `to_consume` is larger
    /// than the burst, the bucket can never hold that many tokens and the error is
    /// [`TryConsumeError::ExceededBurstCapacity`].
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock, TryConsumeError};
    /// use std::num::NonZeroU32;
    ///
    /// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    ///
    /// match bucket.try_consume(NonZeroU32::new(5).unwrap()) {
    ///     Ok(tokens) => println!("took {} tokens", tokens.as_u64()),
    ///     Err(TryConsumeError::RateLimited(limited)) => {
    ///         println!("try again in {:?}", limited.earliest_retry_after());
    ///     }
    ///     Err(TryConsumeError::ExceededBurstCapacity(_)) => {
    ///         println!("5 tokens never fit in this bucket");
    ///     }
    /// }
    /// ```
    pub fn try_consume(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> Result<Tokens, TryConsumeError> {
        self.bucket
            .try_consume(to_consume, &self.clock, &self.limit)
    }

    /// Takes as many whole tokens as the bucket has, up to `to_consume`.
    ///
    /// Returns how many tokens were taken, or `None` if not even one was available. A fraction
    /// of a token stays in the bucket. Useful for batches: process as many items as the limit
    /// allows right now and keep the rest for later.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    /// let mut queue: Vec<u32> = (0..25).collect();
    ///
    /// if let Some(tokens) = bucket.saturating_consume(NonZeroU32::new(25).unwrap()) {
    ///     let batch: Vec<u32> = queue.drain(..tokens.as_u64() as usize).collect();
    ///     assert_eq!(batch.len(), 10); // the bucket held 10 tokens
    /// }
    /// assert_eq!(queue.len(), 15);
    /// ```
    #[must_use]
    pub fn saturating_consume(&self, to_consume: impl Into<NonZeroU32>) -> Option<Tokens> {
        self.bucket
            .saturating_consume(to_consume, &self.clock, &self.limit)
    }

    /// Puts tokens back into the bucket.
    ///
    /// Use it to return tokens you took but didn't use, for example when you reserved tokens
    /// for a batch and processed fewer items. Returned tokens first pay off any debt, and the
    /// bucket never holds more than its burst.
    ///
    /// ```rust
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
    /// let reserved = bucket.consume(NonZeroU32::new(8).unwrap()).unwrap();
    ///
    /// let used = 3;
    /// bucket.add_tokens((reserved.as_u64() - used) as u32);
    /// assert!(bucket.available() > 6.9); // 2 left over, plus the 5 returned
    /// ```
    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        self.bucket.add_tokens(tokens, &self.limit)
    }

    /// Takes `to_consume` tokens even if the bucket doesn't have them, going into debt, and
    /// returns how long to wait until the debt is paid.
    ///
    /// This is the right tool for work that must happen but should be paced: do the work, then
    /// wait the returned duration (if any) before the next piece. While the bucket is in debt,
    /// other callers find it empty, so the limit holds for everyone sharing the bucket.
    ///
    /// Returns `Ok(None)` if the tokens were there and nothing was borrowed. Returns
    /// [`ExceededBurstCapacity`] if `to_consume` is larger than the burst; see
    /// [`saturating_consume_with_borrow`](Self::saturating_consume_with_borrow) to cap the
    /// request instead.
    ///
    /// ```rust,no_run
    /// use gardal::{Limit, LocalTokenBucket, StdClock};
    /// use std::num::NonZeroU32;
    ///
    /// // Copy files at no more than 50 MiB/s, one token per MiB.
    /// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(50).unwrap()), StdClock);
    ///
    /// for file_size_mib in [10u32, 30, 20, 40] {
    ///     // copy_file(...);
    ///     let cost = NonZeroU32::new(file_size_mib).unwrap();
    ///     if let Some(wait) = bucket.consume_with_borrow(cost).unwrap() {
    ///         std::thread::sleep(wait);
    ///     }
    /// }
    /// ```
    pub fn consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> Result<Option<Duration>, ExceededBurstCapacity> {
        self.bucket
            .consume_with_borrow(to_consume, &self.clock, &self.limit)
    }

    /// Like [`consume_with_borrow`](Self::consume_with_borrow), but asks for at most the
    /// burst instead of failing when `to_consume` is larger.
    ///
    /// Returns the time to wait until the debt is paid (zero if nothing was borrowed), together
    /// with the number of tokens charged when the bucket had to borrow (`None` when the tokens
    /// were available).
    #[must_use]
    pub fn saturating_consume_with_borrow(
        &self,
        to_consume: impl Into<NonZeroU32>,
    ) -> (Option<Tokens>, Duration) {
        self.bucket
            .saturating_consume_with_borrow(to_consume, &self.clock, &self.limit)
    }

    /// See [`RawTokenBucket::saturating_borrow_nap`].
    #[cfg(feature = "async")]
    #[inline]
    pub(crate) fn saturating_borrow_nap(&self, to_consume: NonZeroU32) -> f64 {
        self.bucket
            .saturating_borrow_nap(to_consume, &self.clock, &self.limit)
    }

    /// The number of tokens that can be taken right now.
    ///
    /// Never negative: a bucket in debt reports `0.0`. Use [`balance`](Self::balance) to see
    /// the debt. The value can include a fraction of a token.
    pub fn available(&self) -> f64 {
        self.bucket.available(&self.clock, &self.limit)
    }

    /// The bucket's balance: like [`available`](Self::available), but negative while the
    /// bucket is in debt after [`consume_with_borrow`](Self::consume_with_borrow).
    pub fn balance(&self) -> f64 {
        self.bucket.balance(&self.clock, &self.limit)
    }

    /// The bucket's current limit.
    pub fn limit(&self) -> &Limit {
        &self.limit
    }

    /// The bucket's complete state, as a single number.
    ///
    /// This is the time (in the clock's seconds) at which the bucket was, or will be, empty.
    /// Save it to restore the bucket later with [`with_zero_time`](Self::with_zero_time).
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
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
        assert_eq!(
            format!("{tb:?}"),
            "TokenBucket { limit: Limit(rate_per_second=10, burst=20), balance: 15.0 }"
        );
    }

    #[test]
    fn basics_dynamic() {
        let clock = Arc::new(ManualClock::default());
        // initially, the bucket is empty
        let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
        let mut tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
    fn new_starts_full_and_new_empty_starts_empty() {
        use crate::LocalTokenBucket;

        // A rate that doesn't divide the burst evenly, so rounding could leave a bucket a hair
        // short of full.
        let limit = Limit::per_second_and_burst(nonzero!(3u32), nonzero!(10u32));

        // Full with any clock, including a manual clock at time 0.0.
        let clock = Arc::new(ManualClock::new(0.0));
        let full = LocalTokenBucket::new(limit, Arc::clone(&clock));
        assert_eq!(10.0, full.available());
        assert_eq!(10, full.consume(nonzero!(10u32)).unwrap().as_u64());
        let real = LocalTokenBucket::new(limit, StdClock);
        assert_eq!(10, real.consume(nonzero!(10u32)).unwrap().as_u64());

        // Empty as of its creation, no matter how long the clock has been running.
        clock.set(1000.0);
        let empty = LocalTokenBucket::new_empty(limit, Arc::clone(&clock));
        assert_eq!(0.0, empty.available());
        clock.advance(1.0);
        assert!((empty.available() - 3.0).abs() < 1e-9);
        let real = LocalTokenBucket::new_empty(limit, StdClock);
        assert!(real.consume_one().is_none());
    }

    #[test]
    fn raw_reset_keeps_tokens() {
        use crate::LocalStorage;

        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let clock = ManualClock::new(0.0);
        let old = Limit::per_second(nonzero!(10u32));
        let new = Limit::per_second(nonzero!(100u32));
        let mut raw = RawTokenBucket::<LocalStorage, _>::new_empty(&&clock);
        // 5 tokens at 10/s
        clock.set(0.5);
        assert!(close(5.0, raw.available(&&clock, &old)));
        // switching to 100/s keeps the 5 tokens (it used to turn them into 50)
        raw.reset(&&clock, &old, &new);
        assert!(close(5.0, raw.available(&&clock, &new)));
        // debt is kept too: take 15 out of 5, leaving a debt of 10
        let wait = raw
            .consume_with_borrow(nonzero!(15u32), &&clock, &new)
            .unwrap()
            .unwrap();
        assert!(close(0.1, wait.as_secs_f64()));
        assert!(close(-10.0, raw.balance(&&clock, &new)));
        raw.reset(&&clock, &new, &old);
        assert!(close(-10.0, raw.balance(&&clock, &old)));
    }

    #[test]
    fn fractional() {
        let clock = Arc::new(ManualClock::default());
        // initially, the bucket is empty
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(20u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
        // only whole tokens are consumed, the fraction stays in the bucket
        clock.set(1.3);
        let drained = tb.saturating_consume(nonzero!(8u32));
        assert_eq!(drained.unwrap().as_u64(), 1);
        assert!((tb.available() - 0.5).abs() < 1e-9);
        assert!(tb.saturating_consume(nonzero!(8u32)).is_none());
        assert!((tb.available() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn wait_to_consume() {
        let clock = Arc::new(ManualClock::default());
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
        // at t=0 bucket empty; borrow 5 tokens should require waiting for 10.0 seconds. Note that
        // we didn't consume anything as a result.
        match tb.try_consume(nonzero!(5u32)) {
            Ok(_) => panic!("should not be able to consume"),
            Err(TryConsumeError::RateLimited(RateLimited {
                earliest_retry_time: est_wait_time,
            })) => {
                assert_eq!(10.0, est_wait_time.as_secs_f64());
            }
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn try_consume_more_than_burst() {
        let clock = Arc::new(ManualClock::default());
        let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
        // the bucket is full, but 25 tokens can never fit in a bucket of 20
        clock.set(100.0);
        assert_eq!(20.0, tb.available());
        let err = tb.try_consume(nonzero!(25u32)).unwrap_err();
        assert_eq!(
            err,
            TryConsumeError::ExceededBurstCapacity(ExceededBurstCapacity)
        );
        assert_eq!(None, err.earliest_retry_after());
        // nothing was consumed
        assert_eq!(20.0, tb.available());
        // exactly the burst capacity is fine
        assert_eq!(20, tb.try_consume(nonzero!(20u32)).unwrap().as_u64());
        // and an empty bucket reports the wait time, not the burst error
        let err = tb.try_consume(nonzero!(20u32)).unwrap_err();
        assert_eq!(Some(Duration::from_secs(2)), err.earliest_retry_after());
    }

    #[test]
    fn borrow_future() {
        let clock = Arc::new(ManualClock::default());
        let limit = Limit::per_minute(nonzero!(30u32)).with_burst(nonzero!(10u32));
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
        let tb = TokenBucket::<PaddedAtomicStorage, _>::new_empty(limit, Arc::clone(&clock));
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
