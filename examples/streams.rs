use std::time::Duration;

use futures::{StreamExt, stream};
use gardal::futures::RateLimitedStreamExt;
use gardal::{AtomicSharedStorage, FastClock, RateLimit, TokenBucket};
use nonzero_ext::nonzero;
use tokio::task::JoinSet;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let clock = quanta::Clock::new();
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_micros(150), clock.clone())
        .start()
        .unwrap();
    let clock = FastClock::new(clock);

    let limit = RateLimit::per_second_and_burst(nonzero!(5u32), nonzero!(5u32));
    let bucket = TokenBucket::<AtomicSharedStorage, _>::from_parts(limit, clock);

    let start = tokio::time::Instant::now();
    let mut handles = JoinSet::new();
    for i in 1..=10 {
        handles.spawn({
            let bucket = bucket.clone();
            async move {
                let mut stream1 = std::pin::pin!(stream::iter(1..=100).rate_limit(bucket));
                while let Some(item) = stream1.next().await {
                    println!("[stream={i}] item: {}, elapsed={:?}", item, start.elapsed());
                }
            }
        });
    }

    handles.join_all().await;
    println!("Completed in {:?}", start.elapsed());
}
