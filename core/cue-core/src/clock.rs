use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;

/// Monotonic millisecond clock used by the engine.
///
/// Times are relative to an arbitrary epoch and must not move backwards.
pub trait Clock: Send + Sync {
    /// Returns the current monotonic time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// A clock backed by [`Instant`].
#[derive(Debug, Clone)]
pub struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    /// Creates a clock whose current value starts near zero.
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        let millis = self.epoch.elapsed().as_millis();
        u64::try_from(millis).unwrap_or(u64::MAX)
    }
}

/// A clonable, manually advanced clock for deterministic tests and simulations.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now_ms: Arc<AtomicU64>,
}

impl ManualClock {
    /// Creates a manual clock at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms: Arc::new(AtomicU64::new(now_ms)),
        }
    }

    /// Sets the current time.
    ///
    /// The engine separately rejects attempts to tick backwards.
    pub fn set(&self, now_ms: u64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    /// Advances the clock by `delta_ms`, saturating at [`u64::MAX`].
    pub fn advance(&self, delta_ms: u64) {
        let _ = self
            .now_ms
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                Some(now.saturating_add(delta_ms))
            });
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, ManualClock};

    #[test]
    fn manual_clocks_share_time() {
        let clock = ManualClock::new(10);
        let clone = clock.clone();
        clone.advance(15);
        assert_eq!(clock.now_ms(), 25);
        clock.set(40);
        assert_eq!(clone.now_ms(), 40);
    }
}
