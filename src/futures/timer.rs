#[cfg(feature = "tokio-hrtime")]
pub use tokio_hrtime::{Sleep, sleep};

#[cfg(all(feature = "async", not(feature = "tokio-hrtime")))]
pub use tokio::time::{Sleep, sleep};
