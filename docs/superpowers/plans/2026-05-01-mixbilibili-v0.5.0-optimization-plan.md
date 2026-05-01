# mixbilibili v0.5.0 Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve code quality (fix production unwrap/expect), enhance UX (--quiet, better --dry-run, structured report), and harden CI/CD (clippy gate, test, release changelog, size report).

**Architecture:** Three independent layers — code quality fixes in `merger.rs`/`main.rs`, UX additions in `cli.rs`/`merger.rs`/`main.rs`, CI changes in `.github/workflows/`. Each layer produces working, testable software.

**Tech Stack:** Rust (edition 2021), clap, rayon, thiserror, colored, indicatif, anyhow, GitHub Actions

---

### Task 1: Extract `wait_with_timeout` from `merger.rs`

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Add `ChildExt` trait and implementation**

In `src/merger.rs`, before the `merge_pair` function, add a new trait. The current inline polling logic inside `run_with_timeout` (which uses `child.wait_timeout` with a loop) should be extracted:

```rust
/// Extension trait for `std::process::Child` that adds timeout-aware waiting.
trait ChildExt {
    /// Wait for the child process to exit, or return `None` after `timeout`.
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>>;
}

impl ChildExt for std::process::Child {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

Note: `POLL_INTERVAL` constant already exists (`Duration::from_millis(500)`).

- [ ] **Step 2: Replace inline polling in `run_with_timeout`**

Find the `run_with_timeout` function. Replace the inline `wait_timeout` loop with a call to the new trait method:

```rust
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = cmd.spawn().context("Failed to spawn ffmpeg process")?;

    match child.wait_with_timeout(timeout, POLL_INTERVAL) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("ffmpeg process timed out after 5 minutes");
        }
        Err(e) => Err(e).context("Failed to wait for ffmpeg process"),
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: All tests pass (same behavior, just refactored).

- [ ] **Step 4: Commit**

```bash
git add src/merger.rs
git commit -m "refactor: extract ChildExt::wait_with_timeout from inline polling"
```

---

### Task 2: Fix production `expect()` in `merger.rs`

**Files:**
- Modify: `src/merger.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Change `execute_merges` to return `Result<MergeSummary>`**

Find the `execute_merges` function signature. Currently it returns `MergeSummary` directly. Change it to return `Result<MergeSummary>`:

```rust
#[allow(clippy::too_many_arguments)]
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
    progress: Option<MergeProgress>,
    dry_run: bool,
    verbose: bool,
    retry: usize,
) -> Result<MergeSummary> {
    let output_dir = output_dir.to_path_buf();
    let pairs = scan_result.pairs;
    let progress_ref = progress.as_ref();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("Failed to build thread pool")?;

    let results: Vec<MergeResult> = pool.install(|| {
        pairs
            .par_iter()
            .enumerate()
            .map(|(idx, pair)| {
                merge_pair(
                    pair,
                    idx,
                    &output_dir,
                    format,
                    progress_ref,
                    dry_run,
                    verbose,
                    retry,
                )
            })
            .collect()
    });

    // ... rest of the function unchanged (collecting results into MergeSummary)
}
```

- [ ] **Step 2: Update caller in `main.rs`**

In the `execute` function in `src/main.rs`, add `?` to the `execute_merges` call:

```rust
let batch_summary = merger::execute_merges(
    scan_result,
    &args.output,
    format,
    args.jobs,
    args.sdel,
    progress.clone(),
    args.dry_run,
    args.verbose,
    args.retry,
)?;
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/merger.rs src/main.rs
git commit -m "fix: propagate thread pool build error instead of expect()"
```

---

### Task 3: Remove string-matching exit code fallback

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Remove string matching in `get_exit_code`**

Replace the current `get_exit_code` function:

```rust
/// Extract the exit code from an anyhow error.
/// Tries downcasting to AppError first, then returns GENERAL_ERROR.
fn get_exit_code(e: &anyhow::Error) -> i32 {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        return app_err.exit_code();
    }

    exit_codes::GENERAL_ERROR
}
```

This removes the fragile `msg.contains("ffmpeg")` and `msg.contains("merge")` fallbacks.

- [ ] **Step 2: Ensure all error paths use `AppError`**

Verify that all `Err(...)` paths in `run()` return an `AppError` variant:
- `AppError::FfmpegNotFound` — already used in `init()`
- `AppError::MergeFailed { count }` — already used in `finalize()`
- `AppError::UnreadableSource { path }` — already used in `scan_and_filter()`

No changes needed to callers — they already return `AppError`.

- [ ] **Step 3: Update tests**

In `main.rs` tests, remove these two tests that test string matching:

```rust
// DELETE these tests:
// fn test_get_exit_code_fallback_string_match()
// fn test_get_exit_code_generic()  (if it tests generic anyhow)
```

Replace with:

```rust
#[test]
fn test_get_exit_code_non_app_error() {
    let err = anyhow::anyhow!("Something went wrong");
    assert_eq!(get_exit_code(&err), exit_codes::GENERAL_ERROR);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: remove fragile string-matching exit code fallback"
```

---

### Task 4: Add `--quiet` / `-q` flag

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `quiet` field to `Args` struct**

In `src/cli.rs`, add after the `verbose` field:

```rust
    /// Suppress progress output; show only final summary
    #[arg(short = 'q', long, default_value_t = false)]
    pub quiet: bool,
```

- [ ] **Step 2: Update `make_args()` test helper**

In the `make_args()` function in `src/cli.rs`, add:

```rust
        quiet: false,
```

- [ ] **Step 3: Add tests for quiet flag**

In `src/cli.rs` `args_tests` module:

```rust
#[test]
fn test_quiet_flag_default() {
    let args = Args::try_parse_from(["mixbilibili"]).unwrap();
    assert!(!args.quiet);
}

#[test]
fn test_quiet_flag_enabled() {
    let args = Args::try_parse_from(["mixbilibili", "-q"]).unwrap();
    assert!(args.quiet);
}

#[test]
fn test_quiet_flag_long() {
    let args = Args::try_parse_from(["mixbilibili", "--quiet"]).unwrap();
    assert!(args.quiet);
}
```

- [ ] **Step 4: Wire up `quiet` in `main.rs`**

In the `execute` function, change progress creation:

```rust
let progress = if args.progress && !args.quiet {
    Some(progress::MergeProgress::new(ctx.pairs.len()))
} else {
    None
};
```

- [ ] **Step 5: Run tests**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add --quiet flag to suppress progress output"
```

---

### Task 5: Enhance `--dry-run` output

**Files:**
- Modify: `src/merger.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add dry-run pair listing in `main.rs`**

After `scan_and_filter` succeeds, add a dry-run preview before `execute`:

```rust
// In run(), after scan_and_filter, before execute
if args.dry_run {
    println!("{}", "Dry-run mode — the following pairs would be merged:".cyan().bold());
    for pair in &ctx.pairs {
        println!(
            "  {} + {} → {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    if args.sdel {
        println!("\n{}", "The following source files would be deleted:".yellow().bold());
        for pair in &ctx.pairs {
            println!(
                "  {} (video)\n  {} (audio)",
                pair.video.display(),
                pair.audio.display()
            );
        }
    }
    println!("\nWould merge {} pair(s).", ctx.pairs.len());
}
```

- [ ] **Step 2: Verify dry-run skips merges**

The `merge_pair` function already handles `dry_run: true` by returning a simulated success. No changes needed there. The `execute_merges` function passes `dry_run` through to `merge_pair`.

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: enhance --dry-run to list pairs and source files"
```

---

### Task 6: Structured final report

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Rewrite `print_report` method on `MergeSummary`**

Replace the existing `print_report` method:

```rust
pub fn print_report(&self, quiet: bool) {
    if quiet {
        // Compact single-line output
        let total = self.success_count + self.failed_count;
        if self.failed_count > 0 {
            println!(
                "{}/{} merged, {} {}",
                self.success_count,
                total,
                self.failed_count,
                if self.failed_count == 1 { "failed" } else { "failed" }
            );
        } else {
            println!("{}/{} merged", self.success_count, total);
        }
        return;
    }

    // Full structured report
    println!("{}", "================================".bright_black());
    println!("{}", "Merge Report".cyan().bold());
    println!("{}", "================================".bright_black());

    // Counts
    let success_str = format!("{} {} succeeded", "✓".green(), self.success_count);
    let fail_str = if self.failed_count > 0 {
        format!("{} {} failed", "✗".red(), self.failed_count)
    } else {
        format!("{} {} failed", "✗", self.failed_count)
    };
    println!("  {}    {}", success_str, fail_str);

    if self.skipped_count > 0 {
        println!("  {} {} skipped (aria2 downloads)", self.skipped_count, "(aria2)".bright_black());
    }
    if self.orphaned_count > 0 {
        println!("  {} {} orphaned (no matching pair)", self.orphaned_count, "(orphan)".bright_black());
    }

    // Timing
    if !self.durations.is_empty() {
        let total = self.total_duration();
        println!("  {}: {}", "Duration".bright_black(), format_duration(total));
        if let Some(avg) = self.avg_duration() {
            println!("  {}: {}", "Avg".bright_black(), format_duration(avg));
        }
        if let Some(tp) = self.throughput() {
            println!("  {}: {:.2} pairs/sec", "Throughput".bright_black(), tp);
        }
    }

    if self.deletion_failures > 0 {
        println!(
            "  {} {} source file deletion failures",
            self.deletion_failures,
            "(warn)".yellow()
        );
    }

    println!("{}", "================================".bright_black());

    if !self.failures.is_empty() {
        println!("\n{}", "Failed files:".red().bold());
        for (name, error) in &self.failures {
            println!("  {} {}: {}", "✗".red(), name, error);
        }
        println!();
    }
}
```

- [ ] **Step 2: Update all callers of `print_report`**

In `main.rs`, update the call:

```rust
summary.print_report(args.quiet);
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/merger.rs src/main.rs
git commit -m "feat: structured merge report with quiet mode"
```

---

### Task 7: Audit `ffmpeg.rs` for production `unwrap()`

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Check for production `unwrap()` calls**

```bash
grep -n 'unwrap()\|expect(' src/ffmpeg.rs | grep -v '#\[cfg(test)\]'
```

If any production `unwrap()` found, replace with `?` or `.context(...)`.

Based on prior analysis, `ffmpeg.rs` uses `anyhow::Result` properly throughout production code. No changes should be needed. If any are found, handle them.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy -- -D warnings
```

Expected: No warnings.

- [ ] **Step 3: Commit (only if changes made)**

```bash
git add src/ffmpeg.rs
git commit -m "fix: replace production unwrap in ffmpeg.rs"
```

If no changes needed, skip commit.

---

### Task 8: Add PR CI workflow with clippy + test gate

**Files:**
- Create: `.github/workflows/ci-check.yml`

- [ ] **Step 1: Create CI check workflow**

```yaml
name: CI Check

on:
  pull_request:
    branches: [master]

env:
  CARGO_TERM_COLOR: always

jobs:
  lint-and-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: ~/.cargo/registry
          key: ${{ runner.os }}-cargo-registry-${{ hashFiles('Cargo.lock') }}

      - name: Install ffmpeg
        run: |
          sudo apt-get update
          sudo apt-get install -y ffmpeg

      - name: Clippy
        run: cargo clippy -- -D warnings

      - name: Test
        run: cargo test

      - name: Check formatting
        run: cargo fmt -- --check
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci-check.yml
git commit -m "ci: add PR check workflow with clippy, test, and fmt gate"
```

---

### Task 9: Enhance release workflow

**Files:**
- Modify: `.github/workflows/release.yml` (or whatever the release workflow is named)

- [ ] **Step 1: Add test step before build**

In the release workflow, before the "Build release" step, add:

```yaml
      - name: Run tests
        run: cargo test
```

- [ ] **Step 2: Add binary size to step summary**

After the "Build release" step, add:

```yaml
      - name: Binary size report
        shell: bash
        run: |
          SIZE=$(du -h target/${{ matrix.target }}/release/${{ matrix.bin }} | cut -f1)
          echo "Binary: ${{ matrix.bin }} — Size: $SIZE"
          echo "## Binary Size Report" >> $GITHUB_STEP_SUMMARY
          echo "| Target | Binary | Size |" >> $GITHUB_STEP_SUMMARY
          echo "|--------|----------|------|" >> $GITHUB_STEP_SUMMARY
          echo "| ${{ matrix.target }} | ${{ matrix.bin }} | $SIZE |" >> $GITHUB_STEP_SUMMARY
```

- [ ] **Step 3: Extract changelog for release notes**

Replace `generate_release_notes: true` in the release upload step with changelog extraction:

```yaml
      - name: Extract changelog
        id: changelog
        shell: bash
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          NOTES=$(awk "/^## \\[${VERSION}\\]/{p=1;next}/^## \\[/{p=0}p" CHANGELOG.md)
          # Escape for JSON
          NOTES=$(echo "$NOTES" | sed 's/"/\\"/g' | sed ':a;N;$!ba;s/\n/\\n/g')
          echo "notes=$NOTES" >> $GITHUB_OUTPUT

      - name: Upload Release Assets
        uses: softprops/action-gh-release@v1
        with:
          files: mixbilibili-*
          body: ${{ steps.changelog.outputs.notes }}
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: enhance release with test, size report, and changelog extraction"
```

---

## Self-Review

### 1. Spec coverage check

| Spec requirement | Task |
|-----------------|------|
| Fix production unwrap/expect | Task 2 (merger.rs expect), Task 7 (ffmpeg.rs audit) |
| Extract wait_timeout | Task 1 |
| Pure type-driven exit codes | Task 3 |
| --quiet flag | Task 4 |
| --dry-run enhance | Task 5 |
| Structured report | Task 6 |
| Clippy lint gate | Task 8 |
| cargo test in PR | Task 8 |
| Release changelog | Task 9 |
| Release test before build | Task 9 |
| Binary size report | Task 9 |

All covered.

### 2. Placeholder scan

No TBD/TODO/fill-in-later. Every step has actual code. No "similar to Task N" references.

### 3. Type consistency

- `MergeSummary::print_report(&self, quiet: bool)` — defined in Task 6, called in Task 6 with `args.quiet`
- `args.quiet: bool` — defined in Task 4, used in Tasks 4 and 6
- `ChildExt::wait_with_timeout` — defined in Task 1, used in Task 1
- `execute_merges` returns `Result<MergeSummary>` — changed in Task 2, caller updated with `?`
- `args.dry_run` — already exists in codebase, used in Task 5

All consistent.

### 4. Scope check

9 tasks, each bounded. Total files: `cli.rs`, `main.rs`, `merger.rs`, `ffmpeg.rs`, `ci-check.yml` (new), `release.yml`. No unrelated changes.
