# User Experience Enhancements Implementation Plan

> **REQUIRED SUB-SKILL:** Use the executing-plans skill to implement this plan task-by-task.

**Goal:** Add progress bar, dry-run mode, verbose output, and resume capability to improve user experience for batch operations.

**Architecture:** Add new modules for progress tracking and state management. Extend CLI with new flags. Keep backward compatibility.

**Tech Stack:** Rust Edition 2021, indicatif (progress bar), serde (state serialization), existing clap/rayon/anyhow

---

## Overview

The project is mature with comprehensive testing and CI/CD. This plan adds user-facing features for better experience during large batch operations:

| Feature | Impact | Priority |
|---------|--------|----------|
| Progress bar | Visual feedback during long operations | P1 |
| Dry-run mode | Preview without executing | P1 |
| Verbose mode | Detailed per-file output | P2 |
| Resume capability | Continue interrupted batches | P2 |
| Retry logic | Handle transient failures | P3 |

---

## File Structure

| File | Responsibility | Change Type |
|------|----------------|-------------|
| `Cargo.toml` | Dependencies | Modify |
| `src/main.rs` | CLI integration | Modify |
| `src/cli.rs` | New CLI flags | Modify |
| `src/merger.rs` | Progress integration, retry | Modify |
| `src/state.rs` | State file management | Create |
| `src/progress.rs` | Progress bar wrapper | Create |

---

## Chunk 1: P1 Progress Bar

### Task 1: Add indicatif dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Write the failing test (dependency check)**

Run: `cargo check`
Expected: Currently passes, will need indicatif

**Step 2: Add indicatif dependency**

```toml
[dependencies]
clap = { version = "4.5.0", features = ["derive"] }
which = "6.0.0"
rayon = "1.10.0"
colored = "2.1.0"
anyhow = "1.0.82"
indicatif = "0.17.8"
serde = { version = "1.0.200", features = ["derive"] }
```

**Step 3: Run cargo check to verify**

Run: `cargo check`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add indicatif and serde dependencies"
```

---

### Task 2: Create progress module

**Files:**
- Create: `src/progress.rs`

**Step 1: Write the progress module**

```rust
// src/progress.rs
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

pub struct MergeProgress {
    bar: Arc<ProgressBar>,
}

impl MergeProgress {
    pub fn new(total: usize) -> Self {
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}"
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        Self { bar: Arc::new(bar) }
    }

    pub fn inc(&self) {
        self.bar.inc(1);
    }

    pub fn set_message(&self, msg: &str) {
        self.bar.set_message(msg.to_string());
    }

    pub fn finish(&self) {
        self.bar.finish();
    }

    pub fn bar(&self) -> Arc<ProgressBar> {
        self.bar.clone()
    }
}

impl Clone for MergeProgress {
    fn clone(&self) -> Self {
        Self { bar: self.bar.clone() }
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
}
```

**Step 2: Run tests to verify**

Run: `cargo test progress`
Expected: 2 tests pass

**Step 3: Add module to main.rs**

```rust
// src/main.rs - add at top with other modules
mod progress;
mod state;
```

**Step 4: Commit**

```bash
git add src/progress.rs src/main.rs
git commit -m "feat: add progress module with indicatif integration"
```

---

### Task 3: Integrate progress bar into merger

**Files:**
- Modify: `src/merger.rs`

**Step 1: Add progress parameter to merge_pair**

```rust
// src/merger.rs - update imports
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::progress::MergeProgress;
use crate::scanner::{FilePair, ScanResult};
use anyhow::{Context, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
```

**Step 2: Modify merge_pair to use progress**

```rust
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
) -> MergeResult {
    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    if let Some(p) = progress {
        p.set_message(&pair.stem);
    }

    let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);

    match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT) {
        Ok(status) if status.success() => {
            if progress.is_none() {
                println!("{} {}", "✓".green(), pair.stem);
            }
            if let Some(p) = progress {
                p.inc();
            }
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: true,
                error: None,
            }
        }
        Ok(status) => {
            if progress.is_none() {
                println!(
                    "{} {}: ffmpeg exited with code {:?}",
                    "✗".red(),
                    pair.stem,
                    status.code()
                );
            }
            if let Some(p) = progress {
                p.inc();
            }
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(format!("ffmpeg exited with code {:?}", status.code())),
            }
        }
        Err(e) => {
            if progress.is_none() {
                println!("{} {}: {}", "✗".red(), pair.stem, e);
            }
            if let Some(p) = progress {
                p.inc();
            }
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(e.to_string()),
            }
        }
    }
}
```

**Step 3: Update execute_merges signature**

```rust
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
    progress: Option<MergeProgress>,
) -> MergeSummary {
    let output_dir = output_dir.to_path_buf();
    let pairs = &scan_result.pairs;

    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build_global();

    let progress_ref = progress.as_ref();
    
    let results: Vec<MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| merge_pair(pair, idx, &output_dir, format, progress_ref))
        .collect();

    if let Some(p) = &progress {
        p.finish();
    }

    // ... rest unchanged ...
}
```

**Step 4: Run tests to verify existing functionality**

Run: `cargo test merger`
Expected: All tests pass (progress is optional parameter)

**Step 5: Commit**

```bash
git add src/merger.rs
git commit -m "feat(merger): integrate progress bar with optional progress tracking"
```

---

### Task 4: Add progress CLI flag

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

**Step 1: Add progress flag to Args**

```rust
// src/cli.rs - add to Args struct
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    // ... existing fields ...

    /// Show progress bar during batch operations
    #[arg(short = 'p', long, default_value_t = true)]
    pub progress: bool,
}
```

**Step 2: Update main.rs to use progress flag**

```rust
// src/main.rs - in run() function after scan
fn run() -> Result<()> {
    let mut args = Args::parse();
    args.validate()?;

    let format = args.format;

    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }

    let scan_result = scanner::scan_directory(&args.source)?;

    if scan_result.pairs.is_empty() {
        println!("{}", "No file pairs to merge".yellow());
        return Ok(());
    }

    if !args.output.exists() {
        std::fs::create_dir_all(&args.output)
            .map_err(|e| anyhow::anyhow!("Failed to create output directory: {}", e))?;
    }

    println!("Processing {} file pairs...", scan_result.pairs.len());
    
    let progress = if args.progress {
        Some(progress::MergeProgress::new(scan_result.pairs.len()))
    } else {
        None
    };

    let summary = merger::execute_merges(
        scan_result,
        &args.output,
        format,
        args.jobs,
        args.sdel,
        progress,
    );

    summary.print_report();

    if summary.all_success() {
        Ok(())
    } else {
        Err(AppError::MergeFailed.into())
    }
}
```

**Step 3: Run integration tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Manual test with progress**

Run: `cargo run -- --help`
Expected: Shows `--progress` flag in help

**Step 5: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): add --progress flag for progress bar display"
```

---

## Chunk 2: P1 Dry-Run Mode

### Task 5: Add dry-run CLI flag

**Files:**
- Modify: `src/cli.rs`

**Step 1: Add dry-run flag to Args**

```rust
// src/cli.rs - add to Args struct
    /// Preview operations without executing (no files created/deleted)
    #[arg(short = 'n', long)]
    pub dry_run: bool,
```

**Step 2: Write test for dry-run validation**

```rust
// src/cli.rs - add to args_tests module
#[test]
fn test_dry_run_flag_default() {
    let args = Args::try_parse_from(["mixbilibili"]).unwrap();
    assert!(!args.dry_run);
}

#[test]
fn test_dry_run_flag_enabled() {
    let args = Args::try_parse_from(["mixbilibili", "--dry-run"]).unwrap();
    assert!(args.dry_run);
}
```

**Step 3: Run tests to verify**

Run: `cargo test cli`
Expected: 2 new tests pass

**Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): add --dry-run flag for preview mode"
```

---

### Task 6: Implement dry-run in merger

**Files:**
- Modify: `src/merger.rs`

**Step 1: Add dry_run parameter to merge_pair**

```rust
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    dry_run: bool,
) -> MergeResult {
    if dry_run {
        // In dry-run mode, just report success without actual merge
        if let Some(p) = progress {
            p.set_message(&format!("[dry] {}", pair.stem));
            p.inc();
        } else {
            println!("{} {} [dry-run]", "○".cyan(), pair.stem);
        }
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: true,
            error: None,
        };
    }

    // ... existing merge logic ...
}
```

**Step 2: Update execute_merges**

```rust
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
    progress: Option<MergeProgress>,
    dry_run: bool,
) -> MergeSummary {
    // In dry-run mode, don't delete source files
    let effective_delete = delete_source && !dry_run;
    
    // ... pass dry_run to merge_pair ...
    
    let results: Vec<MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| merge_pair(pair, idx, &output_dir, format, progress_ref, dry_run))
        .collect();
    
    // ... rest unchanged, use effective_delete for deletion ...
}
```

**Step 3: Update main.rs**

```rust
// src/main.rs - pass dry_run to execute_merges
let summary = merger::execute_merges(
    scan_result,
    &args.output,
    format,
    args.jobs,
    args.sdel,
    progress,
    args.dry_run,
);

if args.dry_run {
    println!("{}", "Dry-run complete. No files were modified.".cyan());
}
```

**Step 4: Run tests to verify**

Run: `cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/merger.rs src/main.rs
git commit -m "feat(merger): implement dry-run mode without file modifications"
```

---

## Chunk 3: P2 Verbose Mode

### Task 7: Add verbose CLI flag

**Files:**
- Modify: `src/cli.rs`

**Step 1: Add verbose flag**

```rust
// src/cli.rs - add to Args struct
    /// Show detailed output including ffmpeg commands
    #[arg(short = 'v', long)]
    pub verbose: bool,
```

**Step 2: Add tests**

```rust
#[test]
fn test_verbose_flag_default() {
    let args = Args::try_parse_from(["mixbilibili"]).unwrap();
    assert!(!args.verbose);
}

#[test]
fn test_verbose_flag_enabled() {
    let args = Args::try_parse_from(["mixbilibili", "--verbose"]).unwrap();
    assert!(args.verbose);
}
```

**Step 3: Run tests**

Run: `cargo test cli`
Expected: Tests pass

**Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): add --verbose flag for detailed output"
```

---

### Task 8: Implement verbose output

**Files:**
- Modify: `src/ffmpeg.rs`
- Modify: `src/merger.rs`

**Step 1: Add verbose mode to build_merge_command**

```rust
// src/ffmpeg.rs - modify function
pub fn build_merge_command(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    format: OutputFormat,
    verbose: bool,
) -> Command {
    let mut cmd = Command::new("ffmpeg");

    if verbose {
        cmd.arg("-hide_banner");
    } else {
        cmd.arg("-hide_banner").arg("-loglevel").arg("error");
    }
    
    // ... rest unchanged ...
}
```

**Step 2: Update merger.rs**

```rust
// src/merger.rs - add verbose parameter
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    dry_run: bool,
    verbose: bool,
) -> MergeResult {
    // ... in merge section, pass verbose to build_merge_command ...
    let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format, verbose);
    
    if verbose && !dry_run {
        println!("Running: ffmpeg -i {} -i {} -> {}",
            pair.video.display(),
            pair.audio.display(),
            output_path.display());
    }
}
```

**Step 3: Update execute_merges and main.rs**

Pass verbose through the chain like dry_run.

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/ffmpeg.rs src/merger.rs src/main.rs
git commit -m "feat: implement verbose mode with ffmpeg command output"
```

---

## Chunk 4: P2 Resume Capability

### Task 9: Create state module

**Files:**
- Create: `src/state.rs`

**Step 1: Create state module**

```rust
// src/state.rs
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MergeState {
    pub source_dir: String,
    pub output_dir: String,
    pub format: String,
    pub completed: Vec<String>,
    pub failed: Vec<String>,
    pub pending: Vec<String>,
}

impl MergeState {
    pub fn new(source: &Path, output: &Path, format: &str) -> Self {
        Self {
            source_dir: source.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            format: format.to_string(),
            completed: Vec::new(),
            failed: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn state_file_path(source: &Path) -> PathBuf {
        source.join(".mixbilibili_state.json")
    }

    pub fn load(source: &Path) -> Result<Option<Self>> {
        let path = Self::state_file_path(source);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .context("Failed to read state file")?;
        let state: Self = serde_json::from_str(&content)
            .context("Failed to parse state file")?;
        Ok(Some(state))
    }

    pub fn save(&self, source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize state")?;
        fs::write(&path, content)
            .context("Failed to write state file")?;
        Ok(())
    }

    pub fn mark_completed(&mut self, stem: &str) {
        self.pending.retain(|s| s != stem);
        if !self.completed.contains(&stem.to_string()) {
            self.completed.push(stem.to_string());
        }
    }

    pub fn mark_failed(&mut self, stem: &str) {
        self.pending.retain(|s| s != stem);
        if !self.failed.contains(&stem.to_string()) {
            self.failed.push(stem.to_string());
        }
    }

    pub fn is_completed(&self, stem: &str) -> bool {
        self.completed.contains(&stem.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_state_creation() {
        let state = MergeState::new(
            Path::new("/source"),
            Path::new("/output"),
            "mkv"
        );
        assert_eq!(state.format, "mkv");
        assert!(state.completed.is_empty());
    }

    #[test]
    fn test_state_save_load() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path(), "mkv");
        state.pending.push("test".to_string());
        state.mark_completed("test");

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();

        assert_eq!(loaded.completed, vec!["test"]);
    }

    #[test]
    fn test_is_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."), "mkv");
        state.completed.push("video1".to_string());
        assert!(state.is_completed("video1"));
        assert!(!state.is_completed("video2"));
    }
}
```

**Step 2: Add serde_json dependency**

```toml
# Cargo.toml - add to dependencies
serde_json = "1.0.116"
```

**Step 3: Run tests**

Run: `cargo test state`
Expected: 3 tests pass

**Step 4: Commit**

```bash
git add src/state.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: add state module for resume capability"
```

---

### Task 10: Add resume CLI flag

**Files:**
- Modify: `src/cli.rs`

**Step 1: Add resume flag**

```rust
    /// Resume interrupted batch from previous state
    #[arg(short = 'r', long)]
    pub resume: bool,
```

**Step 2: Add tests**

```rust
#[test]
fn test_resume_flag_default() {
    let args = Args::try_parse_from(["mixbilibili"]).unwrap();
    assert!(!args.resume);
}

#[test]
fn test_resume_flag_enabled() {
    let args = Args::try_parse_from(["mixbilibili", "--resume"]).unwrap();
    assert!(args.resume);
}
```

**Step 3: Run tests**

Run: `cargo test cli`
Expected: Tests pass

**Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): add --resume flag for continuing interrupted batches"
```

---

### Task 11: Implement resume logic

**Files:**
- Modify: `src/main.rs`
- Modify: `src/merger.rs`

**Step 1: Add resume logic to main.rs**

```rust
// src/main.rs - in run() function
fn run() -> Result<()> {
    let mut args = Args::parse();
    args.validate()?;

    let format = args.format;

    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }

    // Check for resume state
    let existing_state = if args.resume {
        state::MergeState::load(&args.source)?
    } else {
        None
    };

    let scan_result = scanner::scan_directory(&args.source)?;

    // Filter out already completed if resuming
    let pairs_to_process = if let Some(ref state) = existing_state {
        scan_result.pairs.iter()
            .filter(|p| !state.is_completed(&p.stem))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        scan_result.pairs.clone()
    };

    if pairs_to_process.is_empty() {
        if existing_state.is_some() {
            println!("{}", "All files already merged from previous session".green());
        } else {
            println!("{}", "No file pairs to merge".yellow());
        }
        return Ok(());
    }

    // Initialize state
    let mut merge_state = existing_state.unwrap_or_else(|| {
        state::MergeState::new(&args.source, &args.output, &format.to_string())
    });
    
    for pair in &pairs_to_process {
        merge_state.pending.push(pair.stem.clone());
    }

    // ... proceed with merge, update state after each ...
}
```

**Step 2: Update merger to track state**

Pass state reference to execute_merges and update it after each merge.

**Step 3: Save state periodically**

```rust
// Save state after each batch of merges
merge_state.save(&args.source)?;
```

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/main.rs src/merger.rs
git commit -m "feat: implement resume capability with state tracking"
```

---

## Chunk 5: P3 Retry Logic

### Task 12: Add retry CLI flag

**Files:**
- Modify: `src/cli.rs`

**Step 1: Add retry flag**

```rust
    /// Number of retries for failed merges (0 = no retry)
    #[arg(long, default_value_t = 0)]
    pub retry: usize,
```

**Step 2: Add tests**

```rust
#[test]
fn test_retry_default() {
    let args = Args::try_parse_from(["mixbilibili"]).unwrap();
    assert_eq!(args.retry, 0);
}

#[test]
fn test_retry_custom() {
    let args = Args::try_parse_from(["mixbilibili", "--retry", "3"]).unwrap();
    assert_eq!(args.retry, 3);
}
```

**Step 3: Run tests**

Run: `cargo test cli`
Expected: Tests pass

**Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): add --retry flag for failed merge retries"
```

---

### Task 13: Implement retry logic

**Files:**
- Modify: `src/merger.rs`

**Step 1: Add retry to merge_pair**

```rust
pub fn merge_pair_with_retry(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    dry_run: bool,
    verbose: bool,
    max_retries: usize,
) -> MergeResult {
    if dry_run || max_retries == 0 {
        return merge_pair(pair, pair_index, output_dir, format, progress, dry_run, verbose);
    }

    for attempt in 0..=max_retries {
        if attempt > 0 {
            if let Some(p) = progress {
                p.set_message(&format!("retry {} {}", attempt, pair.stem));
            } else if verbose {
                println!("Retrying {} (attempt {})", pair.stem, attempt);
            }
            // Brief pause before retry
            std::thread::sleep(Duration::from_secs(1));
        }

        let result = merge_pair(pair, pair_index, output_dir, format, progress, dry_run, verbose);
        if result.success {
            return result;
        }
    }

    // All retries failed
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(format!("Failed after {} retries", max_retries)),
    }
}
```

**Step 2: Update execute_merges**

```rust
// In execute_merges, use merge_pair_with_retry when retry > 0
let results: Vec<MergeResult> = pairs
    .par_iter()
    .enumerate()
    .map(|(idx, pair)| {
        merge_pair_with_retry(
            pair, idx, &output_dir, format, 
            progress_ref, dry_run, verbose, max_retries
        )
    })
    .collect();
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/merger.rs src/main.rs
git commit -m "feat(merger): implement retry logic for failed merges"
```

---

## Final Verification

### Task 14: Full test suite and documentation

**Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Check format**

Run: `cargo fmt --check`
Expected: Correct formatting

**Step 4: Build release**

Run: `cargo build --release`
Expected: Successful build

**Step 5: Update CHANGELOG**

```markdown
## [0.3.0] - 2026-04-27

### Added
- Progress bar for batch operations (--progress)
- Dry-run mode for preview (--dry-run)
- Verbose output mode (--verbose)
- Resume capability for interrupted batches (--resume)
- Retry logic for transient failures (--retry N)
```

**Step 6: Final commit**

```bash
git add CHANGELOG.md
git commit -m "docs: update CHANGELOG for v0.3.0"
```

---

## Success Criteria

- [ ] Progress bar displays during batch operations
- [ ] Dry-run mode previews without modifying files
- [ ] Verbose mode shows ffmpeg commands
- [ ] Resume continues from previous state
- [ ] Retry handles transient failures
- [ ] All tests pass
- [ ] No clippy warnings
- [ ] Backward compatible (default behavior unchanged)