//! Exponential reconnect/retry backoff (CONTEXT.md D-16).

use std::time::Duration;

/// Exponential backoff per D-16: 5s initial, 2x growth, 60s cap. Reset to 5s on success.
/// Used for both the ws.rs reconnect loop and ErrorRecoverable retry_after_ms calculations.
pub struct Backoff {
    current_ms: u64,
}

impl Backoff {
    pub fn new() -> Self {
        Self { current_ms: 5_000 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Duration {
        let d = Duration::from_millis(self.current_ms);
        self.current_ms = (self.current_ms * 2).min(60_000);
        d
    }

    pub fn reset(&mut self) {
        self.current_ms = 5_000;
    }

    pub fn current_ms(&self) -> u64 {
        self.current_ms
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_until_cap() {
        let mut b = Backoff::new();
        assert_eq!(b.next(), Duration::from_millis(5_000));
        assert_eq!(b.next(), Duration::from_millis(10_000));
        assert_eq!(b.next(), Duration::from_millis(20_000));
        assert_eq!(b.next(), Duration::from_millis(40_000));
        assert_eq!(b.next(), Duration::from_millis(60_000)); // cap
        assert_eq!(b.next(), Duration::from_millis(60_000)); // stays capped
        b.reset();
        assert_eq!(b.next(), Duration::from_millis(5_000));
    }
}
