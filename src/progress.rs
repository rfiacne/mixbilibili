use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

/// Internal rendering strategy.
enum Renderer {
    Bar(Arc<ProgressBar>),
    Text {
        total: usize,
        completed: Arc<std::sync::atomic::AtomicUsize>,
    },
}

impl Renderer {
    /// Create a text-mode renderer.
    fn new_text(total: usize) -> Self {
        Self::Text {
            total,
            completed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

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
    use crate::ui::{Colors, Symbols};

    let symbols = Symbols::new(crate::ui::supports_unicode());
    let time = format_duration(duration);

    match (success, retry, error) {
        (true, _, _) => {
            let sym = if colored {
                Colors::success(symbols.success).to_string()
            } else {
                symbols.success.to_string()
            };
            format!("{} {} ({})", sym, stem, time)
        }
        (false, Some(r), _) => {
            let sym = if colored {
                Colors::warning(symbols.retry).to_string()
            } else {
                symbols.retry.to_string()
            };
            format!("{} {} retry {} ({})", sym, stem, r, time)
        }
        (false, None, Some(e)) => {
            let sym = if colored {
                Colors::failure(symbols.failure).to_string()
            } else {
                symbols.failure.to_string()
            };
            format!("{} {}: {}", sym, stem, e)
        }
        (false, None, None) => {
            let sym = if colored {
                Colors::failure(symbols.failure).to_string()
            } else {
                symbols.failure.to_string()
            };
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

/// Batch progress-bar template: spinner, status, cyan/blue bar, position, ETA.
const PROGRESS_TEMPLATE: &str = "{spinner} {msg:20} {bar:20.cyan/blue} {pos}/{len} ETA: {eta}";
/// Three-level block gradient for the bar fill.
const PROGRESS_CHARS: &str = "███░░░";

/// Progress bar wrapper for batch merge operations.
pub struct MergeProgress {
    inner: Renderer,
}

impl MergeProgress {
    /// Create a new progress renderer. Auto-detects TTY.
    pub fn new(total: usize) -> Self {
        if std::io::stderr().is_terminal() {
            let bar = ProgressBar::new(total as u64);
            bar.enable_steady_tick(Duration::from_millis(100));

            bar.set_style(
                ProgressStyle::with_template(PROGRESS_TEMPLATE)
                    .unwrap()
                    .progress_chars(PROGRESS_CHARS)
                    .tick_strings(&["●", "○", "●"])
                    .with_key(
                        "eta",
                        |state: &indicatif::ProgressState, w: &mut dyn std::fmt::Write| {
                            let pos = state.pos() as usize;
                            let total = state.len().unwrap_or(0) as usize;
                            let elapsed = state.elapsed().as_secs_f64();
                            if pos > 0 && elapsed > 0.0 && total > pos {
                                let rate = pos as f64 / elapsed;
                                let remaining = (total - pos) as f64 / rate;
                                let _ = write!(
                                    w,
                                    "{}",
                                    format_duration(Duration::from_secs_f64(remaining))
                                );
                            } else {
                                let _ = write!(w, "--");
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

    /// Create a text-mode renderer explicitly, bypassing TTY detection.
    #[cfg(test)]
    pub fn new_text(total: usize) -> Self {
        Self {
            inner: Renderer::new_text(total),
        }
    }

    /// Record a completed file with timing and status.
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
                    completed: Arc::clone(completed),
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

    #[test]
    fn test_progress_bar_template_parses() {
        // indicatif validates the template at construction and `new()` unwraps the
        // result, so a typo would panic in production rather than fail a test.
        let parsed = ProgressStyle::with_template(PROGRESS_TEMPLATE);
        assert!(
            parsed.is_ok(),
            "batch progress-bar template must be valid: {:?}",
            parsed.err()
        );
    }

    #[test]
    fn test_format_record_marks_success() {
        // The symbol set follows locale detection (ASCII fallback on Windows
        // without a UTF-8 LANG), so accept either spelling.
        let result = format_record("test", true, Duration::from_millis(500), None, None, true);
        assert!(result.contains("test"));
        assert!(
            result.contains("✓") || result.contains("[OK]"),
            "record must carry a success marker: {result}"
        );
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    // --- format_duration boundary tests ---

    #[test]
    fn test_format_duration_zero() {
        let s = format_duration(Duration::ZERO);
        assert_eq!(s, "0ms");
    }

    #[test]
    fn test_format_duration_sub_second_boundary() {
        // Just under 1 second → should use ms
        let s = format_duration(Duration::from_millis(999));
        assert!(s.ends_with("ms"), "999ms should format as ms: {}", s);
        assert!(s.contains("999"));
    }

    #[test]
    fn test_format_duration_exactly_one_second() {
        let s = format_duration(Duration::from_secs(1));
        assert!(s.ends_with("s"), "1s should format as seconds: {}", s);
        assert!(!s.contains("ms"));
        assert!(!s.contains("m "));
    }

    #[test]
    fn test_format_duration_under_minute() {
        let s = format_duration(Duration::from_secs(59));
        assert!(s.contains("s"));
        assert!(!s.contains("m "));
    }

    #[test]
    fn test_format_duration_exactly_one_minute() {
        let s = format_duration(Duration::from_secs(60));
        assert!(s.contains("m"), "60s should format with minutes: {}", s);
        assert!(s.contains("1m"));
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        let s = format_duration(Duration::from_secs(90));
        assert!(s.contains("1m"));
        assert!(s.contains("30s"));
    }

    #[test]
    fn test_format_duration_large_value() {
        let s = format_duration(Duration::from_secs(3661));
        assert!(s.contains("m"), "large durations should use minutes: {}", s);
    }

    // --- format_record branch coverage ---

    #[test]
    fn test_format_record_success_uncolored() {
        let s = format_record(
            "my_video",
            true,
            Duration::from_millis(200),
            None,
            None,
            false,
        );
        assert!(s.contains("my_video"));
        assert!(s.contains("200ms"));
    }

    #[test]
    fn test_format_record_failure_with_retry() {
        let s = format_record("vid", false, Duration::from_secs(1), None, Some(2), false);
        assert!(s.contains("vid"));
        assert!(s.contains("retry"));
        assert!(s.contains("2"));
    }

    #[test]
    fn test_format_record_failure_with_error_message() {
        let s = format_record(
            "broken",
            false,
            Duration::from_secs(3),
            Some("ffmpeg crashed"),
            None,
            false,
        );
        assert!(s.contains("broken"));
        assert!(s.contains("ffmpeg crashed"));
    }

    #[test]
    fn test_format_record_failure_no_error_no_retry() {
        let s = format_record("fail", false, Duration::from_millis(50), None, None, false);
        assert!(s.contains("fail"));
        assert!(s.contains("50ms"));
    }

    #[test]
    fn test_format_record_colored_flag_changes_code_path() {
        // When colored=true, format_record uses Colors::success/failure/warning
        // When colored=false, it uses plain symbol.to_string()
        // Both produce valid output containing the stem and duration
        let colored = format_record("x", true, Duration::from_millis(10), None, None, true);
        let plain = format_record("x", true, Duration::from_millis(10), None, None, false);
        // Both should contain the stem and time regardless of coloring
        assert!(colored.contains("x"));
        assert!(colored.contains("10ms"));
        assert!(plain.contains("x"));
        assert!(plain.contains("10ms"));
    }

    // --- MergeProgress text mode tests ---

    #[test]
    fn test_progress_set_message_text_mode_no_panic() {
        let progress = MergeProgress::new_text(5);
        progress.set_message("processing...");
        progress.finish();
    }

    #[test]
    fn test_progress_record_with_retry() {
        let progress = MergeProgress::new_text(5);
        progress.record("vid", false, Duration::from_secs(1), None, Some(1));
        progress.finish();
    }

    #[test]
    fn test_progress_text_mode_finish_idempotent() {
        let progress = MergeProgress::new_text(3);
        progress.finish();
        progress.finish(); // should not panic on double-finish
    }
}
