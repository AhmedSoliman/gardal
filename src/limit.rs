use std::num::NonZeroU32;

const SECONDS_PER_MINUTE: f64 = 60.0;
const SECONDS_PER_HOUR: f64 = 3600.0;

/// Defines the rate and burst size of a token bucket.
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
    pub const fn per_second(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: rate.get() as f64,
        }
    }

    pub const fn per_second_and_burst(rate: NonZeroU32, burst: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64,
            burst: burst.get() as f64,
        }
    }

    pub const fn per_minute(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_MINUTE,
            burst: rate.get() as f64 / SECONDS_PER_MINUTE,
        }
    }

    pub const fn per_hour(rate: NonZeroU32) -> Self {
        Self {
            rate: rate.get() as f64 / SECONDS_PER_HOUR,
            burst: rate.get() as f64 / SECONDS_PER_HOUR,
        }
    }

    pub const fn with_burst(mut self, burst: NonZeroU32) -> Self {
        self.burst = burst.get() as f64;
        self
    }

    pub const fn rate_per_second(&self) -> f64 {
        self.rate
    }

    pub const fn rate_per_minute(&self) -> f64 {
        self.rate * SECONDS_PER_MINUTE
    }

    pub const fn rate_per_hour(&self) -> f64 {
        self.rate * SECONDS_PER_HOUR
    }

    pub const fn burst(&self) -> NonZeroU32 {
        NonZeroU32::new(self.burst as u32).unwrap()
    }
}
