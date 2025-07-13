//! Async utilities for rate limiting with futures and streams.
//!
//! This module provides async-friendly wrappers and utilities for rate limiting
//! in async contexts. It requires the "async" feature to be enabled.
//!
//! # Features
//!
//! - [`RateLimitedStream`] - Rate limit any stream
//! - [`RateLimitedStreamExt`] - Extension trait for easy stream rate limiting
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "async")]
//! # {
//! use gardal::{TokenBucket, RateLimit};
//! use gardal::futures::RateLimitedStreamExt;
//! use futures::stream;
//! use std::num::NonZeroU32;
//!
//! # async fn example() {
//! let limit = RateLimit::per_second(NonZeroU32::new(10).unwrap());
//! let bucket = TokenBucket::new(limit);
//!
//! let stream = stream::iter(0..100)
//!     .rate_limit(bucket);
//! # }
//! # }
//! ```

mod stream;
mod timer;

pub use stream::RateLimitedStream;

use futures::Stream;

use crate::storage::TimeStorage;
use crate::{Clock, TokenBucket};

/// Extension trait for adding rate limiting to any stream.
///
/// This trait provides a convenient way to apply rate limiting to existing streams
/// without having to manually wrap them.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "async")]
/// # {
/// use gardal::{TokenBucket, RateLimit};
/// use gardal::futures::RateLimitedStreamExt;
/// use futures::stream;
/// use std::num::NonZeroU32;
///
/// # async fn example() {
/// let limit = RateLimit::per_second(NonZeroU32::new(5).unwrap());
/// let bucket = TokenBucket::new(limit);
///
/// let rate_limited = stream::iter(1..=10)
///     .rate_limit(bucket);
/// # }
/// # }
/// ```
pub trait RateLimitedStreamExt<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Applies rate limiting to this stream using the provided token bucket.
    ///
    /// # Arguments
    ///
    /// * `bucket` - The token bucket to use for rate limiting
    ///
    /// # Returns
    ///
    /// A [`RateLimitedStream`] that wraps this stream with rate limiting
    fn rate_limit(self, bucket: TokenBucket<ST, C>) -> RateLimitedStream<S, ST, C>;
}

impl<S, ST, C> RateLimitedStreamExt<S, ST, C> for S
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    fn rate_limit(self, bucket: TokenBucket<ST, C>) -> RateLimitedStream<S, ST, C> {
        RateLimitedStream::new(self, bucket)
    }
}
