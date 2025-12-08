use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime};

struct Datum {
    dur_since_epoch: f64,
    std_instant: std::time::Instant,
    #[cfg(feature = "tokio")]
    tokio_instant: tokio::time::Instant,
    #[cfg(feature = "quanta")]
    quanta_instant: quanta::Instant,
}

static CLOCK_ORIGIN: LazyLock<Datum> = LazyLock::new(|| Datum {
    dur_since_epoch: SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64(),
    std_instant: std::time::Instant::now(),
    #[cfg(feature = "tokio")]
    tokio_instant: tokio::time::Instant::now(),
    #[cfg(feature = "quanta")]
    quanta_instant: quanta::Instant::now(),
});

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
    /// The origin time point of the clock.
    ///
    /// The origin is the point where now() is guaranteed to be after.
    fn datum(&self) -> f64 {
        0.0
    }
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
/// use gardal::{TokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = StdClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// ```
#[derive(Clone, Default)]
pub struct StdClock;

impl Clock for StdClock {
    fn datum(&self) -> f64 {
        CLOCK_ORIGIN.dur_since_epoch
    }

    fn now(&self) -> f64 {
        std::time::Instant::now()
            .duration_since(CLOCK_ORIGIN.std_instant)
            .as_secs_f64()
            + CLOCK_ORIGIN.dur_since_epoch
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
/// use gardal::{TokenBucket, Limit, QuantaClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
/// let clock = QuantaClock::default();
/// let bucket = TokenBucket::with_clock(limit, clock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Clone, Default)]
pub struct QuantaClock;

#[cfg(feature = "quanta")]
impl Clock for QuantaClock {
    fn now(&self) -> f64 {
        quanta::Instant::now()
            .duration_since(CLOCK_ORIGIN.quanta_instant)
            .as_secs_f64()
            + CLOCK_ORIGIN.dur_since_epoch
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
/// use gardal::{TokenBucket, Limit, TokioClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(100).unwrap());
/// let bucket = TokenBucket::with_clock(limit, TokioClock);
/// # }
/// ```
#[cfg(feature = "tokio")]
#[derive(Clone, Default)]
pub struct TokioClock;

#[cfg(feature = "tokio")]
impl Clock for TokioClock {
    fn datum(&self) -> f64 {
        CLOCK_ORIGIN.dur_since_epoch
    }

    fn now(&self) -> f64 {
        tokio::time::Instant::now()
            .duration_since(CLOCK_ORIGIN.tokio_instant)
            .as_secs_f64()
            + CLOCK_ORIGIN.dur_since_epoch
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
/// use gardal::{TokenBucket, Limit, FastClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(1000).unwrap());
/// let clock = FastClock::default();
/// let bucket = TokenBucket::with_clock(limit, FastClock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Clone, Default)]
pub struct FastClock;

#[cfg(feature = "quanta")]
impl Clock for FastClock {
    fn datum(&self) -> f64 {
        CLOCK_ORIGIN.dur_since_epoch
    }

    fn now(&self) -> f64 {
        quanta::Instant::recent()
            .duration_since(CLOCK_ORIGIN.quanta_instant)
            .as_secs_f64()
            + CLOCK_ORIGIN.dur_since_epoch
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
/// use gardal::{TokenBucket, Limit, ManualClock};
/// use std::num::NonZeroU32;
/// use std::sync::Arc;
///
/// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
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
