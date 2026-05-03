use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

/// Internal rendering strategy.
enum Renderer {
    /// Full progress bar with speed/ETA (TTY mode).
    Bar(Arc<ProgressBar>),
    /// One-line-per-file text output (no TTY / CI mode).
    Text {
        total: usize,
        completed: std::sync::atomic::AtomicUsize,
    },
}

impl Renderer {
    /// Create a text-mode renderer.
    #[allow(dead_code)]
    fn new_text(total: usize) -> Self {
        Self::Text {
            total,
            completed: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[allow(dead_code)]
    fn record(
        &self,
        stem: &str,
        success: bool,
        duration: Duration,
        error: Option<&str>,
        retry: Option<usize>,
    ) {
        match self {
            Self::Bar(bar) => {
                let msg = format_record(stem, success, duration, error, retry, false);
                bar.set_message(msg);
                bar.inc(1);
            }
            Self::Text { .. } => {
                let line = format_record(stem, success, duration, error, retry, true);
                println!("{}", line);
            }
        }
    }

    fn update_message(&self, msg: &str) {
        match self {
            Self::Bar(bar) => bar.set_message(msg.to_string()),
            Self::Text { .. } => {} // Text mode doesn't update mid-file
        }
    }

    fn finish(&self) {
        match self {
            Self::Bar(bar) => {
                bar.finish();
            }
            Self::Text { total, completed } => {
                completed.store(*total, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

/// Format a record line. If `colored`, adds ANSI color codes.
fn format_record(
    stem: &str,
    success: bool,
    duration: Duration,
    error: Option<&str>,
    retry: Option<usize>,
    colored: bool,
) -> String {
    let time = format_duration(duration);
    match (success, retry, error) {
        (true, _, _) => {
            let sym = if colored {
                "✓".green()
            } else {
                "✓".normal()
            };
            format!("{} {} ({})", sym, stem, time)
        }
        (false, Some(r), _) => {
            let sym = if colored {
                "↻".yellow()
            } else {
                "↻".normal()
            };
            format!("{} {} retry {} ({})", sym, stem, r, time)
        }
        (false, None, Some(e)) => {
            let sym = if colored { "✗".red() } else { "✗".normal() };
            format!("{} {}: {}", sym, stem, e)
        }
        (false, None, None) => {
            let sym = if colored { "✗".red() } else { "✗".normal() };
            format!("{} {} ({})", sym, stem, time)
        }
    }
}

/// Compact duration format: ms, s, or m.
pub(crate) fn format_duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{}ms", d.as_millis())
    } else if d < Duration::from_secs(60) {
        format!("{:.2}s", d.as_secs_f64())
    } else {
        format!("{}m {:.0}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

/// Progress bar wrapper for batch merge operations.
pub struct MergeProgress {
    inner: Renderer,
}

impl MergeProgress {
    /// Create a new progress renderer. Auto-detects TTY.
    pub fn new(total: usize) -> Self {
        if std::io::stderr().is_terminal() {
            let bar = ProgressBar::new(total as u64);
            bar.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] {bar:30.cyan/blue} {pos}/{len} ({per_sec}) {msg}",
                )
                .unwrap()
                .progress_chars("=>-")
                .with_key(
                    "per_sec",
                    |state: &indicatif::ProgressState, w: &mut dyn std::fmt::Write| {
                        let elapsed = state.elapsed().as_secs_f64();
                        if elapsed > 0.0 {
                            let _ = write!(w, "{:.1} files/s", state.pos() as f64 / elapsed);
                        } else {
                            let _ = write!(w, "0.0 files/s");
                        }
                    },
                ),
            );
            Self {
                inner: Renderer::Bar(Arc::new(bar)),
            }
        } else {
            Self {
                inner: Renderer::new_text(total),
            }
        }
    }

    /// Create a text-mode renderer explicitly (for testing or forced text output).
    #[allow(dead_code)]
    pub fn new_text(total: usize) -> Self {
        Self {
            inner: Renderer::new_text(total),
        }
    }

    /// Record a completed file with timing and status.
    #[allow(dead_code)]
    pub fn record(
        &self,
        stem: &str,
        success: bool,
        duration: Duration,
        error: Option<&str>,
        retry: Option<usize>,
    ) {
        self.inner.record(stem, success, duration, error, retry);
    }

    /// Update the status message without advancing progress (for retries).
    pub fn set_message(&self, msg: &str) {
        self.inner.update_message(msg);
    }

    /// Finish the progress renderer.
    pub fn finish(&self) {
        self.inner.finish();
    }

    /// Test helper: returns true if using text mode.
    #[cfg(test)]
    pub fn inner_is_text(&self) -> bool {
        matches!(self.inner, Renderer::Text { .. })
    }
}

impl Clone for MergeProgress {
    fn clone(&self) -> Self {
        Self {
            inner: match &self.inner {
                Renderer::Bar(bar) => Renderer::Bar(bar.clone()),
                Renderer::Text { total, completed } => Renderer::Text {
                    total: *total,
                    completed: std::sync::atomic::AtomicUsize::new(
                        completed.load(std::sync::atomic::Ordering::Relaxed),
                    ),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_creation() {
        let progress = MergeProgress::new(10);
        progress.finish();
    }

    #[test]
    fn test_progress_text_new_and_record() {
        let progress = MergeProgress::new_text(10);
        progress.record("test", true, Duration::from_millis(100), None, None);
        assert!(progress.inner_is_text());
        progress.finish();
    }

    #[test]
    fn test_progress_record_success() {
        let progress = MergeProgress::new_text(10);
        progress.record("test", true, Duration::from_millis(500), None, None);
        progress.finish();
    }

    #[test]
    fn test_progress_record_failure() {
        let progress = MergeProgress::new_text(10);
        progress.record("test", false, Duration::from_secs(2), Some("error"), None);
        progress.finish();
    }

    #[test]
    fn test_progress_clone() {
        let progress = MergeProgress::new_text(10);
        let cloned = progress.clone();
        cloned.record("test", true, Duration::from_millis(100), None, None);
        progress.finish();
        cloned.finish();
    }
}
