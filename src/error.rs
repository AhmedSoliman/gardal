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
/// use gardal::{LocalTokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second(NonZeroU32::new(1).unwrap());
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// match bucket.try_consume_one() {
///     Ok(tokens) => println!("Got tokens: {}", tokens.as_u64()),
///     Err(rate_limited) => {
///         let wait_time = rate_limited.earliest_retry_after();
///         println!("Rate limited, retry in {:?}", wait_time);
///     }
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RateLimited {
    pub(crate) earliest_retry_time: Nanos,
}

/// Error returned when trying to borrow more tokens than the burst capacity allows.
///
/// This occurs when using [`consume_with_borrow`](crate::TokenBucket::consume_with_borrow)
/// or [`try_consume`](crate::TokenBucket::try_consume) (wrapped in [`TryConsumeError`])
/// and requesting more tokens than the bucket's maximum capacity.
///
/// # Examples
///
/// ```rust
/// use gardal::{LocalTokenBucket, Limit, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second_and_burst(
///     NonZeroU32::new(10).unwrap(),
///     NonZeroU32::new(20).unwrap()
/// );
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// // This will fail because 25 > burst capacity of 20
/// match bucket.consume_with_borrow(NonZeroU32::new(25).unwrap()) {
///     Ok(_) => println!("Borrowed successfully"),
///     Err(_) => println!("Cannot borrow more than burst capacity"),
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExceededBurstCapacity;

impl RateLimited {
    /// Returns the suggested duration to wait before retrying the operation.
    ///
    /// # Returns
    ///
    /// A [`Duration`] indicating the minimum time to wait before retrying
    pub fn earliest_retry_after(&self) -> Duration {
        Duration::from(self.earliest_retry_time)
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

/// Error returned by [`try_consume`](crate::TokenBucket::try_consume).
///
/// # Examples
///
/// ```rust
/// use gardal::{LocalTokenBucket, Limit, StdClock, TryConsumeError};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second_and_burst(
///     NonZeroU32::new(10).unwrap(),
///     NonZeroU32::new(20).unwrap()
/// );
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// // 25 tokens never fit in a bucket with a burst capacity of 20
/// let err = bucket.try_consume(NonZeroU32::new(25).unwrap()).unwrap_err();
/// assert!(matches!(err, TryConsumeError::ExceededBurstCapacity(_)));
/// assert_eq!(err.earliest_retry_after(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TryConsumeError {
    /// Not enough tokens are available right now; retry later.
    RateLimited(RateLimited),
    /// More tokens were requested than the bucket's burst capacity, so the request can
    /// never succeed.
    ExceededBurstCapacity(ExceededBurstCapacity),
}

impl TryConsumeError {
    /// Returns the suggested duration to wait before retrying, or `None` if retrying
    /// can never succeed.
    pub fn earliest_retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited(rate_limited) => Some(rate_limited.earliest_retry_after()),
            Self::ExceededBurstCapacity(_) => None,
        }
    }
}

impl Display for TryConsumeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited(e) => Display::fmt(e, f),
            Self::ExceededBurstCapacity(_) => {
                write!(f, "requested more tokens than the burst capacity")
            }
        }
    }
}

impl std::error::Error for TryConsumeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RateLimited(e) => Some(e),
            Self::ExceededBurstCapacity(e) => Some(e),
        }
    }
}

impl From<RateLimited> for TryConsumeError {
    fn from(e: RateLimited) -> Self {
        Self::RateLimited(e)
    }
}

impl From<ExceededBurstCapacity> for TryConsumeError {
    fn from(e: ExceededBurstCapacity) -> Self {
        Self::ExceededBurstCapacity(e)
    }
}

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
