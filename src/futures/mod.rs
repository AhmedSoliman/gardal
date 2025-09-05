//! Async utilities for throttling with futures and streams.
//!
//! This module provides async-friendly wrappers and utilities for throttling
//! in async contexts. It requires the "async" feature to be enabled.
//!
//! # Features
//!
//! - [`ThrottledStream`] - Throttle any stream
//! - [`StreamExt`] - Extension trait for easy stream throttling
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "async")]
//! # {
//! use gardal::{TokenBucket, Limit};
//! use gardal::futures::StreamExt;
//! use futures::stream;
//! use std::num::NonZeroU32;
//!
//! # async fn example() {
//! let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
//! let bucket = TokenBucket::new(limit);
//!
//! let stream = stream::iter(0..100)
//!     .throttle(Some(bucket));
//! # }
//! # }
//! ```

mod stream;
mod timer;

pub use stream::{ThrottledStream, WeightedStream};

use futures::Stream;
use std::num::NonZeroU32;

use crate::storage::TimeStorage;
use crate::{Clock, TokenBucket};

/// Extension trait for adding throttling to any stream.
///
/// This trait provides a convenient way to apply throttling to existing streams
/// without having to manually wrap them.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "async")]
/// # {
/// use gardal::{TokenBucket, Limit};
/// use gardal::futures::StreamExt;
/// use futures::stream;
/// use std::num::NonZeroU32;
///
/// # async fn example() {
/// let limit = Limit::per_second(NonZeroU32::new(5).unwrap());
/// let bucket = TokenBucket::new(limit);
///
/// let throttled = stream::iter(1..=10)
///     .throttle(Some(bucket));
/// # }
/// # }
/// ```
pub trait StreamExt<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Applies throttling to this stream using the provided token bucket.
    ///
    /// # Arguments
    ///
    /// * `bucket` - The token bucket to use for throttling. Pass `None` to disable throttling.
    ///
    /// # Returns
    ///
    /// A [`ThrottledStream`] that wraps this stream with throttling
    fn throttle(self, bucket: impl Into<Option<TokenBucket<ST, C>>>) -> ThrottledStream<S, ST, C>;

    /// Applies weighted throttling to this stream using the provided token bucket.
    ///
    /// Each item in the stream can consume a different number of tokens based on the
    /// weight function. This allows for more sophisticated throttling where different
    /// items have different costs.
    ///
    /// # Arguments
    ///
    /// * `bucket` - The token bucket to use for throttling
    /// * `weight_fn` - A function that determines how many tokens each item consumes
    ///
    /// # Returns
    ///
    /// A [`WeightedStream`] that wraps this stream with weighted throttling
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "async")]
    /// # {
    /// use gardal::{TokenBucket, Limit};
    /// use gardal::futures::StreamExt;
    /// use futures::stream;
    /// use std::num::NonZeroU32;
    ///
    /// # async fn example() {
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = TokenBucket::new(limit);
    ///
    /// let throttled = stream::iter(vec!["small", "large", "medium"])
    ///     .throttle_weighted(bucket, |item: &&str| {
    ///         NonZeroU32::new(item.len() as u32).unwrap_or(NonZeroU32::new(1).unwrap())
    ///     });
    /// # }
    /// # }
    /// ```
    fn throttle_weighted<F>(
        self,
        bucket: TokenBucket<ST, C>,
        weight_fn: F,
    ) -> WeightedStream<S, ST, C, F>
    where
        F: Fn(&S::Item) -> NonZeroU32;
}

impl<S, ST, C> StreamExt<S, ST, C> for S
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    fn throttle(self, bucket: impl Into<Option<TokenBucket<ST, C>>>) -> ThrottledStream<S, ST, C> {
        ThrottledStream::new(self, bucket)
    }

    fn throttle_weighted<F>(
        self,
        bucket: TokenBucket<ST, C>,
        weight_fn: F,
    ) -> WeightedStream<S, ST, C, F>
    where
        F: Fn(&S::Item) -> NonZeroU32,
    {
        WeightedStream::new(self, bucket, weight_fn)
    }
}
