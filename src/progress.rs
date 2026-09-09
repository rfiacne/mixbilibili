use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::time::Duration;

/// Internal rendering strategy.
enum Renderer {
    /// ProgressBar is internally Arc'd: Clone shares state, Send + Sync.
    Bar(ProgressBar),
    Text,
}

impl Renderer {
    fn record(
        &self,
        stem: &str,
        success: bool,
        duration: Duration,
        error: Option<&str>,
        retry: Option<usize>,
    ) {
        let line = format_record(stem, success, duration, error, retry);
        match self {
            Self::Bar(bar) => {
                // indicatif strips ANSI when measuring width, so the bar
                // stays aligned with colored messages.
                bar.set_message(line);
                bar.inc(1);
            }
            Self::Text => println!("{}", line),
        }
    }

    fn finish(&self) {
        if let Self::Bar(bar) = self {
            bar.finish();
        }
    }
}

/// Format a record line.
fn format_record(
    stem: &str,
    success: bool,
    duration: Duration,
    error: Option<&str>,
    retry: Option<usize>,
) -> String {
    let time = format_duration(duration);
    match (success, retry, error) {
        (true, _, _) => format!("{} {} ({})", "✓".green(), stem, time),
        (false, Some(r), _) => format!("{} {} retry {} ({})", "↻".yellow(), stem, r, time),
        (false, None, Some(e)) => format!("{} {}: {}", "✗".red(), stem, e),
        (false, None, None) => format!("{} {} ({})", "✗".red(), stem, time),
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
            bar.enable_steady_tick(Duration::from_millis(100));
            // {per_sec} and {eta} are indicatif's built-in keys — no custom
            // formatters needed.
            bar.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] {bar:30.cyan/blue} {pos}/{len} ({per_sec}) ETA: {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            Self {
                inner: Renderer::Bar(bar),
            }
        } else {
            Self {
                inner: Renderer::Text,
            }
        }
    }

    /// Create a text-mode renderer explicitly (for testing).
    #[cfg(test)]
    pub fn new_text() -> Self {
        Self {
            inner: Renderer::Text,
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

    /// Finish the progress renderer.
    pub fn finish(&self) {
        self.inner.finish();
    }

    /// Test helper: returns true if using text mode.
    #[cfg(test)]
    pub fn inner_is_text(&self) -> bool {
        matches!(self.inner, Renderer::Text)
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
        let progress = MergeProgress::new_text();
        progress.record("test", true, Duration::from_millis(100), None, None);
        assert!(progress.inner_is_text());
        progress.finish();
    }

    #[test]
    fn test_progress_record_success() {
        let progress = MergeProgress::new_text();
        progress.record("test", true, Duration::from_millis(500), None, None);
        progress.finish();
    }

    #[test]
    fn test_progress_record_failure() {
        let progress = MergeProgress::new_text();
        progress.record("test", false, Duration::from_secs(2), Some("error"), None);
        progress.finish();
    }
}
