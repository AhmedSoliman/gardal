use crate::{RateLimited, Tokens};

pub trait TokenBucket {
    fn consume(&self, to_consume: f64) -> Option<Tokens>;
    fn try_consume(&self, to_consume: f64) -> Result<Tokens, RateLimited>;
    fn saturating_consume(&self, to_consume: f64) -> Option<Tokens>;
    fn add_tokens(&self, tokens: impl Into<f64>);
}
