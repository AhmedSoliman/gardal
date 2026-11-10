use std::num::NonZeroU32;

const SECONDS_PER_MINUTE: f64 = 60.0;
const SECONDS_PER_HOUR: f64 = 3600.0;

/// How fast a bucket refills, and how many tokens it can hold.
///
/// A limit has two parts:
///
/// - The **rate**: how many tokens are added to the bucket over time. This is the long-term
///   average throughput you allow.
/// - The **burst**: the bucket's capacity. When the bucket has been idle for a while it fills
///   up to this many tokens, which can then be spent all at once.
///
/// # Choosing the burst
///
/// The constructors set the burst equal to the rate's unit amount, so
/// `Limit::per_minute(100)` allows 100 requests in the very first second after an idle minute.
/// If you want traffic to stay smooth, set a smaller burst with [`with_burst`](Self::with_burst):
///
/// ```rust
/// use gardal::Limit;
/// use std::num::NonZeroU32;
///
/// // 100 per minute on average, but never more than 5 back to back.
/// let limit = Limit::per_minute(NonZeroU32::new(100).unwrap())
///     .with_burst(NonZeroU32::new(5).unwrap());
/// ```
///
/// A larger burst is friendlier to clients that send work in batches; a smaller one protects
/// whatever sits behind the limiter from spikes.
#[derive(Clone, Copy, PartialEq)]
pub struct Limit {
    pub(crate) rate: f64,
    pub(crate) burst: f64,
}

impl std::fmt::Debug for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Limit(rate_per_second={}, burst={})",
            self.rate_per_second(),
            self.burst()
        )
    }
}

impl Limit {
    /// `rate` tokens per second, with a burst of `rate` tokens.
    ///
    /// ```rust
    /// use gardal::Limit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
    /// assert_eq!(limit.rate_per_second(), 100.0);
    /// assert_eq!(limit.burst().get(), 100);
    /// ```
    pub const fn per_second(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: rate.get() as f64,
        }
    }

    /// `rate` tokens per second, with a burst of `burst` tokens.
    ///
    /// ```rust
    /// use gardal::Limit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second_and_burst(
    ///     NonZeroU32::new(10).unwrap(),
    ///     NonZeroU32::new(50).unwrap(),
    /// );
    /// assert_eq!(limit.rate_per_second(), 10.0);
    /// assert_eq!(limit.burst().get(), 50);
    /// ```
    pub const fn per_second_and_burst(rate: NonZeroU32, burst: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: burst.get() as f64,
        }
    }

    /// `rate` tokens per minute, with a burst of `rate` tokens.
    ///
    /// Tokens are added continuously, not once a minute: `per_minute(60)` adds one token every
    /// second.
    ///
    /// ```rust
    /// use gardal::Limit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_minute(NonZeroU32::new(60).unwrap());
    /// assert_eq!(limit.rate_per_second(), 1.0);
    /// assert_eq!(limit.burst().get(), 60);
    /// ```
    pub const fn per_minute(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_MINUTE,
            burst: (rate.get() as f64).max(1.0),
        }
    }

    /// `rate` tokens per hour, with a burst of `rate` tokens.
    ///
    /// Tokens are added continuously: `per_hour(3600)` adds one token every second.
    ///
    /// ```rust
    /// use gardal::Limit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_hour(NonZeroU32::new(3600).unwrap());
    /// assert_eq!(limit.rate_per_second(), 1.0);
    /// ```
    pub const fn per_hour(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_HOUR,
            burst: (rate.get() as f64).max(1.0),
        }
    }

    /// Returns this limit with a different burst, keeping the rate.
    ///
    /// ```rust
    /// use gardal::Limit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap())
    ///     .with_burst(NonZeroU32::new(100).unwrap());
    /// assert_eq!(limit.rate_per_second(), 10.0);
    /// assert_eq!(limit.burst().get(), 100);
    /// ```
    pub const fn with_burst(mut self, burst: NonZeroU32) -> Self {
        self.burst = burst.get() as f64;
        self
    }

    /// The refill rate, in tokens per second.
    pub const fn rate_per_second(&self) -> f64 {
        self.rate
    }

    /// The refill rate, in tokens per minute.
    pub const fn rate_per_minute(&self) -> f64 {
        self.rate * SECONDS_PER_MINUTE
    }

    /// The refill rate, in tokens per hour.
    pub const fn rate_per_hour(&self) -> f64 {
        self.rate * SECONDS_PER_HOUR
    }

    /// The burst: the most tokens the bucket can hold.
    pub const fn burst(&self) -> NonZeroU32 {
        NonZeroU32::new(self.burst as u32).unwrap()
    }
}
