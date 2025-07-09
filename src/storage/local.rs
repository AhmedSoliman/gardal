use std::cell::Cell;

use super::TimeStorage;

/// Non atomic implementation of [`TimeStorage`]. This is intended for
/// single threaded scenarios and uses [`Cell`] internally.
#[derive(Debug)]
pub struct LocalStorage(Cell<f64>);

impl TimeStorage for LocalStorage {
    fn new(zero_time: f64) -> Self {
        Self(Cell::new(zero_time))
    }

    fn load(&self) -> f64 {
        self.0.get()
    }

    fn store(&self, value: f64) {
        self.0.set(value);
    }

    fn compare_exchange_weak(&self, _current: f64, new: f64) -> Result<(), f64> {
        self.0.set(new);
        Ok(())
    }
}
