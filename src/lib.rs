#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
//!
//! ## API map
//!
//! - [`TokenBucket`]: the rate limiter. Start here; its documentation explains how buckets
//!   work and how to choose between the ways of taking tokens.
//! - [`Limit`]: the rate and the burst.
//! - [`Clock`]: where buckets read the time from, with a guide to the built-in clocks.
//! - [`TimeStorage`]: where buckets keep their state, with a guide to the storage options.
//! - [`Tokens`], [`RateLimited`], [`TryConsumeError`] and [`ExceededBurstCapacity`]: what the
//!   `consume*` methods return.
//! - [`RawTokenBucket`]: a bucket without its clock and limit, for keeping many buckets
//!   cheaply.
//! - `gardal::futures` (`async` feature): throttling for streams.

mod bucket;
mod clock;
mod error;
#[cfg(feature = "async")]
pub mod futures;
mod limit;
mod raw_bucket;
mod storage;
mod tokens;

pub use bucket::{TokenBucket, UNLIMITED_BUCKET};
#[cfg(feature = "tokio")]
pub use clock::TokioClock;
pub use clock::{Clock, ManualClock, StdClock};
#[cfg(feature = "quanta")]
pub use clock::{FastClock, QuantaClock};
pub use error::*;
#[cfg(feature = "async")]
pub use futures::StreamExt;
pub use limit::Limit;
pub use raw_bucket::RawTokenBucket;
pub use tokens::Tokens;

pub use storage::{
    TimeStorage, atomic::AtomicSharedStorage, atomic::AtomicStorage, local::LocalStorage,
    padded_atomic::PaddedAtomicSharedStorage, padded_atomic::PaddedAtomicStorage,
};

/// A single-threaded token bucket backed by [`LocalStorage`] (not `Sync`).
pub type LocalTokenBucket<C> = TokenBucket<LocalStorage, C>;
/// A thread-safe token bucket backed by [`AtomicStorage`].
pub type AtomicTokenBucket<C> = TokenBucket<AtomicStorage, C>;
/// A thread-safe token bucket backed by [`PaddedAtomicSharedStorage`]; clones share
/// the same token state.
pub type SharedTokenBucket<C> = TokenBucket<PaddedAtomicSharedStorage, C>;

pub(crate) mod private {
    pub trait Sealed {}
}
