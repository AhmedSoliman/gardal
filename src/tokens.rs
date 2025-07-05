use std::num::NonZero;

pub struct Tokens(pub(crate) NonZero<u64>);

impl Tokens {
    pub fn new(value: NonZero<u64>) -> Self {
        let f_value = value.get() as f64;
        Self::new_unchecked(f_value)
    }

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

    pub fn as_u64(&self) -> u64 {
        f64::from_bits(self.0.get()) as u64
    }
}

impl From<Tokens> for f64 {
    fn from(tokens: Tokens) -> Self {
        tokens.as_u64() as f64
    }
}
