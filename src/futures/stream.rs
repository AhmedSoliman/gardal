use std::marker::PhantomPinned;
use std::num::NonZeroU32;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use futures::Stream;
use pin_project_lite::pin_project;

use super::timer::{Sleep, sleep};
use crate::Limit;
use crate::storage::TimeStorage;
use crate::{bucket::TokenBucket, clock::Clock};
#[cfg(not(feature = "tokio-hrtime"))]
use tokio::time::Instant;

pin_project! {
    /// A stream wrapper that applies throttling using a token bucket.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::{TokenBucket, Limit};
    /// use gardal::futures::ThrottledStream;
    /// use futures::stream;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second(NonZeroU32::new(10).unwrap());
    /// let bucket = TokenBucket::new(limit);
    /// let stream = stream::iter(0..100);
    ///
    /// let throttled = ThrottledStream::new(stream, bucket);
    /// ```
    pub struct ThrottledStream<S, ST, C>
    where
        S: Stream,
        ST: TimeStorage,
        C: Clock,
    {
        #[pin]
        weighted_stream: WeightedStream<S, ST, C, fn(&S::Item) -> NonZeroU32>,
    }
}

impl<S, ST, C> ThrottledStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Creates a new rate-limited stream.
    ///
    /// # Arguments
    ///
    /// * `stream` - The underlying stream to throttle
    /// * `bucket` - The token bucket to use for throttling
    pub fn new(stream: S, bucket: impl Into<Option<TokenBucket<ST, C>>>) -> Self {
        let unit_weight_fn: fn(&S::Item) -> NonZeroU32 =
            |_| unsafe { NonZeroU32::new_unchecked(1) };
        Self {
            weighted_stream: WeightedStream::new(stream, bucket, unit_weight_fn),
        }
    }

    /// Returns a reference to the current throttling configuration.
    pub fn limit(&self) -> Option<&Limit> {
        self.weighted_stream.limit()
    }

    /// Returns the number of tokens currently available in the bucket.
    ///
    /// Returns zero if the bucket is in debt from borrowing.
    pub fn available(&self) -> f64 {
        self.weighted_stream.available()
    }

    /// Adds tokens back to the bucket.
    ///
    /// Useful for returning tokens from cancelled operations or manually
    /// adding capacity.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Number of tokens to add
    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        self.weighted_stream.add_tokens(tokens)
    }

    /// Consumes the throttled stream, returning the underlying stream.
    pub fn into_inner(self) -> S {
        self.weighted_stream.into_inner()
    }

    /// Returns a reference to the underlying stream.
    pub fn inner(&self) -> &S {
        self.weighted_stream.inner()
    }

    /// Returns a reference to the underlying stream.
    pub fn inner_mut(&mut self) -> &mut S {
        self.weighted_stream.inner_mut()
    }

    /// Returns a reference to the underlying stream.
    pub fn inner_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        self.project().weighted_stream.inner_pin_mut()
    }
}

impl<S, ST, C> Stream for ThrottledStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().weighted_stream.poll_next(cx)
    }
}

pin_project! {
    /// A stream that is throttled by a token bucket with weighted consumption.
    /// Each item can consume a different number of tokens based on a weight function.
    pub struct WeightedStream<S, ST, C, F>
    where
        S: Stream,
        ST: TimeStorage,
        C: Clock,
        F: Fn(&S::Item) -> NonZeroU32,
    {
        #[pin]
        stream: S,
        bucket: Option<TokenBucket<ST, C>>,
        weight_fn: F,
        #[pin]
        delay: Option<Sleep>,
        pending_item: Option<S::Item>,
        // Make sure we are always !Unpin even if using tokio-hrtime for API consistency
        _marker: PhantomPinned,
    }
}

impl<S, ST, C, F> WeightedStream<S, ST, C, F>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
    F: Fn(&S::Item) -> NonZeroU32,
{
    /// Creates a new weighted stream.
    ///
    /// # Arguments
    ///
    /// * `stream` - The underlying stream to throttle
    /// * `bucket` - The token bucket for throttling
    /// * `weight_fn` - A function that determines how many tokens each item consumes
    ///
    /// # Examples
    ///
    /// ```rust
    /// use gardal::futures::WeightedStream;
    /// use gardal::{LocalStorage, Limit, TokioClock, TokenBucket};
    /// use futures::stream;
    /// use std::num::NonZeroU32;
    ///
    /// let limit = Limit::per_second_and_burst(NonZeroU32::new(10).unwrap(), NonZeroU32::new(10).unwrap());
    /// let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());
    ///
    /// let stream = stream::iter(vec!["small", "large", "medium"]);
    /// let weighted_stream = WeightedStream::new(stream, bucket, |item: &&str| {
    ///     NonZeroU32::new(item.len() as u32).unwrap_or(NonZeroU32::new(1).unwrap())
    /// });
    /// ```
    pub fn new(stream: S, bucket: impl Into<Option<TokenBucket<ST, C>>>, weight_fn: F) -> Self {
        Self {
            stream,
            bucket: bucket.into(),
            weight_fn,
            delay: None,
            pending_item: None,
            _marker: PhantomPinned,
        }
    }

    /// Returns a reference to the current throttling configuration.
    pub fn limit(&self) -> Option<&Limit> {
        self.bucket.as_ref().map(|b| b.limit())
    }

    /// Returns the number of tokens currently available in the bucket.
    ///
    /// Returns zero if the bucket is in debt from borrowing.
    pub fn available(&self) -> f64 {
        if let Some(bucket) = &self.bucket {
            bucket.available()
        } else {
            f64::MAX
        }
    }

    /// Adds tokens back to the bucket.
    ///
    /// Useful for returning tokens from cancelled operations or manually
    /// adding capacity.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Number of tokens to add
    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        if let Some(bucket) = &self.bucket {
            bucket.add_tokens(tokens)
        }
    }

    /// Returns true if the throttled stream has staged pending item to be delivered.
    pub fn has_pending_item(&self) -> bool {
        self.pending_item.is_some()
    }

    /// Consumes the throttled stream, returning the underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Returns a reference to the underlying stream.
    pub fn inner(&self) -> &S {
        &self.stream
    }

    /// Returns a reference to the underlying stream.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// Returns a reference to the underlying stream.
    pub fn inner_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        self.project().stream
    }
}

impl<S, ST, C, F> Stream for WeightedStream<S, ST, C, F>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
    F: Fn(&S::Item) -> NonZeroU32,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let coop = ready!(tokio::task::coop::poll_proceed(cx));
        if let Some(bucket) = &this.bucket {
            // If we have a pending item and delay, wait for the delay to complete
            if this.pending_item.is_some() {
                if let Some(delay) = this.delay.as_mut().as_pin_mut() {
                    ready!(delay.poll(cx));
                    this.delay.set(None);
                }
                // Return the pending item
                return Poll::Ready(this.pending_item.take());
            }

            // Get the next item from the underlying stream
            let next_item = ready!(this.stream.poll_next(cx));

            match next_item {
                Some(item) => {
                    let weight = (this.weight_fn)(&item);
                    match bucket.consume_with_borrow(weight).unwrap() {
                        Some(duration) => {
                            // Store the item and set up delay
                            *this.pending_item = Some(item);
                            #[cfg(feature = "tokio-hrtime")]
                            {
                                this.delay.set(Some(sleep(Duration::from(duration))));
                            }
                            #[cfg(not(feature = "tokio-hrtime"))]
                            {
                                if let Some(delay) = this.delay.as_mut().as_pin_mut() {
                                    delay.reset(Instant::now() + Duration::from(duration));
                                } else {
                                    this.delay.set(Some(sleep(Duration::from(duration))));
                                }
                            }
                            // Wake up to continue polling
                            cx.waker().wake_by_ref();

                            coop.made_progress();
                            Poll::Pending
                        }
                        None => {
                            coop.made_progress();
                            // No delay needed, return item immediately
                            Poll::Ready(Some(item))
                        }
                    }
                }
                None => Poll::Ready(None),
            }
        } else {
            this.stream.poll_next(cx).map(|item| {
                coop.made_progress();
                item
            })
        }
    }
}

#[cfg(all(test, not(feature = "tokio-hrtime")))]
mod tests {
    use super::*;
    use crate::Limit;
    use crate::clock::TokioClock;
    use crate::storage::local::LocalStorage;
    use std::time::Duration;

    use futures::stream;
    use nonzero_ext::nonzero;
    use tokio_stream::StreamExt;

    #[tokio::test(start_paused = true)]
    async fn test_throttled_stream() {
        let start = tokio::time::Instant::now();
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(ThrottledStream::new(stream, bucket));

        let mut results = vec![];
        while let Some(item) = throttled_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        assert!(elapsed >= Duration::from_secs(4));
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttled_stream_burst() {
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(3u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(ThrottledStream::new(stream, bucket));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = throttled_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        assert!(elapsed >= Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttled_stream_all_ready() {
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second(nonzero!(100000u32)).with_burst(nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(ThrottledStream::new(stream, bucket));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = throttled_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttled_slow() {
        let stream = stream::iter(vec![1, 2, 3, 4, 5])
            .throttle(Duration::from_secs(2))
            .chain(stream::iter(vec![6, 7, 8, 9]));
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(3u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(ThrottledStream::new(stream, bucket));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = throttled_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(elapsed < Duration::from_secs(14));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_uniform_weight() {
        let start = tokio::time::Instant::now();
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream =
            std::pin::pin!(WeightedStream::new(stream, bucket, |_| nonzero!(1u32)));

        let mut results = vec![];
        while let Some(item) = weighted_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        assert!(elapsed >= Duration::from_secs(4));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_variable_weight() {
        let start = tokio::time::Instant::now();
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second_and_burst(nonzero!(2u32), nonzero!(2u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream = std::pin::pin!(WeightedStream::new(stream, bucket, |&item| {
            if item % 2 == 0 {
                nonzero!(2u32) // Even numbers consume 2 tokens
            } else {
                nonzero!(1u32) // Odd numbers consume 1 token
            }
        }));

        let mut results = vec![];
        while let Some(item) = weighted_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        // Total tokens needed: 1 + 2 + 1 + 2 + 1 = 7 tokens
        // With 2 tokens/sec and burst of 2, should take at least 2.5 seconds
        assert!(elapsed >= Duration::from_millis(2500));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_with_burst() {
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(5u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream = std::pin::pin!(WeightedStream::new(stream, bucket, |&item| {
            NonZeroU32::new(item as u32).unwrap_or(nonzero!(1u32))
        }));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = weighted_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5]);
        // Total tokens: 1 + 2 + 3 + 4 + 5 = 15 tokens
        // With burst of 5, first 5 tokens are immediate, then need 10 more at 1/sec
        assert!(elapsed >= Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_empty() {
        let stream = stream::iter(Vec::<i32>::new());
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream =
            std::pin::pin!(WeightedStream::new(stream, bucket, |_| nonzero!(1u32)));

        let mut results = vec![];
        while let Some(item) = weighted_stream.next().await {
            results.push(item);
        }

        assert_eq!(results, Vec::<i32>::new());
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_single_item() {
        let stream = stream::iter(vec![42]);
        let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(10u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream =
            std::pin::pin!(WeightedStream::new(stream, bucket, |_| nonzero!(3u32)));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = weighted_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![42]);
        // Item should be delivered immediately since we have plenty of burst capacity
        assert!(elapsed < Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_expensive_item_delayed() {
        // This test verifies that expensive items are properly delayed before being returned
        let stream = stream::iter(vec![10]); // Single expensive item
        let limit = Limit::per_second_and_burst(nonzero!(1u32), nonzero!(10u32)); // Enough burst for the item
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream = std::pin::pin!(WeightedStream::new(stream, bucket, |&item| {
            NonZeroU32::new(item as u32).unwrap_or(nonzero!(1u32)) // Item value determines token cost
        }));

        let start = tokio::time::Instant::now();
        let mut results = vec![];
        while let Some(item) = weighted_stream.next().await {
            let elapsed_at_delivery = start.elapsed();
            results.push((item, elapsed_at_delivery));
        }

        assert_eq!(results.len(), 1);
        let (item, delivery_time) = results[0];
        assert_eq!(item, 10);

        // The expensive item (10 tokens) should be delayed significantly
        // With 1 token/sec rate and 10 token burst, it should take ~9 seconds
        assert!(delivery_time >= Duration::from_secs(9));
    }

    #[tokio::test(start_paused = true)]
    async fn test_weighted_stream_mixed_items_correct_timing() {
        // Test that cheap items come quickly and expensive items are delayed appropriately
        let stream = stream::iter(vec![1, 5, 1]); // cheap, expensive, cheap
        let limit = Limit::per_second_and_burst(nonzero!(2u32), nonzero!(10u32)); // Enough burst capacity
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut weighted_stream = std::pin::pin!(WeightedStream::new(stream, bucket, |&item| {
            NonZeroU32::new(item as u32).unwrap_or(nonzero!(1u32))
        }));

        let start = tokio::time::Instant::now();
        let mut results = vec![];
        while let Some(item) = weighted_stream.next().await {
            let elapsed_at_delivery = start.elapsed();
            results.push((item, elapsed_at_delivery));
        }

        assert_eq!(results.len(), 3);

        // First item (1 token) should be immediate (burst capacity)
        let (item1, time1) = results[0];
        assert_eq!(item1, 1);
        assert!(time1 < Duration::from_secs(1)); // More lenient timing

        // Second item (5 tokens) should be delayed significantly
        let (item2, time2) = results[1];
        assert_eq!(item2, 5);
        // After consuming 1 token, we need 4 more tokens at 2/sec = 2 seconds
        assert!(time2 >= Duration::from_secs(2));

        // Third item (1 token) should come after additional delay
        let (item3, time3) = results[2];
        assert_eq!(item3, 1);
        assert!(time3 > time2);
    }
}
