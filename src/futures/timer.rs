#[cfg(feature = "tokio-hrtime")]
pub use tokio_hrtime::{Instant, Sleep, sleep};

#[cfg(all(feature = "async", not(feature = "tokio-hrtime")))]
pub use tokio::time::{Instant, Sleep, sleep};
