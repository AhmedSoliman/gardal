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

// A trait for a monotonic clock abstraction
pub trait Clock {
    /// A monotonic clock that returns the current time in seconds
    fn now(&self) -> f64;
}

/// This uses std::time::Instant as a monotonic clock, it's precise but slow.
///
/// If the token bucket is used in a high-performance context, it's recommended
/// to use FastClock instead (based on quanta's coarse clock) it provides
/// precision limited to how quanta's upkeep thread is configured and it can be
/// up to 10x faster than StdClock.
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
    /// Make sure the clock's upkeep thread/task is running.
    /// otherwise, the token bucket will not observe the clock
    /// changes.
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

pub struct ManualClock {
    pub now: Mutex<f64>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl ManualClock {
    pub fn new(now: f64) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    pub fn set(&self, now: f64) {
        let mut guard = self.now.lock().unwrap();
        *guard = now;
    }

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
