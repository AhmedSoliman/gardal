use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use futures::{StreamExt, stream};
use gardal::futures::RateLimitedStreamExt;
use gardal::{PaddedAtomicSharedStorage, RateLimit, TokenBucket, TokioClock};
use nonzero_ext::nonzero;
use tokio::task::JoinSet;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let limit = RateLimit::per_second_and_burst(nonzero!(1000000u32), nonzero!(100u32));
    let bucket =
        TokenBucket::<PaddedAtomicSharedStorage, _>::from_parts(limit, TokioClock::default());

    let mut print_start = tokio::time::Instant::now();
    let program_start = print_start;
    let global_processed = Arc::new(AtomicU64::new(0));
    let mut handles = JoinSet::new();
    for i in 1..=100 {
        handles.spawn({
            let bucket = bucket.clone();
            let global_processed = global_processed.clone();
            async move {
                let mut stream1 = std::pin::pin!(stream::repeat(1).rate_limit(bucket));
                // for unthrottled stream those have identical performance.
                // let mut stream1 = std::pin::pin!(stream::repeat(1).rate_limit(None::<TokenBucket>));
                // let mut stream1 = std::pin::pin!(stream::iter(1..=1000000000));
                let mut iter_start = tokio::time::Instant::now();
                let mut processed = 0;
                while let Some(_item) = stream1.next().await {
                    const ITEMS: u64 = 1000000;
                    processed += 1;
                    global_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if processed % ITEMS == 0 {
                        let elapsed = iter_start.elapsed();
                        println!(
                            "  [stream={i}] processed {ITEMS} in {:?} rate={:.2} items/s",
                            elapsed,
                            ITEMS as f64 / elapsed.as_secs_f64()
                        );
                        iter_start = tokio::time::Instant::now();
                        // println!("[stream={i}] item: {}, elapsed={:?}", item, start.elapsed());
                    }
                }
            }
        });
    }

    let mut print_interval = tokio::time::interval(Duration::from_secs(2));

    let mut fut = std::pin::pin!(handles.join_all());
    let mut processed_last_print = 0;
    loop {
        tokio::select! {
            _ = &mut fut => {
                let global = global_processed.load(std::sync::atomic::Ordering::Relaxed);
                println!("Completed {global} in {:?}", program_start.elapsed());
                break;
            }
            _ = print_interval.tick() => {
                    let global = global_processed.load(std::sync::atomic::Ordering::Relaxed);
                    let processed = global - processed_last_print;
                    processed_last_print = global;

                    println!("[total={global}] processed={processed} rate={:.2} items/s", processed as f64 / print_start.elapsed().as_secs_f64());
                    print_start = tokio::time::Instant::now();
                }
        }
    }
}
