//! Maintenance of a multi-pack bank: the master's CID2 0x64 dialog on the CAN socket bus, the pack bus
//! checks around it, and the decision which preparation a flash of the master needs.

pub mod bus;
pub mod dialog;
pub mod policy;
#[cfg(test)]
pub(crate) mod testkit;

use std::time::{Duration, Instant};

/// Monotonic time since an arbitrary start; tests use a virtual clock.
pub trait Clock {
    fn now(&self) -> Duration;
}

pub struct SystemClock(Instant);

impl SystemClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
