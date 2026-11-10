use std::num::NonZero;

/// Tokens taken from a bucket: always at least one.
///
/// The `consume*` methods return `Tokens` to tell you how many tokens you got. That matters for
/// [`saturating_consume`](crate::TokenBucket::saturating_consume), which can give you fewer than
/// you asked for. Read the count with [`as_u64`](Self::as_u64).
///
/// `Tokens` converts into an `f64`, so tokens you end up not needing can go straight back with
/// [`add_tokens`](crate::TokenBucket::add_tokens):
///
/// ```rust
/// use gardal::{Limit, LocalTokenBucket, StdClock};
/// use std::num::NonZeroU32;
///
/// let bucket = LocalTokenBucket::new(Limit::per_second(NonZeroU32::new(10).unwrap()), StdClock);
///
/// if let Some(tokens) = bucket.consume(NonZeroU32::new(5).unwrap()) {
///     assert_eq!(tokens.as_u64(), 5);
///     // The work was cancelled: give the tokens back.
///     bucket.add_tokens(tokens);
/// }
/// ```
// The token count is stored as the bit pattern of a strictly positive `f64`. For positive
// floats the bit patterns order the same way as the values, so the derived comparisons are
// numeric comparisons.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tokens(pub(crate) NonZero<u64>);

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Tokens")
            .field(&f64::from_bits(self.0.get()))
            .finish()
    }
}

impl Tokens {
    /// Creates `Tokens` holding `value` tokens.
    ///
    /// Buckets create `Tokens` for you; this is mostly useful in tests and wrappers.
    pub fn new(value: NonZero<u64>) -> Self {
        let f_value = value.get() as f64;
        Self::new_unchecked(f_value)
    }

    /// Creates `Tokens` from an `f64`, or `None` if `value` isn't greater than zero.
    pub fn new_checked(value: f64) -> Option<Self> {
        (value > 0.0).then(|| Self::new_unchecked(value))
    }

    pub(crate) fn new_unchecked(value: f64) -> Self {
        // 0.0 has an all-zero bit pattern, which would violate `NonZero`'s invariant.
        debug_assert!(value > 0.0);
        Self(unsafe { NonZero::new_unchecked(value.to_bits()) })
    }

    /// The number of tokens.
    pub fn as_u64(&self) -> u64 {
        f64::from_bits(self.0.get()) as u64
    }
}

impl From<Tokens> for f64 {
    fn from(tokens: Tokens) -> Self {
        tokens.as_u64() as f64
    }
}
