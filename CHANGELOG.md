# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-10

First stable release. Changes since `0.0.1-alpha.9`:

### Breaking

- `TokenBucket::try_consume` and `RawTokenBucket::try_consume` now return
  `Result<Tokens, TryConsumeError>`. `TryConsumeError::RateLimited` carries the usual
  `RateLimited`, and `TryConsumeError::ExceededBurstCapacity` reports a request above the burst
  capacity, which can never succeed. `TryConsumeError::earliest_retry_after()` returns
  `Option<Duration>`. `try_consume_one` still returns `RateLimited`.
- `TokenBucket::consume_with_borrow` and `RawTokenBucket::consume_with_borrow` now return
  `Result<Option<Duration>, ExceededBurstCapacity>`. The `Nanos` type is no longer public, and
  `RateLimited::earliest_retry_after_nanos` was removed; use `RateLimited::earliest_retry_after`.
- `Clock` is now implemented for `&C`, `Box<C>` and `Arc<C>` for any `C: Clock`, replacing the
  impls that only covered `&ManualClock` and `Arc<ManualClock>`. Remove your own impls of
  `Clock` for references or `Arc`s of your clocks.
- `ManualClock`'s `now` field is now private; use `ManualClock::set` and `ManualClock::advance`.
- How a new bucket starts is now an explicit choice: `TokenBucket::new` starts full and the new
  `TokenBucket::new_empty` starts empty, both as of the moment the bucket is created.
  `TokenBucket::with_clock` and `TokenBucket::with_datum` were removed (use `new_empty`): they
  started empty as of the clock's first use in the process rather than as of the bucket's
  creation, so a bucket created later could start anywhere between empty and full.
  `Clock::datum`, which only served them, was removed as well.
- `TokenBucket::new` is full with any clock; it used to start empty with a `ManualClock` at
  time `0.0`.
- `RawTokenBucket::new` takes the clock and the limit, and `RawTokenBucket::new_empty` was added.
- `RawTokenBucket::reset` now takes both the current and the new limit:
  `reset(clock, current_limit, new_limit)`.
- `saturating_consume` only consumes whole tokens; a fractional remainder stays in the bucket,
  and it returns `None` when less than one token is available.
- `Tokens::new_checked` returns `None` for negative and NaN values, not only for zero.
- Upgraded `quanta` to 0.13, which requires Rust 1.87. Its default features are now disabled,
  so gardal no longer turns on quanta's `mock` feature for you.
- The `consume*` methods that return `Option` or a tuple are now `#[must_use]`.

### Fixed

- Each built-in clock now sets up its own origin on first use. Before, the first read of any
  clock set up all of them, so with the `quanta` feature even a `StdClock` paid for quanta's
  calibration, and that first reading was taken before the delay, making the next reading jump
  ahead (a bucket could refill by more than a token between two back-to-back calls).
- `RawTokenBucket::reset` computed the bucket's tokens with the new limit's rate, so changing
  the rate changed the number of tokens (for example, 5 tokens at 10/s became 50 at 100/s).
  It also dropped any debt; it now keeps it, like `TokenBucket::reset`.
- `try_consume` with more tokens than the burst capacity panicked: on a debug assertion in debug
  builds, and in `RateLimited::earliest_retry_after` (negative duration) in release builds.
- `WeightedStream` (and `throttle_weighted`) no longer panics when an item weighs more than the
  bucket's burst capacity; the item is charged the full burst capacity instead.
- `saturating_consume` could consume a fraction of a token and return `Tokens` whose
  `as_u64()` was `0`.

### Added

- `Debug` for `TokenBucket`, `RawTokenBucket`, `Tokens`, all clocks and all storage types.
- `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `PartialOrd` and `Ord` for `Tokens`.
- `PartialEq` for `Limit`.
- `TryConsumeError` (see above).
- `Clone`, `Copy`, `PartialEq`, `Eq` and `Hash` for `RateLimited` and `ExceededBurstCapacity`
  (plus `Default` for the latter); `Copy` for the zero-sized clocks.
- Rewritten documentation: guides for choosing how to take tokens, which clock and which
  storage to use, and stream throttling, plus documentation for every public item.
- The MIT license text (`LICENSE-MIT`); the crate has always been `Apache-2.0 OR MIT`.
- docs.rs now documents all features.

### Performance

- gardal no longer enables quanta's `mock` feature, which removes a thread-local clock-override
  lookup from every `QuantaClock` read.

[0.1.0]: https://github.com/AhmedSoliman/gardal/releases/tag/v0.1.0
