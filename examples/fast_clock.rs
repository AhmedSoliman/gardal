use std::time::Duration;

use gardal::{FastClock, RateLimit, TokenBucket};
use nonzero_ext::nonzero;

fn main() {
    let clock = quanta::Clock::new();
    // Updates at 1Khz
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_millis(1), clock.clone())
        .start()
        .unwrap();
    let clock = FastClock::new(clock);
    let limit = RateLimit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
    let tb = TokenBucket::with_clock(limit, clock);
    // after two seconds bucket should be full
    println!("sleeping for 2 seconds...");
    std::thread::sleep(Duration::from_secs(2));
    println!("available: {}", tb.available());
    assert_eq!(5, tb.consume(nonzero!(5u32)).unwrap().as_u64());
    println!("Consumed 5, available: {}", tb.available());
}
