use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;

use crate::clock::Nanos;

/// The bucket doesn't have enough tokens right now.
///
/// [`earliest_retry_after`](Self::earliest_retry_after) tells you how long until it will. It is
/// an estimate: if something else takes tokens from the same bucket in the meantime, your
/// retry can be rate limited again, so retry in a loop.
///
/// ```rust,no_run
/// use gardal::{Limit, LocalTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(1).unwrap()), StdClock);
///
/// for job in 0..3 {
///     // Wait until there's a token for this job.
///     while let Err(limited) = bucket.try_consume_one() {
///         std::thread::sleep(limited.earliest_retry_after());
///     }
///     println!("running job {job}");
/// }
/// ```
///
/// In async code, sleep with your runtime's timer instead, or let
/// `gardal::futures` do the waiting for streams.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RateLimited {
    pub(crate) earliest_retry_time: Nanos,
}

/// The request asks for more tokens than the bucket can ever hold.
///
/// A bucket never holds more than its burst, so retrying won't help. Split the work into smaller
/// requests, raise the burst with [`Limit::with_burst`](crate::Limit::with_burst), or use the
/// `saturating_*` methods, which cap the request at the burst.
///
/// Returned by [`consume_with_borrow`](crate::TokenBucket::consume_with_borrow), and by
/// [`try_consume`](crate::TokenBucket::try_consume) inside a [`TryConsumeError`].
///
/// ```rust
/// use gardal::{ExceededBurstCapacity, Limit, LocalTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second_and_burst(NonZeroU32::new(10).unwrap(), NonZeroU32::new(20).unwrap());
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// assert_eq!(
///     bucket.consume_with_borrow(NonZeroU32::new(25).unwrap()),
///     Err(ExceededBurstCapacity)
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExceededBurstCapacity;

impl RateLimited {
    /// How long until the bucket has the requested tokens, if nobody else takes them first.
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

/// Why [`try_consume`](crate::TokenBucket::try_consume) couldn't take the tokens.
///
/// Either the bucket doesn't have them yet ([`RateLimited`](Self::RateLimited): wait and retry),
/// or it never will ([`ExceededBurstCapacity`](Self::ExceededBurstCapacity): retrying won't
/// help). [`earliest_retry_after`](Self::earliest_retry_after) covers both cases at once.
///
/// ```rust
/// use gardal::{Limit, LocalTokenBucket, StdClock, TryConsumeError};
/// use std::num::NonZeroU32;
///
/// let limit = Limit::per_second_and_burst(NonZeroU32::new(10).unwrap(), NonZeroU32::new(20).unwrap());
/// let bucket = LocalTokenBucket::new(limit, StdClock);
///
/// // 25 tokens never fit in a bucket that holds at most 20.
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
    /// How long to wait before retrying, or `None` if retrying can never succeed.
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
