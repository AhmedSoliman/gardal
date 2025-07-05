use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;

use crate::clock::Nanos;

pub struct RateLimited {
    pub(crate) earliest_retry_time: Nanos,
}

/// The bucket is full and the requested amount of tokens can never be accepted
/// because it would exceed the burst capacity.
pub struct ExceededBurstCapacity;

impl RateLimited {
    /// The suggested duration to wait before retrying.
    pub fn earliest_retry_after(&self) -> Duration {
        Duration::from_secs_f64(self.earliest_retry_time.as_secs_f64())
    }

    /// The suggested duration to wait before retrying in nanoseconds.
    pub fn earliest_retry_after_nanos(&self) -> Nanos {
        self.earliest_retry_time
    }
}

impl Debug for RateLimited {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rate limited; suggested nap duration is {:?}",
            self.earliest_retry_time
        )
    }
}

impl Display for RateLimited {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rate limited; suggested nap duration is {}",
            self.earliest_retry_time
        )
    }
}

impl Debug for ExceededBurstCapacity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "exceeded burst capacity")
    }
}

impl Display for ExceededBurstCapacity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "borrowing more than the total burst capacity is not permitted"
        )
    }
}

impl std::error::Error for RateLimited {}
impl std::error::Error for ExceededBurstCapacity {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rate_limited() {
        let rl = RateLimited {
            earliest_retry_time: Nanos::from_secs_f64_unchecked(10.0),
        };
        assert_eq!(
            "rate limited; suggested nap duration is 10s",
            rl.to_string()
        );
    }

    #[test]
    fn display_exceeded_burst_capacity() {
        let rl = ExceededBurstCapacity;
        assert_eq!(
            "borrowing more than the total burst capacity is not permitted",
            rl.to_string()
        );
    }
}
