use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use pin_project_lite::pin_project;

use super::timer::{Instant, Sleep, sleep};
use crate::{RateLimit, TimeStorage};
use crate::{clock::Clock, raw::RawTokenBucket};

pin_project! {
    /// A stream that is rate limited by a token bucket.
    ///
    /// The stream does not buffer any items, this means that the stream will always
    /// try to consume tokens from the bucket before driving the underlying stream.
    ///
    /// The side effect of this is that the stream might be throttled for 1 extra poll cycle if the
    /// token bucket is empty while the underlying stream has already terminated.
    pub struct LazyRateLimitedStream<S, ST, C>
    where
        S: Stream,
        ST: TimeStorage,
        C: Clock,
    {
        #[pin]
        stream: S,
        bucket: RawTokenBucket<ST, C>,
        #[pin]
        delay: Sleep,
        terminated: bool,
    }
}

impl<S, ST, C> LazyRateLimitedStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Creates a new throttled stream.
    pub fn new(stream: S, bucket: RawTokenBucket<ST, C>) -> Self {
        Self {
            terminated: false,
            stream,
            bucket,
            delay: sleep(Duration::ZERO),
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

    /// Prime the bucket with the burst capacity.
    pub fn prime(&self) {
        self.bucket.add_tokens(self.bucket.limit().burst().get())
    }

    pub fn add_tokens(&self, tokens: impl Into<f64>) {
        self.bucket.add_tokens(tokens)
    }
}

impl<S, ST, C> Stream for LazyRateLimitedStream<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }

        let mut this = self.project();
        loop {
            // are we already waiting for a delay?
            match this.delay.as_mut().poll(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => return Poll::Pending,
            }

            match this.bucket.try_consume_one() {
                Ok(tokens) => {
                    // I've consumed the tokens, but let's say that the stream has nothing to offer,
                    // then we need to put back the tokens in the bucket.
                    match this.stream.poll_next(cx) {
                        Poll::Ready(Some(item)) => {
                            return Poll::Ready(Some(item));
                        }
                        Poll::Ready(None) => {
                            this.bucket.add_tokens(tokens);
                            *this.terminated = true;
                            return Poll::Ready(None);
                        }
                        Poll::Pending => {
                            this.bucket.add_tokens(tokens);
                            return Poll::Pending;
                        }
                    }
                }
                Err(e) => {
                    this.delay
                        .as_mut()
                        .reset(Instant::now() + e.earliest_retry_after());
                }
            }
        }
    }
}

#[cfg(all(test, not(feature = "tokio-hrtime")))]
mod tests {
    use super::*;
    use crate::clock::TokioClock;
    use crate::{LocalStorage, RateLimit};
    use std::time::Duration;

    use futures::stream;
    use nonzero_ext::nonzero;
    use tokio_stream::StreamExt;

    #[tokio::test(start_paused = true)]
    async fn test_throttled_stream() {
        let start = tokio::time::Instant::now();
        let stream = stream::iter(vec![1, 2, 3, 4, 5]);
        let limit = RateLimit::per_second_and_burst(nonzero!(1u32), nonzero!(1u32));
        let bucket = RawTokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(LazyRateLimitedStream::new(stream, bucket));

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
        let bucket = RawTokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(LazyRateLimitedStream::new(stream, bucket));
        throttled_stream.prime();

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
        let bucket = RawTokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(LazyRateLimitedStream::new(stream, bucket));

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
        let bucket = RawTokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

        let mut throttled_stream = std::pin::pin!(LazyRateLimitedStream::new(stream, bucket));
        throttled_stream.prime();

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
