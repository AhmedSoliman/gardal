# Gardal

[![Crates.io](https://img.shields.io/crates/v/gardal.svg)](https://crates.io/crates/gardal)
[![Documentation](https://docs.rs/gardal/badge.svg)](https://docs.rs/gardal)
[![CI](https://github.com/AhmedSoliman/gardal/actions/workflows/ci.yml/badge.svg)](https://github.com/AhmedSoliman/gardal/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0%20OR%20MIT-blue.svg)](#license)

Fast, lock-free token-bucket rate limiting and throttling for Rust. Borrow against future
capacity, pick the clock and storage that fit your workload, and throttle async streams.

> **Name origin**: "Gardal" (جردل) is Egyptian Arabic for "bucket".

## Highlights

- **Cheap hot path**: consuming tokens is a single atomic compare-and-swap (or a plain `Cell`
  update for single-threaded buckets), with no locks and no allocation.
- **Borrowing**: take tokens now and get told how long to wait until the debt is paid, which
  makes pacing work straightforward.
- **Pluggable clocks**: std, Tokio (including paused time in tests), quanta, a coarse "fast"
  clock, a manual clock for tests, or your own.
- **Pluggable storage**: atomic or single-threaded, padded against false sharing, private or
  shared between clones.
- **Stream throttling**: throttle any `Stream`, optionally with a per-item weight.
- **Restorable**: a bucket's entire state is one `f64`, so it can be persisted and restored.

## Installation

```toml
[dependencies]
gardal = "0.1"
```

Optional features:

| Feature        | Enables                                                                  |
|----------------|--------------------------------------------------------------------------|
| `async`        | Stream throttling (`gardal::futures`) and `TokioClock`                   |
| `tokio`        | `TokioClock` only                                                        |
| `quanta`       | `QuantaClock` and `FastClock`                                            |
| `tokio-hrtime` | Sub-millisecond stream delays using [tokio-hrtime] (use with `async`)    |

For example: `gardal = { version = "0.1", features = ["async"] }`.

The examples below use the `nonzero!` macro from [nonzero_ext] for brevity;
`NonZeroU32::new(10).unwrap()` works just as well.

## Quick start

```rust
use gardal::{Limit, LocalTokenBucket, StdClock};
use nonzero_ext::nonzero;

// 10 tokens per second, holding at most 20.
let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
// `new` starts with a full bucket.
let bucket = LocalTokenBucket::new(limit, StdClock);

match bucket.try_consume(nonzero!(5u32)) {
    Ok(tokens) => println!("consumed {} tokens", tokens.as_u64()),
    Err(err) => println!("not now: {err}"),
}
```

You choose how a bucket starts: `new` starts **full**, so the whole burst can be used right
away, and `new_empty` starts **empty** and fills up at the configured rate, which avoids a
burst right at startup.

## Defining limits

A `Limit` is a refill rate plus a burst capacity: the most tokens the bucket can hold, and so
the most that can be consumed at once after the bucket has been idle.

```rust
use gardal::Limit;
use nonzero_ext::nonzero;

let limit = Limit::per_second(nonzero!(10u32)); // burst defaults to the rate: 10
let limit = Limit::per_second_and_burst(nonzero!(10u32), nonzero!(20u32));
let limit = Limit::per_minute(nonzero!(100u32)); // burst of 100
let limit = Limit::per_hour(nonzero!(1000u32)).with_burst(nonzero!(10u32));
```

## Consuming tokens

| Method                              | When not enough tokens are available                                  |
|-------------------------------------|-----------------------------------------------------------------------|
| `consume(n)`                        | Returns `None` and consumes nothing                                   |
| `try_consume(n)`                    | Returns an error with the time to wait before retrying                |
| `saturating_consume(n)`             | Consumes the whole tokens available, up to `n`                        |
| `consume_with_borrow(n)`            | Consumes `n` anyway, going into debt, and returns how long to wait    |
| `saturating_consume_with_borrow(n)` | Like `consume_with_borrow`, but caps `n` at the burst capacity        |

`consume_one` and `try_consume_one` are shortcuts for a single token, and `add_tokens` returns
tokens to the bucket (for example, from cancelled work). Requests for more tokens than the
burst capacity can never be satisfied: `try_consume` and `consume_with_borrow` report them
as `ExceededBurstCapacity`.

Borrowing suits work that must happen but should be paced:

```rust
use gardal::{Limit, LocalTokenBucket, StdClock};
use nonzero_ext::nonzero;

let bucket = LocalTokenBucket::new(Limit::per_second(nonzero!(10u32)), StdClock);

// The bucket starts full, so the first 10 tokens are free...
assert_eq!(bucket.consume_with_borrow(nonzero!(10u32)).unwrap(), None);
// ...and the next 5 are borrowed: wait about half a second before doing more work.
if let Some(wait) = bucket.consume_with_borrow(nonzero!(5u32)).unwrap() {
    println!("in debt, pausing for {wait:?}");
}
```

## Throttling streams

With the `async` feature, any `Stream` can be throttled:

```rust
use futures::{StreamExt, stream};
use gardal::futures::StreamExt as _;
use gardal::{Limit, SharedTokenBucket, TokioClock};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let bucket = SharedTokenBucket::new(Limit::per_second(nonzero!(5u32)), TokioClock);

    // 5 items right away (the bucket starts full), then 5 per second.
    let mut items = std::pin::pin!(stream::iter(1..=10).throttle(bucket));
    while let Some(item) = items.next().await {
        println!("item {item}");
    }
}
```

Use `throttle_weighted` when items have different costs. An item that weighs more than the
burst capacity is charged the full burst capacity.

```rust
use std::num::NonZeroU32;

use futures::{StreamExt, stream};
use gardal::futures::StreamExt as _;
use gardal::{Limit, SharedTokenBucket, TokioClock};
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() {
    let bucket = SharedTokenBucket::new(Limit::per_second(nonzero!(10u32)), TokioClock);

    let requests = stream::iter([("ping", 1u32), ("upload", 8), ("ping", 1)]);
    let mut requests = std::pin::pin!(
        requests.throttle_weighted(bucket, |&(_, cost)| NonZeroU32::new(cost).unwrap())
    );
    while let Some((name, cost)) = requests.next().await {
        println!("{name} (cost {cost})");
    }
}
```

`throttle` takes an `Option`, so throttling can be switched off without changing the stream's
type, and without any token bucket overhead:

```rust
use futures::{Stream, stream};
use gardal::futures::StreamExt as _;
use gardal::{Limit, SharedTokenBucket, TokioClock};

fn numbers(limit: Option<Limit>) -> impl Stream<Item = u32> {
    stream::iter(0..100).throttle(limit.map(|limit| SharedTokenBucket::new(limit, TokioClock)))
}
```

Throttled streams sleep on Tokio timers, so they must be polled inside a Tokio runtime. Tokio
timers have millisecond granularity; the `tokio-hrtime` feature switches to the operating
system's high-resolution timers through [tokio-hrtime] for sub-millisecond delays (those timers
don't follow Tokio's paused time in tests).

## Storage

| Storage                       | Thread-safe   | Clones share state | Notes                                            |
|-------------------------------|---------------|--------------------|--------------------------------------------------|
| `PaddedAtomicStorage` (default) | yes         | not `Clone`        | Own cache line, so no false sharing              |
| `AtomicStorage`               | yes           | not `Clone`        | Smaller, no padding                              |
| `PaddedAtomicSharedStorage`   | yes           | yes                | `Arc`-backed; used by `SharedTokenBucket`        |
| `AtomicSharedStorage`         | yes           | yes                | `Arc`-backed, no padding                         |
| `LocalStorage`                | no (`!Sync`)  | not `Clone`        | `Cell`-based; fastest for a single thread        |

Buckets with non-shared storage can still be shared by reference or through an `Arc`. The
aliases `LocalTokenBucket`, `AtomicTokenBucket` and `SharedTokenBucket` pick `LocalStorage`,
`AtomicStorage` and `PaddedAtomicSharedStorage` respectively; otherwise name the storage
explicitly:

```rust
use gardal::{AtomicStorage, Limit, StdClock, TokenBucket};
use nonzero_ext::nonzero;

let bucket = TokenBucket::<AtomicStorage, _>::new(Limit::per_second(nonzero!(10u32)), StdClock);
```

## Clocks

| Clock         | Feature            | Source                        | Notes                                                |
|---------------|--------------------|-------------------------------|------------------------------------------------------|
| `StdClock`    |                    | `std::time::Instant`          | Default                                              |
| `TokioClock`  | `tokio` or `async` | `tokio::time::Instant`        | Follows Tokio's paused time in tests                 |
| `QuantaClock` | `quanta`           | `quanta::Instant::now()`      | Typically cheaper to read than `StdClock`            |
| `FastClock`   | `quanta`           | `quanta::Instant::recent()`   | Cheapest; as precise as quanta's upkeep interval     |
| `ManualClock` |                    | Set by you                    | For tests and simulations                            |

`FastClock` reads a cached time that a `quanta::Upkeep` thread refreshes; start one and keep
its handle alive:

```rust
use std::time::Duration;

use gardal::{FastClock, Limit, SharedTokenBucket};
use nonzero_ext::nonzero;

let _upkeep = quanta::Upkeep::new(Duration::from_millis(1))
    .start()
    .expect("quanta upkeep thread");
let bucket = SharedTokenBucket::new(Limit::per_second(nonzero!(1000u32)), FastClock);
```

`Clock` is implemented for `&C`, `Box<C>` and `Arc<C>`, so one clock can serve many buckets,
and you can implement it for your own time source.

### Persisting and time resolution

The built-in clocks report `f64` seconds since the Unix epoch, so a bucket's state is
meaningful across processes: save `get_zero_time()` and rebuild the bucket later with
`with_zero_time`.

```rust
use gardal::{Limit, LocalTokenBucket, StdClock};
use nonzero_ext::nonzero;

let limit = Limit::per_second(nonzero!(10u32));
let bucket = LocalTokenBucket::new(limit, StdClock);
let _ = bucket.consume(nonzero!(5u32));

let saved = bucket.get_zero_time();
// `restored` continues exactly where `bucket` left off, with about 5 tokens.
let restored = LocalTokenBucket::with_zero_time(limit, StdClock, saved);
assert_eq!(restored.get_zero_time(), saved);
```

The trade-off is resolution: near today's Unix time an `f64` moves in steps of about 238 ns.
At `r` tokens per second that is a refill step of about `r × 2.4e-7` tokens, which is
negligible unless you go beyond roughly 4 million tokens per second. If you need more, implement
`Clock` with a recent origin (such as the process start), at the cost of zero times that only
make sense within one process.

## Performance

- Consuming tokens never allocates or locks; with atomic storage it is one compare-and-swap in
  the uncontended case.
- Reading the clock is usually the largest cost: `QuantaClock` and especially `FastClock` are
  cheaper than `StdClock`.
- Padded storage keeps a contended bucket on its own cache line.
- Buckets are generic over storage and clock, so each combination is compiled into
  specialized code with no dynamic dispatch.

Run the benchmarks with `cargo bench`.

## Examples

The [`examples/`](https://github.com/AhmedSoliman/gardal/tree/main/examples) directory has
runnable programs:

- [`basic.rs`](https://github.com/AhmedSoliman/gardal/blob/main/examples/basic.rs): basic token bucket usage
- [`fast_clock.rs`](https://github.com/AhmedSoliman/gardal/blob/main/examples/fast_clock.rs): `FastClock` with a quanta upkeep thread
- [`streams.rs`](https://github.com/AhmedSoliman/gardal/blob/main/examples/streams.rs): throttling many concurrent streams with one shared bucket
- [`weighted_stream.rs`](https://github.com/AhmedSoliman/gardal/blob/main/examples/weighted_stream.rs): weighted stream throttling with per-item costs

Run one with `cargo run --example basic`.

## Minimum supported Rust version

Rust 1.87 or later.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE](https://github.com/AhmedSoliman/gardal/blob/main/LICENSE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/AhmedSoliman/gardal/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Contributions are welcome; please open an issue or a pull request.

[tokio-hrtime]: https://crates.io/crates/tokio-hrtime
[nonzero_ext]: https://crates.io/crates/nonzero_ext
