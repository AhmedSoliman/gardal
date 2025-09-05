#![doc = include_str!("../README.md")]
//!
//! # Core Components
//!
//! - [`TokenBucket`] - The main token bucket implementation with pluggable storage and clock
//! - [`RateLimit`] - Configuration for rate and burst limits
//! - [`Clock`] trait and implementations for time sources
//! - Storage implementations for different concurrency needs
//!
//! # Quick Start
//!
//! ```rust
//! use std::num::NonZeroU32;
//!
//! use gardal::{TokenBucket, RateLimit};
//!
//! // Create a rate limit: 10 tokens per second, burst of 20
//! let limit = RateLimit::per_second_and_burst(
//!     NonZeroU32::new(10).unwrap(),
//!     NonZeroU32::new(20).unwrap()
//! );
//!
//! // Create token bucket
//! let bucket = TokenBucket::new(limit);
//!
//! // Try to consume tokens
//! if let Some(tokens) = bucket.consume(NonZeroU32::new(5).unwrap()) {
//!     println!("Consumed {} tokens", tokens.as_u64());
//! }
//! ```

mod bucket;
mod clock;
mod error;
#[cfg(feature = "async")]
pub mod futures;
mod limit;
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
pub use futures::RateLimitedStreamExt;
pub use limit::RateLimit;
pub use tokens::Tokens;

pub use storage::{
    TimeStorage, atomic::AtomicSharedStorage, atomic::AtomicStorage, local::LocalStorage,
    padded_atomic::PaddedAtomicSharedStorage, padded_atomic::PaddedAtomicStorage,
};

pub(crate) mod private {
    pub trait Sealed {}
}
