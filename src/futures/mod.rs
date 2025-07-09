mod stream;
mod timer;

pub use stream::RateLimitedStream;

use futures::Stream;

use crate::storage::TimeStorage;
use crate::{Clock, TokenBucket};

pub trait RateLimitedStreamExt<S, ST, C>
where
    S: Stream,
    ST: TimeStorage,
    C: Clock,
{
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
