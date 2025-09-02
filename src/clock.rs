use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Represents a non-zero time duration in nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Nanos(NonZeroU64);

impl Nanos {
    pub fn new_checked(secs: f64) -> Option<Self> {
        if secs <= 0.0 {
            None
        } else {
            Some(Self::from_secs_f64_unchecked(secs))
        }
    }

    pub const fn as_secs_f64(&self) -> f64 {
        f64::from_bits(self.0.get())
    }

    pub(crate) const fn from_secs_f64_unchecked(secs: f64) -> Self {
        Self(unsafe { NonZeroU64::new_unchecked(secs.to_bits()) })
    }
}

impl From<Nanos> for Duration {
    fn from(nanos: Nanos) -> Duration {
        Duration::from_secs_f64(nanos.as_secs_f64())
    }
}

impl Debug for Nanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dur = Duration::from_secs_f64(self.as_secs_f64());
        Debug::fmt(&dur, f)
    }
}

impl Display for Nanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dur = Duration::from_secs_f64(self.as_secs_f64());
        Debug::fmt(&dur, f)
    }
}

/// Trait for monotonic clock implementations used by token buckets.
///
/// Implementations must provide monotonic time that never goes backwards.
/// The time is measured in seconds as floating-point values.
pub trait Clock {
    /// Returns the current time in seconds since an arbitrary epoch.
    ///
    /// The returned value must be monotonic (never decrease) and should
    /// have sufficient precision for rate limiting purposes.
    ///
    /// # Returns
    ///
    /// Current time in seconds as a floating-point value
    fn now(&self) -> f64;
}

/// Standard clock implementation using [`std::time::Instant`].
///
/// This provides high precision timing but may be slower than alternatives.
/// For high-performance scenarios, consider using `FastClock` which can be
/// up to 10x faster with slightly reduced precision.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = StdClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// ```
#[derive(Clone)]
pub struct StdClock {
    origin: std::time::Instant,
}

impl Default for StdClock {
    fn default() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

impl Clock for StdClock {
    fn now(&self) -> f64 {
        std::time::Instant::now()
            .duration_since(self.origin)
            .as_secs_f64()
    }
}

/// High-precision clock implementation using the `quanta` crate.
///
/// Provides precise timing with better performance characteristics than [`StdClock`]
/// in some scenarios. Requires the "quanta" feature to be enabled.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "quanta")]
/// # {
/// use gardal::{TokenBucket, RateLimit, QuantaClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = QuantaClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Clone)]
pub struct QuantaClock {
    origin: quanta::Instant,
}

#[cfg(feature = "quanta")]
impl Default for QuantaClock {
    fn default() -> Self {
        Self::new(quanta::Clock::new())
    }
}

#[cfg(feature = "quanta")]
impl QuantaClock {
    /// Creates a new `QuantaClock` from a `quanta::Clock` instance.
    ///
    /// # Arguments
    ///
    /// * `clock` - The quanta clock instance to use
    pub fn new(clock: quanta::Clock) -> Self {
        let origin = clock.now();
        Self { origin }
    }
}

#[cfg(feature = "quanta")]
impl Clock for QuantaClock {
    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }
}

/// Tokio-compatible clock implementation using [`tokio::time::Instant`].
///
/// Designed for use in async contexts with Tokio. Requires the "tokio" feature.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "tokio")]
/// # {
/// use gardal::{TokenBucket, RateLimit, TokioClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = TokioClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// # }
/// ```
#[cfg(feature = "tokio")]
#[derive(Clone)]
pub struct TokioClock {
    origin: tokio::time::Instant,
}

#[cfg(feature = "tokio")]
impl Default for TokioClock {
    fn default() -> Self {
        Self {
            origin: tokio::time::Instant::now(),
        }
    }
}

#[cfg(feature = "tokio")]
impl Clock for TokioClock {
    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }
}

/// High-performance clock using quanta's coarse timing.
///
/// Provides significantly better performance than [`StdClock`] (up to 10x faster)
/// with precision limited by quanta's upkeep thread configuration. Requires the
/// "quanta" feature to be enabled.
///
/// # Performance vs Precision Trade-off
///
/// This clock sacrifices some precision for speed by using quanta's coarse timing.
/// The precision depends on how frequently quanta's upkeep thread runs.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "quanta")]
/// # {
/// use gardal::{TokenBucket, RateLimit, FastClock};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(1000).unwrap());
/// let clock = FastClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Clone)]
pub struct FastClock {
    clock: quanta::Clock,
    origin: quanta::Instant,
}

#[cfg(feature = "quanta")]
impl Default for FastClock {
    fn default() -> Self {
        Self::new(quanta::Clock::new())
    }
}

#[cfg(feature = "quanta")]
impl FastClock {
    /// Creates a new `FastClock` from a `quanta::Clock` instance.
    ///
    /// **Important**: Ensure the clock's upkeep thread is running, otherwise
    /// the token bucket will not observe clock changes and timing will be incorrect.
    ///
    /// # Arguments
    ///
    /// * `clock` - The quanta clock instance to use
    pub fn new(clock: quanta::Clock) -> Self {
        let origin = clock.recent();
        Self { clock, origin }
    }
}

#[cfg(feature = "quanta")]
impl Clock for FastClock {
    fn now(&self) -> f64 {
        (self.clock.recent() - self.origin).as_secs_f64()
    }
}

/// Manual clock implementation for testing and simulation.
///
/// Allows precise control over time progression, making it ideal for unit tests
/// and deterministic simulations of rate limiting behavior.
///
/// # Thread Safety
///
/// This clock is thread-safe and can be shared across multiple threads.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit, ManualClock};
/// use std::num::NonZeroU32;
/// use std::sync::Arc;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(10).unwrap());
/// let clock = Arc::new(ManualClock::new(0.0));
/// let bucket = TokenBucket::with_clock(limit, Arc::clone(&clock));
///
/// // Initially no tokens available
/// assert!(bucket.consume_one().is_none());
///
/// // Advance time by 1 second
/// clock.advance(1.0);
/// assert!(bucket.consume_one().is_some());
/// ```
pub struct ManualClock {
    pub now: Mutex<f64>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl ManualClock {
    /// Creates a new manual clock starting at the specified time.
    ///
    /// # Arguments
    ///
    /// * `now` - Initial time value in seconds
    pub fn new(now: f64) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Sets the current time to the specified value.
    ///
    /// # Arguments
    ///
    /// * `now` - New time value in seconds
    pub fn set(&self, now: f64) {
        let mut guard = self.now.lock().unwrap();
        *guard = now;
    }

    /// Advances the current time by the specified duration.
    ///
    /// # Arguments
    ///
    /// * `delta` - Time to advance in seconds
    pub fn advance(&self, delta: f64) {
        let mut guard = self.now.lock().unwrap();
        *guard += delta;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> f64 {
        let guard = self.now.lock().unwrap();
        *guard
    }
}

impl Clock for &ManualClock {
    fn now(&self) -> f64 {
        let guard = self.now.lock().unwrap();
        *guard
    }
}

impl Clock for Arc<ManualClock> {
    fn now(&self) -> f64 {
        let guard = self.now.lock().unwrap();
        *guard
    }
}
