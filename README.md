# Gardal

[![Crates.io](https://img.shields.io/crates/v/gardal.svg)](https://crates.io/crates/gardal)
[![Documentation](https://docs.rs/gardal/badge.svg)](https://docs.rs/gardal)
[![CI](https://github.com/AhmedSoliman/gardal/actions/workflows/ci.yml/badge.svg)](https://github.com/AhmedSoliman/gardal/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0%20OR%20MIT-blue.svg)](http://www.apache.org/licenses/LICENSE-2.0)

A performance-focused token bucket rate limiting and throttling library for Rust with optional async support.

> **Name Origin**: "Gardal" (جردل) is the Egyptian Arabic word for "bucket", reflecting the library's core token bucket algorithm.


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
gardal = "0.1.0"

# For async support
gardal = { version = "0.1.0", features = ["async"] }

# For high-performance timing
gardal = { version = "0.1.0", features = ["quanta"] }

# For high-resolution async timers
gardal = { version = "0.1.0", features = ["async", "tokio-hrtime"] }
```

### Basic Usage

```rust
use gardal::{Limit, LocalTokenBucket, StdClock};
use nonzero_ext::nonzero;

// Create a token bucket: 10 tokens per second, burst of 20
let bucket = LocalTokenBucket::new(Limit::per_second_and_burst(
    nonzero!(10u32),
    nonzero!(20u32),
), StdClock);

// Consume 5 tokens
match bucket.consume(nonzero!(5u32)) {
    Some(tokens) => println!("Consumed {} tokens", tokens.as_u64()),
    None => println!("Not enough tokens available"),
}
```

### Async Stream Rate Limiting

```rust
use futures::{StreamExt, stream};
use gardal::futures::StreamExt as GardalStreamExt;
use gardal::{Limit, SharedTokenBucket, QuantaClock};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let limit = Limit::per_second(nonzero!(5u32));
    let bucket = SharedTokenBucket::new(
        limit, 
        QuantaClock,
    );

    let mut stream = stream::iter(1..=100)
        .throttle(bucket)
        .boxed();

    while let Some(item) = stream.next().await {
        println!("Processed item: {}", item);
    }
}
```

If you want to have an unlimited stream in a type-compatible way, you can pass `None::<TokenBucket>` (or `gardal::UNLIMITED_BUCKET`) to `throttle` and that would avoid any overhead from the token bucket logic.

Use `throttle_weighted` when items have different costs; the closure returns how many tokens each item consumes (see [`weighted_stream.rs`](examples/weighted_stream.rs)). An item that weighs more than the bucket's burst capacity is charged the full burst capacity.

Throttled streams sleep using Tokio timers, so they must be polled from within a Tokio runtime.

## Rate Limit Configuration

Gardal supports various rate limit configurations:

```rust
use gardal::Limit;
use nonzero_ext::nonzero;

// 10 requests per second
let limit = Limit::per_second(nonzero!(10u32));

// 10 requests per second with burst of 20
let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));

// 100 requests per minute
let limit = Limit::per_minute(nonzero!(100u32));

// 1000 requests per hour
let limit = Limit::per_hour(nonzero!(1000u32));
```

## Storage Strategies

Choose the appropriate storage strategy for your use case:

- **`PaddedAtomicStorage`** (default): Thread-safe atomic storage, padded to its own cache line to avoid false sharing
- **`AtomicStorage`**: Thread-safe atomic storage without padding (smaller footprint)
- **`PaddedAtomicSharedStorage`**: Like `PaddedAtomicStorage`, but `Arc`-backed so clones of the bucket share the same token state
- **`AtomicSharedStorage`**: Like `AtomicStorage`, but `Arc`-backed so clones of the bucket share the same token state
- **`LocalStorage`**: Non-atomic (`Cell`-based) storage for single-threaded use; fastest, but not `Sync`

Convenience aliases: `LocalTokenBucket` (`LocalStorage`), `AtomicTokenBucket` (`AtomicStorage`) and `SharedTokenBucket` (`PaddedAtomicSharedStorage`).

```rust
use gardal::{TokenBucket, StdClock, AtomicSharedStorage, Limit};
use nonzero_ext::nonzero;

// Explicitly specify storage type
let bucket = TokenBucket::<AtomicSharedStorage, _>::new(
    Limit::per_second(nonzero!(10u32)),
    StdClock
);
```

## Clock Sources

Gardal supports multiple clock implementations:

- **`StdClock`**: Standard library clock (default)
- **`QuantaClock`**: High-precision clock backed by `quanta::Instant::now()` (requires `quanta` feature)
- **`FastClock`**: Coarse, very cheap clock backed by `quanta::Instant::recent()`; precision depends on a running `quanta::Upkeep` thread (requires `quanta` feature)
- **`TokioClock`**: Tokio-based clock for async applications, respects `tokio::time::pause()` (requires `tokio` feature, also enabled by `async`)
- **`ManualClock`**: Manual clock for testing

`Clock` is also implemented for `&C`, `Box<C>` and `Arc<C>`, so one clock can be shared by many buckets. You can implement `Clock` for your own time source.

### Time Precision

The built-in clocks report time as `f64` seconds since the Unix epoch. This absolute origin lets you persist a bucket's state (`get_zero_time`) and restore it later (`with_zero_time`), even in another process.

The trade-off is resolution: around today's Unix time an `f64` can only represent steps of about 238 ns. Buckets refill continuously, so at `r` tokens per second the smallest observable refill is about `r × 2.4e-7` tokens: negligible for most workloads, but it reaches whole tokens at around 4 million tokens per second. If you need finer resolution than that, implement `Clock` with a recent origin (for example, the process start), at the cost of zero times that are only meaningful within one process.

### High-Resolution Async Timers

For applications requiring precise timing in async contexts, enable the `tokio-hrtime` feature:

```rust
use futures::{StreamExt, stream};
use gardal::futures::StreamExt as GardalStreamExt;
use gardal::{Limit, SharedTokenBucket, TokioClock};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let limit = Limit::per_second(nonzero!(1000u32)); // High-frequency rate limiting
    let bucket = SharedTokenBucket::new(limit, TokioClock);

    let mut stream = stream::iter(1..=10000)
        .throttle(bucket)
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
- [`weighted_stream.rs`](examples/weighted_stream.rs): Weighted stream throttling with per-item costs

## Minimum Supported Rust Version (MSRV)

Gardal requires Rust 1.87.0 or later.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE](https://github.com/AhmedSoliman/gardal/blob/main/LICENSE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/AhmedSoliman/gardal/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
