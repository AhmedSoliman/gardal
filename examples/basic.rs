use std::time::Duration;

use gardal::{RateLimit, TokenBucket};
use nonzero_ext::nonzero;

fn main() {
    let tb = TokenBucket::new(RateLimit::per_second_and_burst(
        nonzero!(10u32),
        nonzero!(20u32),
    ));
    // after two seconds bucket should be full
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(5, tb.consume(nonzero!(5u32)).unwrap().as_u64());
}
