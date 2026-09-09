use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cooperative cancellation token.
///
/// The Ctrl+C handler is one adapter that triggers it; tests trigger it
/// directly. Either way, callers observe it through this interface instead of
/// reaching for a process-global flag.
#[derive(Clone, Default, Debug)]
pub struct Interrupt(Arc<AtomicBool>);

impl Interrupt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_triggered(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn trigger(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Closure to hand to `ctrlc::set_handler`.
    pub fn ctrlc_handler(&self) -> impl Fn() + Send + Sync + 'static {
        let token = self.clone();
        move || token.trigger()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggered_token_is_visible_to_clones() {
        let token = Interrupt::new();
        let clone = token.clone();
        assert!(!clone.is_triggered());
        token.trigger();
        assert!(clone.is_triggered());
    }

    #[test]
    fn tokens_are_independent() {
        let a = Interrupt::new();
        let b = Interrupt::new();
        a.trigger();
        assert!(a.is_triggered());
        assert!(!b.is_triggered(), "each run gets its own token");
    }

    #[test]
    fn handler_triggers_the_token() {
        let token = Interrupt::new();
        (token.ctrlc_handler())();
        assert!(token.is_triggered());
    }
}
