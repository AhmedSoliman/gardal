//! The basics: create a bucket, take tokens, and see it refill.
//!
//! Run with `cargo run --example basic`.

use std::time::Duration;

use gardal::{AtomicTokenBucket, Limit, StdClock, TryConsumeError};
use nonzero_ext::nonzero;

fn main() {
    // 10 tokens per second, holding at most 20. Buckets start full.
    let bucket = AtomicTokenBucket::new(
        Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32)),
        StdClock,
    );
    println!("starting with {} tokens", bucket.available());

    // Spend the whole burst at once.
    let tokens = bucket
        .consume(nonzero!(20u32))
        .expect("the bucket starts full");
    println!("took {} tokens", tokens.as_u64());

    // The bucket is empty now: `try_consume` says how long to wait.
    match bucket.try_consume(nonzero!(5u32)) {
        Ok(_) => unreachable!("the bucket was just emptied"),
        Err(TryConsumeError::RateLimited(limited)) => {
            let wait = limited.earliest_retry_after();
            println!("rate limited, waiting {wait:?}");
            std::thread::sleep(wait);
        }
        Err(TryConsumeError::ExceededBurstCapacity(_)) => unreachable!("5 fits in 20"),
    }

    // The wait covered the 5 tokens; sleep a hair longer so rounding can't leave us short.
    std::thread::sleep(Duration::from_millis(10));
    let tokens = bucket.consume(nonzero!(5u32)).expect("refilled by now");
    println!("took {} more tokens after waiting", tokens.as_u64());
}
