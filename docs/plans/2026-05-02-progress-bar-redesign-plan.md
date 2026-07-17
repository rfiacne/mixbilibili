# Progress Bar Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace thin `MergeProgress` wrapper with a facade that auto-selects progress bar or text mode, adds per-file status, speed/ETA, and a `record()` API.

**Architecture:** `MergeProgress` holds an internal `Renderer` enum (`Bar` or `Text`). Construction auto-detects TTY via `console::user_attended()`. Both variants share the same public API. `merger.rs` callers switch from `inc()` + `println!` to `record()`.

**Tech Stack:** Rust 2021, indicatif 0.17.8, console (transitive dep), colored 2.1.0

---

### Task 1: Rewrite `src/progress.rs` — `Renderer` enum facade with TTY detection

**Files:**
- Modify: `src/progress.rs`
- Test: `src/progress.rs` (inline tests)

- [ ] **Step 1: Write tests for the new `Renderer` enum and TTY detection**

Add to the bottom of `src/progress.rs` (existing tests module will be updated later):

```rust
#[cfg(test)]
mod progress_v2_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_text_renderer_new_and_finish() {
        // Text renderer should not panic on new/finish
        let r = Renderer::new_text(10);
        r.finish();
    }

    #[test]
    fn test_text_renderer_inc() {
        let r = Renderer::new_text(10);
        r.inc();
        r.inc();
        // inc should increment completed counter
    }

    #[test]
    fn test_text_renderer_record_success() {
        let r = Renderer::new_text(10);
        r.record("test_file", true, Duration::from_millis(1200), None, None);
        // Should print: ✓ test_file (1.20s)
    }

    #[test]
    fn test_text_renderer_record_failure() {
        let r = Renderer::new_text(10);
        r.record("test_file", false, Duration::from_secs(2), Some("error msg"), None);
        // Should print: ✗ test_file: error msg
    }

    #[test]
    fn test_text_renderer_record_retry() {
        let r = Renderer::new_text(10);
        r.record("test_file", false, Duration::from_secs(3), Some("error"), Some(2));
        // Should print: ↻ test_file retry 2 (3.00s)
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test progress_v2_tests -- --test-threads=1`
Expected: FAIL — `Renderer` type and `new_text`, `record` methods don't exist

- [ ] **Step 3: Rewrite `src/progress.rs`**

Replace the entire file content:

```rust
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::Duration;

/// Internal rendering strategy.
enum Renderer {
    /// Full progress bar with speed/ETA (TTY mode).
    Bar(Arc<ProgressBar>),
    /// One-line-per-file text output (no TTY / CI mode).
    Text { total: usize, completed: std::sync::atomic::AtomicUsize },
}

impl Renderer {
    /// Create a text-mode renderer.
    fn new_text(total: usize) -> Self {
        Self::Text {
            total,
            completed: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn inc(&self) {
        match self {
            Self::Bar(bar) => bar.inc(1),
            Self::Text { completed, .. } => {
                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn record(&self, stem: &str, success: bool, duration: Duration, error: Option<&str>, retry: Option<usize>) {
        match self {
            Self::Bar(bar) => {
                let msg = format_record_message(stem, success, duration, error, retry);
                bar.set_message(msg);
                bar.inc(1);
            }
            Self::Text { .. } => {
                let line = format_record_line(stem, success, duration, error, retry);
                println!("{}", line);
            }
        }
    }

    fn update_message(&self, msg: &str) {
        match self {
            Self::Bar(bar) => bar.set_message(msg),
            Self::Text { .. } => {} // Text mode doesn't update mid-file
        }
    }

    fn finish(&self) {
        match self {
            Self::Bar(bar) => {
                bar.finish();
            }
            Self::Text { total, completed } => {
                let done = completed.load(std::sync::atomic::Ordering::Relaxed);
                if done < total {
                    let remaining = total - done;
                    for _ in 0..remaining {
                        completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

/// Format the per-file message for the progress bar.
fn format_record_message(stem: &str, success: bool, duration: Duration, error: Option<&str>, retry: Option<usize>) -> String {
    let symbol = if success { "✓" } else { "✗" };
    let time = format_duration_short(duration);
    match (success, retry, error) {
        (true, _, _) => format!("{} {} ({})", symbol, stem, time),
        (false, Some(r), _) => format!("↻ {} retry {} ({})", stem, r, time),
        (false, None, Some(e)) => format!("✗ {}: {}", stem, e),
        (false, None, None) => format!("✗ {} ({})", stem, time),
    }
}

/// Format a single line for text mode output.
fn format_record_line(stem: &str, success: bool, duration: Duration, error: Option<&str>, retry: Option<usize>) -> String {
    let time = format_duration_short(duration);
    match (success, retry, error) {
        (true, _, _) => format!("{} {} ({})", "✓".green(), stem, time),
        (false, Some(r), _) => format!("↻ {} retry {} ({})", "↻".yellow(), stem, r, time),
        (false, None, Some(e)) => format!("{} {}: {}", "✗".red(), stem, e),
        (false, None, None) => format!("{} {} ({})", "✗".red(), stem, time),
    }
}

/// Compact duration format: ms, s, or m.
fn format_duration_short(d: Duration) -> String {
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
        if console::user_attended() {
            let bar = ProgressBar::new(total as u64);
            bar.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] {bar:30.cyan/blue} {pos}/{len} ({per_sec}) {msg}",
                )
                .unwrap()
                .progress_chars("=>-")
                .with_key("per_sec", |state| {
                    let elapsed = state.elapsed().as_secs_f64();
                    if elapsed > 0.0 {
                        format!("{:.1} files/s", state.pos() as f64 / elapsed)
                    } else {
                        "0.0 files/s".to_string()
                    }
                }),
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

    /// Increment progress by 1.
    pub fn inc(&self) {
        self.inner.inc();
    }

    /// Record a completed file with timing and status.
    pub fn record(&self, stem: &str, success: bool, duration: Duration, error: Option<&str>, retry: Option<usize>) {
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
```

- [ ] **Step 4: Keep existing tests, update for new API**

Replace the existing `tests` module at the bottom of `src/progress.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_creation() {
        let progress = MergeProgress::new(10);
        // Bar mode: check length; Text mode: just verify no panic
        progress.finish();
    }

    #[test]
    fn test_progress_text_new_and_inc() {
        let progress = MergeProgress::new_text(10);
        progress.inc();
        assert!(progress.inner_matches_text());
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
        cloned.inc();
        progress.finish();
        cloned.finish();
    }
}
```

Wait — `inner_matches_text()` doesn't exist. Add a test helper:

```rust
impl MergeProgress {
    /// Test helper: returns true if using text mode.
    #[cfg(test)]
    pub fn inner_matches_text(&self) -> bool {
        matches!(self.inner, Renderer::Text { .. })
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test progress -- --test-threads=1`
Expected: All progress tests PASS

- [ ] **Step 6: Run full test suite to verify no breakage**

Run: `cargo test`
Expected: ALL PASS (111 tests — existing merger/main tests still work since `MergeProgress` public API is backward-compatible)

- [ ] **Step 7: Commit**

```bash
git add src/progress.rs
git commit -m "feat(progress): rewrite MergeProgress with Renderer enum facade and TTY detection"
```

---

### Task 2: Update `src/merger.rs` — use `record()` instead of `inc()` + `println!`

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Update `do_dry_run` to use `record()`**

Replace:
```rust
if progress.is_none() {
    println!(
        "{} {} {}",
        t("circle").cyan(),
        pair.stem,
        t("dry_run_marker")
    );
}
if let Some(p) = progress {
    p.inc();
}
```

With:
```rust
if let Some(p) = progress {
    p.record(&pair.stem, true, start.elapsed(), None, None);
} else {
    println!(
        "{} {} {}",
        t("circle").cyan(),
        pair.stem,
        t("dry_run_marker")
    );
}
```

- [ ] **Step 2: Update `do_merge` success path to use `record()`**

Replace:
```rust
Ok(status) if status.success() => {
    if let Some(p) = progress {
        p.inc();
    }
    if progress.is_none() {
        println!("{} {}", t("checkmark").green(), pair.stem);
    }
    return MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: true,
        error: None,
        duration: start.elapsed(),
    };
}
```

With:
```rust
Ok(status) if status.success() => {
    let duration = start.elapsed();
    if let Some(p) = progress {
        p.record(&pair.stem, true, duration, None, None);
    } else {
        println!("{} {}", t("checkmark").green(), pair.stem);
    }
    return MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: true,
        error: None,
        duration,
    };
}
```

- [ ] **Step 3: Update `do_merge` failure paths to use `record()`**

Replace the two failure branches (exit code failure and error failure):

```rust
Ok(status) if attempt == max_retries => {
    let duration = start.elapsed();
    let error_msg = format!("ffmpeg exited with code {:?} after {} retries", status.code(), max_retries);
    if let Some(p) = progress {
        p.record(&pair.stem, false, duration, Some(&error_msg), None);
    } else {
        println!(
            "{} {}: {}",
            t("cross").red(),
            pair.stem,
            error_msg
        );
    }
    return MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(error_msg),
        duration,
    };
}
Err(e) if attempt == max_retries => {
    let duration = start.elapsed();
    let error_msg = format!("{e} after {max_retries} retries");
    if let Some(p) = progress {
        p.record(&pair.stem, false, duration, Some(&error_msg), None);
    } else {
        println!("{} {}: {}", t("cross").red(), pair.stem, e);
    }
    return MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(error_msg),
        duration,
    };
}
```

- [ ] **Step 4: Update `do_merge` retry path to use `set_message` → `record`**

Replace:
```rust
if attempt > 0 {
    std::thread::sleep(Duration::from_secs(1));
    if let Some(p) = progress {
        p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
    } else if verbose {
        println!(
            "{} {}",
            "↻".yellow(),
            tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
        );
    }
}
```

With:
```rust
if attempt > 0 {
    std::thread::sleep(Duration::from_secs(1));
    if let Some(p) = progress {
        p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
    } else if verbose {
        println!(
            "{} {}",
            "↻".yellow(),
            tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
        );
    }
}
```

This keeps the old `set_message()` pattern for retries — it updates the bar message without advancing the counter, since the file hasn't finished processing yet.

- [ ] **Step 5: Remove `set_message` if no longer needed by merger.rs**

- [ ] **Step 5: Check `tf` import**

`tf` is still used for `verbose_retry` in the `else if verbose` branch of the retry path. Keep the import as-is.

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: ALL PASS (111 tests)

- [ ] **Step 7: Run clippy and fmt**

Run: `cargo clippy -- -D warnings && cargo fmt --check`
Expected: clean

- [ ] **Step 8: Commit**

```bash
git add src/merger.rs
git commit -m "feat(progress): use record() API instead of inc() + println! in merger"
```

---

### Task 3: Visual verification and README update

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Build and test progress bar visually**

Run with a directory containing a few test files:
```bash
cargo build --release
```

Create test directory:
```bash
mkdir -p /tmp/test-progress && cd /tmp/test-progress
touch video1.mp4 video1.m4a video2.mp4 video2.m4a video3.mp4 video3.m4a
```

Run in TTY mode (should show progress bar):
```bash
mixbilibili -s /tmp/test-progress -o /tmp/test-out --dry-run
```

Run with output piped (should show text mode):
```bash
mixbilibili -s /tmp/test-progress -o /tmp/test-out --dry-run 2>&1 | cat
```

- [ ] **Step 2: Update README Features section**

Add to the Features list in `README.md`:
```markdown
- **Smart progress display**: Full progress bar with speed/ETA on TTY, clean text output in CI/pipes
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add smart progress display to README features"
```

---

### Task 4: Final verification

**Files:**
- All modified files

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run formatter check**

Run: `cargo fmt --check`
Expected: No changes needed

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: address clippy/fmt issues from progress bar changes"
```
