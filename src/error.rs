use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;

use crate::clock::Nanos;

/// Error returned when a token consumption request is rate limited.
///
/// Contains information about when the operation can be retried.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(1).unwrap());
/// let bucket = TokenBucket::new(limit);
///
/// match bucket.try_consume_one() {
///     Ok(tokens) => println!("Got tokens: {}", tokens.as_u64()),
///     Err(rate_limited) => {
///         let wait_time = rate_limited.earliest_retry_after();
///         println!("Rate limited, retry in {:?}", wait_time);
///     }
/// }
/// ```
pub struct RateLimited {
    pub(crate) earliest_retry_time: Nanos,
}

/// Error returned when trying to borrow more tokens than the burst capacity allows.
///
/// This occurs when using [`consume_with_borrow`](crate::TokenBucket::consume_with_borrow)
/// and requesting more tokens than the bucket's maximum capacity.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second_and_burst(
///     NonZeroU32::new(10).unwrap(),
///     NonZeroU32::new(20).unwrap()
/// );
/// let bucket = TokenBucket::new(limit);
///
/// // This will fail because 25 > burst capacity of 20
/// match bucket.consume_with_borrow(NonZeroU32::new(25).unwrap()) {
///     Ok(_) => println!("Borrowed successfully"),
///     Err(_) => println!("Cannot borrow more than burst capacity"),
/// }
/// ```
pub struct ExceededBurstCapacity;

impl RateLimited {
    /// Returns the suggested duration to wait before retrying the operation.
    ///
    /// # Returns
    ///
    /// A [`Duration`] indicating the minimum time to wait before retrying
    pub fn earliest_retry_after(&self) -> Duration {
        Duration::from_secs_f64(self.earliest_retry_time.as_secs_f64())
    }

    /// Returns the suggested wait time in high-precision nanoseconds.
    ///
    /// # Returns
    ///
    /// A `Nanos` value representing the minimum time to wait before retrying
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
