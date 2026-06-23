# User Experience Enhancements Plan

**Date**: 2026-06-23  
**Version Target**: v0.7.0  
**Status**: Draft

## Overview

Four user-facing improvements to make mixbilibili more intuitive and safer:

1. **Interactive preview mode** — Show what will be merged before execution
2. **Dry-run summary with estimated time** — Add time estimates to dry-run output
3. **Progress estimation before merge starts** — Calculate and display total expected duration
4. **Better interrupt handling** — Graceful shutdown with cleanup

---

## 1. Interactive Preview Mode

### Current State
- `--dry-run` flag exists, shows file pairs
- No interactive confirmation before execution
- Users can accidentally merge wrong files

### Goal
Add `--interactive` / `-i` flag that shows preview and prompts for confirmation.

### Implementation

**File: `src/cli.rs`**
```rust
// Add new argument to build_cli()
.arg(
    Arg::new("interactive")
        .short('i')
        .long("interactive")
        .help(t("cli_interactive"))
        .action(ArgAction::SetTrue),
)
```

**File: `src/main.rs` — `run()` function**
```rust
// After scan_and_filter(), before execute()
if args.interactive && !args.dry_run {
    if !show_preview_and_confirm(&ctx, &args, &format)? {
        println!("{}", t("operation_cancelled").yellow());
        return Ok(());
    }
}
```

**New function: `show_preview_and_confirm()`**
```rust
fn show_preview_and_confirm(
    ctx: &ScanContext,
    args: &Args,
    format: &cli::OutputFormat,
) -> Result<bool> {
    // Display preview table
    println!("{}", t("preview_header").cyan().bold());
    println!();
    
    for (i, pair) in ctx.pairs.iter().enumerate() {
        println!(
            "  {}. {} + {} -> {}.{}",
            i + 1,
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    
    println!();
    println!(
        "{}",
        tf("preview_summary", &[
            &ctx.pairs.len().to_string(),
            format.extension()
        ])
    );
    
    if args.sdel {
        println!("{}", t("preview_sdel_warning").yellow());
    }
    
    // Prompt for confirmation
    println!();
    print!("{} ", t("confirm_proceed").bold());
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    
    let response = input.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}
```

**i18n keys to add:**
- `cli_interactive` — "Prompt for confirmation before merging"
- `preview_header` — "Preview of merge operations:"
- `preview_summary` — "Total: {0} pairs will be merged to .{1} format"
- `preview_sdel_warning` — "⚠ Source files will be deleted after merge"
- `confirm_proceed` — "Proceed? [y/N]"
- `operation_cancelled` — "Operation cancelled by user"

**Testing:**
- Test interactive flag parsing
- Test confirmation flow (yes/no/empty)
- Test interactive + dry-run combination (dry-run takes precedence)

---

## 2. Dry-Run Summary with Estimated Time

### Current State
- Dry-run shows file pairs
- No time estimation
- Users can't plan their workflow

### Goal
Calculate and display estimated merge time based on file sizes.

### Implementation

**File: `src/ffmpeg.rs`** — New function
```rust
/// Estimate merge time based on video file size and typical throughput
pub fn estimate_merge_duration(video_path: &Path) -> std::time::Duration {
    // Get file size
    let metadata = match std::fs::metadata(video_path) {
        Ok(m) => m,
        Err(_) => return std::time::Duration::from_secs(0),
    };
    
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
    
    // Estimate: ~10 MB/s throughput (conservative for typical hardware)
    // Adjust based on actual benchmark data
    let estimated_seconds = size_mb / 10.0;
    
    std::time::Duration::from_secs(estimated_seconds as u64)
}
```

**File: `src/scanner.rs`** — Extend `ScanContext`
```rust
pub struct ScanContext {
    pub pairs: Vec<MergePair>,
    pub stats: ScanStats,
    pub estimated_duration: std::time::Duration,  // NEW
}
```

**File: `src/scanner.rs`** — Calculate in `scan_directory()`
```rust
let estimated_duration: std::time::Duration = pairs
    .iter()
    .map(|p| ffmpeg::estimate_merge_duration(&p.video))
    .sum();

Ok(Some(ScanContext {
    pairs,
    stats,
    estimated_duration,
}))
```

**File: `src/main.rs`** — Update dry-run output
```rust
if args.dry_run {
    println!("{}", t("dry_run_header").cyan().bold());
    for pair in &ctx.pairs {
        println!(
            "  {} + {} -> {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    
    // NEW: Show time estimate
    println!();
    println!(
        "{}",
        tf("dry_run_time_estimate", &[
            &format_duration(ctx.estimated_duration.as_secs())
        ])
    );
    
    if args.sdel {
        println!("\n{}", t("dry_run_sdel_header").yellow().bold());
        for pair in &ctx.pairs {
            println!(
                "  {} (video)\n  {} (audio)",
                pair.video.display(),
                pair.audio.display()
            );
        }
    }
    
    println!(
        "\n{}",
        tf("dry_run_summary", &[&ctx.pairs.len().to_string()])
    );
    println!("{}", t("dry_run_complete").cyan());
    return Ok(());
}
```

**i18n keys to add:**
- `dry_run_time_estimate` — "Estimated time: {0}"

**Testing:**
- Test duration estimation with various file sizes
- Test dry-run output includes time estimate
- Test estimation handles missing files gracefully

---

## 3. Progress Estimation Before Merge Starts

### Current State
- Progress bar shows completed/total
- No upfront time estimate
- Users don't know how long to wait

### Goal
Show estimated total time before starting merges.

### Implementation

**File: `src/main.rs`** — Update `execute()` function
```rust
fn execute(
    args: &Args,
    ctx: ScanContext,
    merge_state: &mut state::MergeState,
    format: cli::OutputFormat,
) -> Result<merger::MergeSummary> {
    // NEW: Show estimated time before starting
    if args.progress && !args.quiet {
        println!(
            "{}",
            tf("starting_merges", &[
                &ctx.pairs.len().to_string(),
                &format_duration(ctx.estimated_duration.as_secs())
            ])
        );
    }
    
    // ... rest of execute()
}
```

**File: `src/progress.rs`** — Enhance progress bar
```rust
pub struct MergeProgress {
    inner: Renderer,
    start_time: std::time::Instant,  // NEW
    estimated_total: std::time::Duration,  // NEW
}

impl MergeProgress {
    pub fn new(total: usize, estimated: std::time::Duration) -> Self {
        Self {
            inner: Renderer::new(total),
            start_time: std::time::Instant::now(),
            estimated_total: estimated,
        }
    }
    
    pub fn update(&self, completed: usize) {
        let elapsed = self.start_time.elapsed();
        let eta = if completed > 0 {
            let rate = elapsed.as_secs_f64() / completed as f64;
            let remaining = (self.inner.total() - completed) as f64 * rate;
            std::time::Duration::from_secs(remaining as u64)
        } else {
            self.estimated_total
        };
        
        self.inner.set_message(format!(
            "ETA: {}",
            format_duration(eta.as_secs())
        ));
        self.inner.inc(1);
    }
}
```

**i18n keys to add:**
- `starting_merges` — "Starting {0} merges (estimated time: {1})"

**Testing:**
- Test progress bar shows ETA
- Test ETA calculation accuracy
- Test progress bar with estimated duration

---

## 4. Better Interrupt Handling

### Current State
- Ctrl+C sets `INTERRUPTED` flag
- Merges continue until completion
- No cleanup of partial files

### Goal
Graceful shutdown with cleanup of partial output files.

### Implementation

**File: `src/merger.rs`** — Check interrupt in merge loop
```rust
pub fn merge_pair(
    pair: &MergePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    retry_count: usize,
) -> MergeResult {
    // ... existing code ...
    
    for attempt in 0..=retry_count {
        // NEW: Check interrupt before each attempt
        if crate::main::INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst) {
            return MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                duration: start.elapsed(),
                error: Some("interrupted".to_string()),
            };
        }
        
        // ... rest of merge logic ...
    }
}
```

**File: `src/main.rs`** — Cleanup on interrupt
```rust
fn execute(
    args: &Args,
    ctx: ScanContext,
    merge_state: &mut state::MergeState,
    format: cli::OutputFormat,
) -> Result<merger::MergeSummary> {
    // ... existing code ...
    
    let results: Vec<merger::MergeResult> = ctx
        .pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            let result = merger::merge_pair(/* ... */);
            
            // NEW: Cleanup partial files on interrupt
            if !result.success && result.error.as_ref().is_some_and(|e| e.contains("interrupted")) {
                let output_path = args.output.join(format!("{}.{}", pair.stem, format.extension()));
                if output_path.exists() {
                    let _ = std::fs::remove_file(&output_path);
                    if args.verbose {
                        eprintln!("Cleaned up partial file: {}", output_path.display());
                    }
                }
            }
            
            // ... rest of result handling ...
        })
        .collect();
    
    // ... rest of execute() ...
}
```

**File: `src/main.rs`** — Enhanced interrupt message
```rust
if INTERRUPTED.load(Ordering::SeqCst) {
    println!("{}", t("interrupted_cleanup").yellow());
    println!("{}", t("interrupted_resume_hint").cyan());
    Ok(())
} else if summary.all_success() {
    Ok(())
} else {
    Err(AppError::MergeFailed {
        count: summary.failed_count,
    }
    .into())
}
```

**i18n keys to add:**
- `interrupted_cleanup` — "Interrupted. Partial files cleaned up."
- `interrupted_resume_hint` — "Use --resume to continue from where you left off."

**Testing:**
- Test interrupt during merge
- Test partial file cleanup
- Test resume after interrupt
- Test interrupt message display

---

## Implementation Order

1. **Phase 1: Time Estimation** (Foundation)
   - Add `estimate_merge_duration()` to `ffmpeg.rs`
   - Extend `ScanContext` with `estimated_duration`
   - Add i18n keys
   - Write tests

2. **Phase 2: Dry-Run Enhancement** (Quick Win)
   - Update dry-run output to show time estimate
   - Add i18n key
   - Write tests

3. **Phase 3: Progress Estimation** (User-Facing)
   - Enhance `MergeProgress` with ETA
   - Update `execute()` to show estimated time
   - Add i18n key
   - Write tests

4. **Phase 4: Interactive Mode** (Safety Feature)
   - Add `--interactive` flag
   - Implement `show_preview_and_confirm()`
   - Add i18n keys
   - Write tests

5. **Phase 5: Interrupt Handling** (Robustness)
   - Add interrupt checks in merge loop
   - Implement partial file cleanup
   - Update interrupt messages
   - Add i18n keys
   - Write tests

---

## Testing Strategy

### Unit Tests
- Time estimation accuracy (various file sizes)
- Interactive flag parsing
- Interrupt flag checking
- ETA calculation

### Integration Tests
- Dry-run with time estimate
- Interactive mode (mock stdin)
- Interrupt during merge (mock Ctrl+C)
- Partial file cleanup

### Manual Testing
- Run with real video files
- Test interrupt at various stages
- Verify time estimates are reasonable
- Test resume after interrupt

---

## Migration Notes

- No breaking changes to existing CLI
- All new features are opt-in via flags
- Existing `--dry-run` behavior preserved
- Time estimation is best-effort (file size based)

---

## Future Enhancements (Out of Scope)

- Machine learning for better time estimation
- Per-codec throughput tracking
- Network drive detection and adjustment
- Parallel merge time estimation
