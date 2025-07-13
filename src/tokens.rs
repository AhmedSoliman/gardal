use std::num::NonZero;

/// Represents a non-zero number of consumed tokens.
///
/// This type guarantees that at least one token was consumed, making it safe
/// to use in contexts where zero tokens would be meaningless.
///
/// # Examples
///
/// ```rust
/// use gardal::{TokenBucket, RateLimit};
/// use std::num::NonZeroU32;
///
/// let limit = RateLimit::per_second(NonZeroU32::new(10).unwrap());
/// let bucket = TokenBucket::new(limit);
///
/// if let Some(tokens) = bucket.consume_one() {
///     println!("Consumed {} tokens", tokens.as_u64());
/// }
/// ```
pub struct Tokens(pub(crate) NonZero<u64>);

impl Tokens {
    /// Creates a new `Tokens` instance from a non-zero value.
    ///
    /// # Arguments
    ///
    /// * `value` - A non-zero number of tokens
    pub fn new(value: NonZero<u64>) -> Self {
        let f_value = value.get() as f64;
        Self::new_unchecked(f_value)
    }

    /// Creates a new `Tokens` instance from a floating-point value.
    ///
    /// Returns `None` if the value is zero or negative.
    ///
    /// # Arguments
    ///
    /// * `value` - Number of tokens as a floating-point value
    ///
    /// # Returns
    ///
    /// * `Some(Tokens)` if value > 0
    /// * `None` if value <= 0
    pub fn new_checked(value: f64) -> Option<Self> {
        if value == 0.0 {
            None
        } else {
            Some(Self::new_unchecked(value))
        }
    }

    pub(crate) fn new_unchecked(value: f64) -> Self {
        debug_assert!(value >= 0.0);
        Self(unsafe { NonZero::new_unchecked(value.to_bits()) })
    }

    /// Returns the number of tokens as a 64-bit unsigned integer.
    ///
    /// # Returns
    ///
    /// The token count as a `u64`
    pub fn as_u64(&self) -> u64 {
        f64::from_bits(self.0.get()) as u64
    }
}

impl From<Tokens> for f64 {
    fn from(tokens: Tokens) -> Self {
        tokens.as_u64() as f64
    }
}
