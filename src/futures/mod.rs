mod lazy_stream;
mod timer;

pub use lazy_stream::LazyRateLimitedStream;

use futures::Stream;

use crate::{Clock, RawTokenBucket, TimeStorage};

pub trait LazyRateLimitedStreamExt<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    /// Create a stream that is rate limited by a token bucket. The stream does not buffer any items,
    /// this means that the stream will always try to consume tokens from the bucket before driving
    /// the underlying stream.
    ///
    /// The side effect of this is that the stream might be throttled for 1 extra poll cycle if the
    /// token bucket is empty while the underlying stream has already terminated.
    fn lazy_rate_limited(self, bucket: RawTokenBucket<ST, C>) -> LazyRateLimitedStream<S, ST, C>;
}

impl<S, ST, C> LazyRateLimitedStreamExt<S, ST, C> for S
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
    fn lazy_rate_limited(self, bucket: RawTokenBucket<ST, C>) -> LazyRateLimitedStream<S, ST, C> {
        LazyRateLimitedStream::new(self, bucket)
    }
}
