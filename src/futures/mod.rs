//! Throttling for async streams.
//!
//! Wrap any [`Stream`] so that it yields items no faster than a [`TokenBucket`] allows.
//! Requires the `async` feature. The throttled streams wait on Tokio timers, so poll them
//! inside a Tokio runtime.
//!
//! # How it works
//!
//! Each time the inner stream produces an item, the throttled stream takes tokens for it:
//! one per item with [`throttle`](StreamExt::throttle), or a weight you choose with
//! [`throttle_weighted`](StreamExt::throttle_weighted). If the bucket has the tokens, the
//! item comes out right away. If it doesn't, the tokens are borrowed and the item is held
//! back until the debt is paid. Items always come out in order, and none are dropped.
//!
//! ```rust
//! use futures::{StreamExt as _, stream};
//! use gardal::futures::StreamExt;
//! use gardal::{Limit, SharedTokenBucket, TokioClock};
//! use std::num::NonZeroU32;
//!
//! # #[tokio::main(flavor = "current_thread", start_paused = true)]
//! # async fn main() {
//! let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), TokioClock);
//!
//! // The first 10 numbers come out right away (the bucket starts full), then 10 per second.
//! let mut numbers = std::pin::pin!(stream::iter(0..15).throttle(bucket));
//! while let Some(n) = numbers.next().await {
//!     println!("{n}");
//! }
//! # }
//! ```
//!
//! # Tips
//!
//! - **One limit for many streams.** Give each stream a clone of the same
//!   [`SharedTokenBucket`](crate::SharedTokenBucket); together they stay within the limit.
//! - **Turning throttling off.** `throttle` takes an `Option`, so passing `None` (or
//!   [`UNLIMITED_BUCKET`](crate::UNLIMITED_BUCKET)) gives an unthrottled stream of the same
//!   type, with no token bucket overhead.
//! - **Pinning.** Throttled streams must be pinned before you call `next()` on them: use
//!   [`std::pin::pin!`], or `Box::pin` / `.boxed()` when the stream needs to be stored.
//! - **Testing.** With [`TokioClock`](crate::TokioClock) and Tokio's paused time
//!   (`#[tokio::test(start_paused = true)]`), the delays finish instantly and tests are
//!   deterministic. This doesn't apply with the `tokio-hrtime` feature, whose timers don't
//!   follow Tokio's paused time.

mod stream;
mod timer;

pub use stream::{ThrottledStream, WeightedStream};

use futures::Stream;
use std::num::NonZeroU32;

use crate::storage::TimeStorage;
use crate::{Clock, TokenBucket};

/// Adds [`throttle`](Self::throttle) and [`throttle_weighted`](Self::throttle_weighted) to
/// every [`Stream`].
///
/// This trait has the same name as `futures::StreamExt`. If you use both, import one of them
/// under another name or as `_`:
///
/// ```rust
/// use futures::StreamExt as _;
/// use gardal::futures::StreamExt;
/// ```
pub trait StreamExt<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Throttles this stream so that each item takes one token from `bucket`.
    ///
    /// Pass `None` to leave the stream unthrottled while keeping the same type.
    ///
    /// ```rust
    /// use futures::stream;
    /// use gardal::futures::StreamExt;
    /// use gardal::{Limit, SharedTokenBucket, TokioClock};
    /// use std::num::NonZeroU32;
    ///
    /// fn events(limit: Option<Limit>) -> impl futures::Stream<Item = u32> {
    ///     let bucket = limit.map(|limit| SharedTokenBucket::new(limit, TokioClock));
    ///     stream::iter(0..100).throttle(bucket)
    /// }
    ///
    /// let throttled = events(Some(Limit::per_second(NonZeroU32::new(5).unwrap())));
    /// let unthrottled = events(None);
    /// ```
    fn throttle(self, bucket: impl Into<Option<TokenBucket<ST, C>>>) -> ThrottledStream<S, ST, C>;

    /// Throttles this stream so that each item takes `weight_fn(&item)` tokens from `bucket`.
    ///
    /// Use it when items have different costs, for example bytes in a message, or rows in a
    /// batch. An item heavier than the bucket's burst is charged the burst.
    ///
    /// ```rust
    /// use futures::{StreamExt as _, stream};
    /// use gardal::futures::StreamExt;
    /// use gardal::{Limit, SharedTokenBucket, TokioClock};
    /// use std::num::NonZeroU32;
    ///
    /// # #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// # async fn main() {
    /// // Up to 1 KiB per second, charging one token per byte.
    /// let bucket = SharedTokenBucket::new(Limit::per_second(NonZeroU32::new(1024).unwrap()), TokioClock);
    ///
    /// let messages = stream::iter(vec![vec![0u8; 100], vec![0u8; 900], vec![0u8; 500]]);
    /// let mut messages = std::pin::pin!(messages.throttle_weighted(bucket, |msg: &Vec<u8>| {
    ///     NonZeroU32::new(msg.len() as u32).unwrap_or(NonZeroU32::MIN)
    /// }));
    /// while let Some(msg) = messages.next().await {
    ///     println!("sending {} bytes", msg.len());
    /// }
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
