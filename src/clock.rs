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

/// A strictly positive duration, stored as the bit pattern of an `f64` number of seconds.
///
/// Internal representation used on hot paths to avoid converting to [`Duration`] until
/// the caller asks for it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Nanos(NonZeroU64);

impl Nanos {
    pub(crate) const fn as_secs_f64(&self) -> f64 {
        f64::from_bits(self.0.get())
    }

    /// The caller must guarantee that `secs > 0.0`.
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
        Debug::fmt(&Duration::from(*self), f)
    }
}

impl Display for Nanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&Duration::from(*self), f)
    }
}

/// Trait for monotonic clock implementations used by token buckets.
///
/// Implementations must provide monotonic time that never goes backwards.
/// The time is measured in seconds as floating-point values.
///
/// `Clock` is implemented for `&C`, `Box<C>` and `Arc<C>` for any `C: Clock`, so a
/// single clock can be shared between several buckets.
///
/// # Precision
///
/// The built-in clocks ([`StdClock`], `TokioClock`, `QuantaClock` and `FastClock`)
/// report seconds since the Unix epoch (the wall-clock time at process start plus the
/// monotonic time elapsed since then). Using a common, absolute origin means a bucket's
/// zero time (see [`TokenBucket::get_zero_time`](crate::TokenBucket::get_zero_time)) can
/// be persisted and restored with
/// [`TokenBucket::with_zero_time`](crate::TokenBucket::with_zero_time), even in another
/// process.
///
/// The trade-off is resolution: an `f64` around today's Unix time (~1.8e9 seconds) can
/// only represent steps of about 238 ns. The bucket refills continuously, so at a rate
/// of `r` tokens per second the smallest observable refill is about `r × 2.4e-7`
/// tokens. That is negligible for most workloads but reaches whole tokens at around
/// 4 million tokens per second. A custom clock with a recent origin (for instance, the
/// process start) has much finer resolution, at the cost of zero times that are only
/// meaningful within one process.
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

impl<C: Clock + ?Sized> Clock for &C {
    #[inline]
    fn datum(&self) -> f64 {
        (**self).datum()
    }

    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    #[inline]
    fn datum(&self) -> f64 {
        (**self).datum()
    }

    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    #[inline]
    fn datum(&self) -> f64 {
        (**self).datum()
    }

    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
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
#[derive(Debug, Clone, Copy, Default)]
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
#[derive(Debug, Clone, Copy, Default)]
pub struct QuantaClock;

#[cfg(feature = "quanta")]
impl Clock for QuantaClock {
    fn datum(&self) -> f64 {
        CLOCK_ORIGIN.dur_since_epoch
    }

    fn now(&self) -> f64 {
        quanta::Instant::now()
            .duration_since(CLOCK_ORIGIN.quanta_instant)
            .as_secs_f64()
            + CLOCK_ORIGIN.dur_since_epoch
    }
}

/// Tokio-compatible clock implementation using [`tokio::time::Instant`].
///
/// Designed for use in async contexts with Tokio. It follows Tokio's notion of time,
/// so it respects `tokio::time::pause()` and `tokio::time::advance()` in tests.
/// Requires the "tokio" feature.
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
#[derive(Debug, Clone, Copy, Default)]
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
/// The precision depends on how frequently quanta's upkeep thread runs. Start one
/// with [`quanta::Upkeep`]; without it, every call falls back to a full
/// `quanta::Instant::now()` read.
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
#[derive(Debug, Clone, Copy, Default)]
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
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<f64>,
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
