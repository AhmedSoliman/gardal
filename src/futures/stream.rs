use std::num::NonZeroU32;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use futures::Stream;
use pin_project_lite::pin_project;

use super::timer::{Sleep, sleep};
use crate::RateLimit;
use crate::storage::TimeStorage;
use crate::{clock::Clock, bucket::TokenBucket};
#[cfg(not(feature = "tokio-hrtime"))]
use tokio::time::Instant;

pin_project! {
    /// A stream that is rate limited by a token bucket.
    pub struct RateLimitedStream<S, ST, C>
    where
        S: Stream,
        ST: TimeStorage,
        C: Clock,
    {
        #[pin]
        stream: S,
        bucket: TokenBucket<ST, C>,
        #[pin]
        delay: Option<Sleep>,
        ready_to_consume: bool,
    }
}

impl<S, ST, C> RateLimitedStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Creates a new throttled stream.
    pub fn new(stream: S, bucket: TokenBucket<ST, C>) -> Self {
        Self {
            stream,
            bucket,
            ready_to_consume: true,
            delay: None,
        }
    }

    /// Returns the current rate limit.
    pub fn limit(&self) -> &RateLimit {
        self.bucket.limit()
    }

    /// Available tokens in the bucket. Zero if in debt.
    pub fn available(&self) -> f64 {
        self.bucket.available()
    }

    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        self.bucket.add_tokens(tokens)
    }
}

impl<S, ST, C> Stream for RateLimitedStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        // are we already waiting for a delay?

        if let Some(delay) = this.delay.as_mut().as_pin_mut() {
            ready!(delay.poll(cx));
        }

        // deliver immediately
        let next_item = ready!(this.stream.poll_next(cx));

        match this
            .bucket
            .consume_with_borrow(unsafe { NonZeroU32::new_unchecked(1) })
            .unwrap()
        {
            Some(duration) => {
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
            }
            None => {
                this.delay.set(None);
            }
        }
        Poll::Ready(next_item)
    }
}

#[cfg(all(test, not(feature = "tokio-hrtime")))]
mod tests {
    use super::*;
    use crate::RateLimit;
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
        let limit = RateLimit::per_second_and_burst(nonzero!(1u32), nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(RateLimitedStream::new(stream, bucket));

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
        let limit = RateLimit::per_second_and_burst(nonzero!(1u32), nonzero!(3u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(RateLimitedStream::new(stream, bucket));

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
        let limit = RateLimit::per_second(nonzero!(100000u32)).with_burst(nonzero!(1u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(RateLimitedStream::new(stream, bucket));

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
        let limit = RateLimit::per_second_and_burst(nonzero!(1u32), nonzero!(3u32));
        let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(RateLimitedStream::new(stream, bucket));

        let mut results = vec![];
        let start = tokio::time::Instant::now();
        while let Some(item) = throttled_stream.next().await {
            results.push(item);
        }
        let elapsed = start.elapsed();

        assert_eq!(results, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(elapsed < Duration::from_secs(14));
    }
}
