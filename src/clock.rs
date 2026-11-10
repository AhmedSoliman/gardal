use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime};

/// A monotonic instant paired with the Unix time at which it was taken, so that a monotonic
/// clock can report seconds since the Unix epoch.
///
/// Each clock source has its own origin, created on first use: reading a `StdClock` never pays
/// for setting up the Tokio or quanta clocks (quanta calibrates itself on first use, which can
/// take a while).
struct Origin<I> {
    unix_secs: f64,
    instant: I,
}

impl<I> Origin<I> {
    fn new(now: impl FnOnce() -> I) -> Self {
        // Take the instant first: the first read of some clocks (quanta) is slow, and the Unix
        // time must be taken after it for the two to match.
        let instant = now();
        let unix_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        Self { unix_secs, instant }
    }
}

static STD_ORIGIN: LazyLock<Origin<std::time::Instant>> =
    LazyLock::new(|| Origin::new(std::time::Instant::now));

#[cfg(feature = "tokio")]
static TOKIO_ORIGIN: LazyLock<Origin<tokio::time::Instant>> =
    LazyLock::new(|| Origin::new(tokio::time::Instant::now));

#[cfg(feature = "quanta")]
static QUANTA_ORIGIN: LazyLock<Origin<quanta::Instant>> =
    LazyLock::new(|| Origin::new(quanta::Instant::now));

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

/// Where a token bucket gets the current time from.
///
/// A bucket refills based on how much time has passed, so it needs a clock. Every bucket method
/// reads the clock once, which makes the clock's speed part of the cost of taking tokens.
///
/// # Choosing a clock
///
/// | Clock | Feature | Use it when |
/// |---|---|---|
/// | [`StdClock`] | | You want a good default |
/// | `TokioClock` | `tokio` or `async` | You use Tokio, and want tests with paused time to work |
/// | `QuantaClock` | `quanta` | You want precise time that is cheaper to read than `StdClock` |
/// | `FastClock` | `quanta` | Taking tokens is very hot and ~1 ms precision is fine |
/// | [`ManualClock`] | | Tests and simulations, where you move time yourself |
///
/// `Clock` is also implemented for `&C`, `Box<C>` and `Arc<C>`, so one clock can be shared by
/// many buckets.
///
/// # Writing your own clock
///
/// Implement [`now`](Clock::now) to return the current time in seconds, as an `f64`. It must
/// never go backwards.
///
/// ```rust
/// use gardal::{Clock, Limit, SharedTokenBucket};
/// use std::num::NonZeroU32;
/// use std::time::Instant;
///
/// /// Seconds since this clock was created.
/// struct SinceStart(Instant);
///
/// impl Clock for SinceStart {
///     fn now(&self) -> f64 {
///         self.0.elapsed().as_secs_f64()
///     }
/// }
///
/// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
/// let bucket = SharedTokenBucket::new(limit, SinceStart(Instant::now()));
/// ```
///
/// # Precision
///
/// The built-in clocks ([`StdClock`], `TokioClock`, `QuantaClock` and `FastClock`) count seconds
/// since the Unix epoch. Because every process agrees on that origin, a bucket's state (see
/// [`TokenBucket::get_zero_time`](crate::TokenBucket::get_zero_time)) can be saved and restored
/// with [`TokenBucket::with_zero_time`](crate::TokenBucket::with_zero_time), even in another
/// process.
///
/// The trade-off is resolution: an `f64` near today's Unix time (~1.8e9 seconds) moves in steps
/// of about 238 ns. At `r` tokens per second, that is a refill step of about `r × 2.4e-7`
/// tokens, which only becomes noticeable at around 4 million tokens per second. A custom clock
/// that counts from a recent point, like `SinceStart` above, is much more precise, but its
/// saved states only make sense within the same process.
pub trait Clock {
    /// The current time, in seconds since any fixed starting point.
    ///
    /// It must never decrease.
    fn now(&self) -> f64;
}

impl<C: Clock + ?Sized> Clock for &C {
    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    #[inline]
    fn now(&self) -> f64 {
        (**self).now()
    }
}

/// The default clock, based on [`std::time::Instant`].
///
/// Precise and available everywhere. If reading the clock shows up in your profiles, consider
/// `QuantaClock` or `FastClock` (with the `quanta` feature).
///
/// ```rust
/// use gardal::{Limit, StdClock, SharedTokenBucket};
/// use std::num::NonZeroU32;
///
/// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), StdClock);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct StdClock;

impl Clock for StdClock {
    fn now(&self) -> f64 {
        // Initialize the origin before reading the time, so the first reading isn't older
        // than the origin.
        let origin = &*STD_ORIGIN;
        origin.unix_secs + origin.instant.elapsed().as_secs_f64()
    }
}

/// A precise clock based on [`quanta::Instant::now`], usually cheaper to read than
/// [`StdClock`]. Requires the `quanta` feature.
///
/// ```rust
/// # #[cfg(feature = "quanta")]
/// # {
/// use gardal::{Limit, QuantaClock, SharedTokenBucket};
/// use std::num::NonZeroU32;
///
/// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), QuantaClock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Debug, Clone, Copy, Default)]
pub struct QuantaClock;

#[cfg(feature = "quanta")]
impl Clock for QuantaClock {
    fn now(&self) -> f64 {
        let origin = &*QUANTA_ORIGIN;
        origin.unix_secs
            + quanta::Instant::now()
                .duration_since(origin.instant)
                .as_secs_f64()
    }
}

/// A clock based on [`tokio::time::Instant`]. Requires the `tokio` feature (also enabled by
/// `async`).
///
/// It follows Tokio's notion of time, so it respects `tokio::time::pause()` and
/// `tokio::time::advance()`: in tests, rate limits play out instantly and deterministically.
///
/// ```rust
/// # #[cfg(feature = "tokio")]
/// # {
/// use gardal::{Limit, SharedTokenBucket, TokioClock};
/// use std::num::NonZeroU32;
///
/// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(100).unwrap()), TokioClock);
/// # }
/// ```
#[cfg(feature = "tokio")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioClock;

#[cfg(feature = "tokio")]
impl Clock for TokioClock {
    fn now(&self) -> f64 {
        let origin = &*TOKIO_ORIGIN;
        origin.unix_secs + origin.instant.elapsed().as_secs_f64()
    }
}

/// The cheapest clock: reads a cached time instead of asking the operating system. Requires the
/// `quanta` feature.
///
/// The cached time is refreshed by a [`quanta::Upkeep`] thread, so the clock is only as precise
/// as that thread's interval. Start one before using this clock, and keep its handle alive for
/// as long as the clock is used. Without it, every read falls back to a full
/// [`quanta::Instant::now`].
///
/// Good for very hot paths where millisecond precision is enough.
///
/// ```rust
/// # #[cfg(feature = "quanta")]
/// # {
/// use gardal::{FastClock, Limit, SharedTokenBucket};
/// use std::num::NonZeroU32;
/// use std::time::Duration;
///
/// // Refresh the cached time every millisecond.
/// let _upkeep = quanta::Upkeep::new(Duration::from_millis(1))
///     .start()
///     .expect("quanta upkeep thread");
///
/// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(1000).unwrap()), FastClock);
/// # }
/// ```
#[cfg(feature = "quanta")]
#[derive(Debug, Clone, Copy, Default)]
pub struct FastClock;

#[cfg(feature = "quanta")]
impl Clock for FastClock {
    fn now(&self) -> f64 {
        let origin = &*QUANTA_ORIGIN;
        origin.unix_secs
            + quanta::Instant::recent()
                .duration_since(origin.instant)
                .as_secs_f64()
    }
}

/// A clock you move by hand, for tests and simulations.
///
/// Time starts where you say and only changes when you call [`set`](Self::set) or
/// [`advance`](Self::advance), so tests are fast and give the same result every time. Share it
/// with the bucket through an `Arc` (or a reference) so you can keep moving it.
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
///
/// // Empty at first.
/// assert!(bucket.consume_one().is_none());
///
/// // A second later, 10 tokens have been added.
/// clock.advance(1.0);
/// assert_eq!(bucket.available(), 10.0);
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
    /// Creates a clock whose time is `now` seconds.
    pub fn new(now: f64) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Sets the time to `now` seconds. Don't move it backwards.
    pub fn set(&self, now: f64) {
        let mut guard = self.now.lock().unwrap();
        *guard = now;
    }

    /// Moves the time forward by `delta` seconds.
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
