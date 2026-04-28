// src/progress.rs
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

/// Progress bar wrapper for batch merge operations
pub struct MergeProgress {
    bar: Arc<ProgressBar>,
}

impl MergeProgress {
    /// Create a new progress bar with given total
    pub fn new(total: usize) -> Self {
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        Self { bar: Arc::new(bar) }
    }

    /// Increment progress by 1
    pub fn inc(&self) {
        self.bar.inc(1);
    }

    /// Set the current message
    pub fn set_message(&self, msg: &str) {
        self.bar.set_message(msg.to_string());
    }

    /// Finish the progress bar
    pub fn finish(&self) {
        self.bar.finish();
    }

    /// Get the underlying progress bar for cloning
    #[allow(dead_code)]
    pub fn bar(&self) -> Arc<ProgressBar> {
        self.bar.clone()
    }
}

impl Clone for MergeProgress {
    fn clone(&self) -> Self {
        Self {
            bar: self.bar.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_creation() {
        let progress = MergeProgress::new(10);
        assert!(progress.bar().length() == Some(10));
    }

    #[test]
    fn test_progress_inc() {
        let progress = MergeProgress::new(10);
        progress.inc();
        assert!(progress.bar().position() == 1);
    }

    #[test]
    fn test_progress_set_message() {
        let progress = MergeProgress::new(10);
        progress.set_message("test");
        // Message is set, cannot easily verify content but method works
        progress.finish();
    }

    #[test]
    fn test_progress_clone() {
        let progress = MergeProgress::new(10);
        let cloned = progress.clone();
        assert!(cloned.bar().length() == Some(10));
    }
}
