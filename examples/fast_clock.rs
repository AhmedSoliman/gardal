//! Using `FastClock`, the cheapest clock, for very hot paths.
//!
//! `FastClock` reads a cached time that a `quanta::Upkeep` thread refreshes, so it is only as
//! precise as the upkeep interval.
//!
//! Run with `cargo run --example fast_clock`.

use std::time::Duration;

use gardal::{FastClock, Limit, SharedTokenBucket};
use nonzero_ext::nonzero;

fn main() {
    // Refresh the cached time every millisecond. Keep the handle alive while the clock is used.
    let _upkeep = quanta::Upkeep::new(Duration::from_millis(1))
        .start()
        .expect("quanta upkeep thread");

    let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
    let bucket = SharedTokenBucket::new(limit, FastClock);

    // Buckets start full.
    assert_eq!(20, bucket.consume(nonzero!(20u32)).unwrap().as_u64());
    println!("took 20 tokens, {:.2} left", bucket.available());

    // At 10 tokens per second, one second refills 10 tokens.
    println!("sleeping for 1 second...");
    std::thread::sleep(Duration::from_secs(1));
    println!("{:.2} tokens available", bucket.available());
    assert_eq!(5, bucket.consume(nonzero!(5u32)).unwrap().as_u64());
}
