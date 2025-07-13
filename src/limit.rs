use std::num::NonZeroU32;

const SECONDS_PER_MINUTE: f64 = 60.0;
const SECONDS_PER_HOUR: f64 = 3600.0;

/// Configuration for token bucket rate and burst limits.
///
/// Defines how fast tokens are added to the bucket (rate) and the maximum
/// number of tokens the bucket can hold (burst capacity).
///
/// # Examples
///
/// ```rust
/// use gardal::RateLimit;
/// use std::num::NonZeroU32;
///
/// // 100 requests per second, burst of 200
/// let limit = RateLimit::per_second_and_burst(
///     NonZeroU32::new(100).unwrap(),
///     NonZeroU32::new(200).unwrap()
/// );
///
/// // 60 requests per minute (1 per second), burst equals rate
/// let limit = RateLimit::per_minute(NonZeroU32::new(60).unwrap());
/// ```
#[derive(Clone, Copy)]
pub struct RateLimit {
    pub(crate) rate: f64,
    pub(crate) burst: f64,
}

impl std::fmt::Debug for RateLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RateLimit(rate_per_second={}, burst={})",
            self.rate_per_second(),
            self.burst()
        )
    }
}

impl RateLimit {
    /// Creates a rate limit with the specified tokens per second.
    ///
    /// The burst capacity is set equal to the rate, allowing for one second's
    /// worth of tokens to be consumed immediately.
    ///
    /// # Arguments
    ///
    /// * `rate` - Number of tokens per second
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::RateLimit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
    /// assert_eq!(limit.rate_per_second(), 100.0);
    /// assert_eq!(limit.burst(), NonZeroU32::new(100).unwrap());
    /// ```
    pub const fn per_second(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: rate.get() as f64,
        }
    }

    /// Creates a rate limit with specified tokens per second and burst capacity.
    ///
    /// # Arguments
    ///
    /// * `rate` - Number of tokens per second
    /// * `burst` - Maximum number of tokens the bucket can hold
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::RateLimit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = RateLimit::per_second_and_burst(
    ///     NonZeroU32::new(10).unwrap(),
    ///     NonZeroU32::new(50).unwrap()
    /// );
    /// assert_eq!(limit.rate_per_second(), 10.0);
    /// assert_eq!(limit.burst(), NonZeroU32::new(50).unwrap());
    /// ```
    pub const fn per_second_and_burst(rate: NonZeroU32, burst: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: burst.get() as f64,
        }
    }

    /// Creates a rate limit with the specified tokens per minute.
    ///
    /// The burst capacity is set equal to the per-minute rate.
    ///
    /// # Arguments
    ///
    /// * `rate` - Number of tokens per minute
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::RateLimit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = RateLimit::per_minute(NonZeroU32::new(60).unwrap());
    /// assert_eq!(limit.rate_per_second(), 1.0);
    /// assert_eq!(limit.rate_per_minute(), 60.0);
    /// ```
    pub const fn per_minute(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_MINUTE,
            burst: rate.get() as f64 / SECONDS_PER_MINUTE,
        }
    }

    /// Creates a rate limit with the specified tokens per hour.
    ///
    /// The burst capacity is set equal to the per-hour rate.
    ///
    /// # Arguments
    ///
    /// * `rate` - Number of tokens per hour
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::RateLimit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = RateLimit::per_hour(NonZeroU32::new(3600).unwrap());
    /// assert_eq!(limit.rate_per_second(), 1.0);
    /// assert_eq!(limit.rate_per_hour(), 3600.0);
    /// ```
    pub const fn per_hour(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_HOUR,
            burst: rate.get() as f64 / SECONDS_PER_HOUR,
        }
    }

    /// Sets a custom burst capacity for this rate limit.
    ///
    /// # Arguments
    ///
    /// * `burst` - Maximum number of tokens the bucket can hold
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::RateLimit;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = RateLimit::per_second(NonZeroU32::new(10).unwrap())
    ///     .with_burst(NonZeroU32::new(100).unwrap());
    /// assert_eq!(limit.rate_per_second(), 10.0);
    /// assert_eq!(limit.burst(), NonZeroU32::new(100).unwrap());
    /// ```
    pub const fn with_burst(mut self, burst: NonZeroU32) -> Self {
        self.burst = burst.get() as f64;
        self
    }

    /// Returns the rate in tokens per second.
    ///
    /// # Returns
    ///
    /// Number of tokens added to the bucket per second
    pub const fn rate_per_second(&self) -> f64 {
        self.rate
    }

    /// Returns the rate in tokens per minute.
    ///
    /// # Returns
    ///
    /// Number of tokens added to the bucket per minute
    pub const fn rate_per_minute(&self) -> f64 {
        self.rate * SECONDS_PER_MINUTE
    }

    /// Returns the rate in tokens per hour.
    ///
    /// # Returns
    ///
    /// Number of tokens added to the bucket per hour
    pub const fn rate_per_hour(&self) -> f64 {
        self.rate * SECONDS_PER_HOUR
    }

    /// Returns the burst capacity.
    ///
    /// # Returns
    ///
    /// Maximum number of tokens the bucket can hold
    pub const fn burst(&self) -> NonZeroU32 {
        NonZeroU32::new(self.burst as u32).unwrap()
    }
}
