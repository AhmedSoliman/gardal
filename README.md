# Gardal

[![Crates.io](https://img.shields.io/crates/v/gardal.svg)](https://crates.io/crates/gardal)
[![Documentation](https://docs.rs/gardal/badge.svg)](https://docs.rs/gardal)
[![CI](https://github.com/AhmedSoliman/gardal/workflows/CI/badge.svg)](https://github.com/AhmedSoliman/gardal/actions)
[![License](https://img.shields.io/badge/license-Apache%202.0%20OR%20MIT-blue.svg)](http://www.apache.org/licenses/LICENSE-2.0)

A performance-focused token bucket rate limiting and throttling library for Rust with optional async support.

## Features

- **High Performance**: Optimized for minimal overhead with pluggable storage strategies
- **Flexible Clock Sources**: Support for standard, manual, fast (quanta), and Tokio clocks
- **Async Support**: Optional async/await support with stream rate limiting
- **Thread-Safe**: Multiple storage options including atomic and shared storage
- **Zero-Cost Abstractions**: Generic design allows compile-time optimization
- **Configurable**: Support for different rate limits (per second, minute, hour) and burst sizes

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
gardal = "0.0.1-alpha.2"

# For async support
gardal = { version = "0.0.1-alpha.2", features = ["async"] }

# For high-performance timing
gardal = { version = "0.0.1-alpha.2", features = ["quanta"] }

# For high-resolution async timers
gardal = { version = "0.0.1-alpha.2", features = ["async", "tokio-hrtime"] }
```

### Basic Usage

```rust
use gardal::{RateLimit, TokenBucket};
use nonzero_ext::nonzero;

// Create a token bucket: 10 tokens per second, burst of 20
let bucket = TokenBucket::new(RateLimit::per_second_and_burst(
    nonzero!(10u32),
    nonzero!(20u32),
));

// Consume 5 tokens
match bucket.consume(nonzero!(5u32)) {
    Some(tokens) => println!("Consumed {} tokens", tokens.as_u64()),
    None => println!("Not enough tokens available"),
}
```

### Async Stream Rate Limiting

```rust
use futures::{StreamExt, stream};
use gardal::futures::RateLimitedStreamExt;
use gardal::{RateLimit, TokenBucket, AtomicSharedStorage, QuantaClock};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let limit = RateLimit::per_second(nonzero!(5u32));
    let bucket = TokenBucket::<AtomicSharedStorage, _>::from_parts(
        limit, 
        QuantaClock::default()
    );

    let mut stream = stream::iter(1..=100)
        .rate_limit(bucket)
        .boxed();

    while let Some(item) = stream.next().await {
        println!("Processed item: {}", item);
    }
}
```

## Rate Limit Configuration

Gardal supports various rate limit configurations:

```rust
use gardal::RateLimit;
use nonzero_ext::nonzero;

// 10 requests per second
let limit = RateLimit::per_second(nonzero!(10u32));

// 10 requests per second with burst of 20
let limit = RateLimit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));

// 100 requests per minute
let limit = RateLimit::per_minute(nonzero!(100u32));

// 1000 requests per hour
let limit = RateLimit::per_hour(nonzero!(1000u32));
```

## Storage Strategies

Choose the appropriate storage strategy for your use case:

- **`AtomicStorage`**: Basic atomic storage for single-threaded or low-contention scenarios
- **`PaddedAtomicStorage`**: Cache-line padded atomic storage (default) for better performance
- **`AtomicSharedStorage`**: Optimized for high-contention multi-threaded scenarios
- **`LocalStorage`**: Thread-local storage for single-threaded applications

```rust
use gardal::{TokenBucket, AtomicSharedStorage, RateLimit};
use nonzero_ext::nonzero;

// Explicitly specify storage type
let bucket = TokenBucket::<AtomicSharedStorage>::from_parts(
    RateLimit::per_second(nonzero!(10u32)),
    gardal::StdClock::default()
);
```

## Clock Sources

Gardal supports multiple clock implementations:

- **`FastClock`**: High-performance quanta-based clock (requires `quanta` feature)
- **`StdClock`**: Standard library clock (default)
- **`TokioClock`**: Tokio-based clock for async applications (requires `async` feature)
- **`ManualClock`**: Manual clock for testing

### High-Resolution Async Timers

For applications requiring precise timing in async contexts, enable the `tokio-hrtime` feature:

```rust
use futures::{StreamExt, stream};
use gardal::futures::RateLimitedStreamExt;
use gardal::{RateLimit, TokenBucket};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let limit = RateLimit::per_second(nonzero!(1000u32)); // High-frequency rate limiting
    let bucket = TokenBucket::new(limit);

    let mut stream = stream::iter(1..=10000)
        .rate_limit(bucket)
        .boxed();

    // Uses tokio-hrtime for microsecond-precision delays
    while let Some(item) = stream.next().await {
        println!("Processed item: {}", item);
    }
}
```

The `tokio-hrtime` feature provides:
- **Microsecond precision**: More accurate timing than standard Tokio timers
- **Better performance**: Optimized for high-frequency rate limiting scenarios
- **Reduced jitter**: More consistent timing behavior under load

## Performance

Gardal is designed for high-performance scenarios:

- Zero-allocation token consumption in the fast path
- Lock-free atomic operations
- Pluggable storage strategies for different contention patterns
- Optional high-resolution timing with quanta
- Compile-time optimizations through generics

Run benchmarks with:

```bash
cargo bench
```

## Features

- `async`: Enables async/await support and stream rate limiting
- `tokio`: Enables Tokio clock integration
- `quanta`: Enables high-performance timing with the quanta crate
- `tokio-hrtime`: Enables high-resolution async timers for microsecond-precision rate limiting

## Examples

See the [`examples/`](examples/) directory for more usage patterns:

- [`basic.rs`](examples/basic.rs): Basic token bucket usage
- [`fast_clock.rs`](examples/fast_clock.rs): Using high-performance clocks
- [`streams.rs`](examples/streams.rs): Async stream rate limiting

## Minimum Supported Rust Version (MSRV)

Gardal requires Rust 1.87.0 or later.

## License

Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
