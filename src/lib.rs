mod clock;
mod error;
#[cfg(feature = "async")]
pub mod futures;
mod limit;
mod raw;
mod storage;
mod tokens;

#[cfg(feature = "tokio")]
pub use clock::TokioClock;
pub use clock::{Clock, ManualClock, StdClock};
#[cfg(feature = "quanta")]
pub use clock::{FastClock, QuantaClock};
pub use error::*;
pub use limit::RateLimit;
pub use raw::RawTokenBucket;
pub use tokens::Tokens;

pub use storage::atomic::{AtomicSharedStorage, AtomicStorage};
pub use storage::local::LocalStorage;
pub use storage::padded_atomic::PaddedAtomicStorage;
