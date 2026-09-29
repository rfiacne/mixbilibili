use crate::cli::{self, Args, OutputFormat};
use crate::error::AppError;
use crate::i18n::{t, tf};
use crate::INTERRUPTED;
use crate::{ffmpeg, merger, progress, scanner, state};
use anyhow::{Context, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Initialize the application: parse args, verify ffmpeg, ensure directories.
fn init() -> Result<(Args, OutputFormat)> {
    let matches = cli::build_cli().get_matches();
    let mut args = cli::parse_args(&matches);
    args.validate()?;

    let format = args.format;

    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }

    if !args.output.exists() {
        std::fs::create_dir_all(&args.output).context(t("failed_create_output").into_owned())?;
    }

    Ok((args, format))
}

/// Result of the scan-and-filter phase.
struct ScanContext {
    /// Pairs ready for merging (filtered by resume state).
    pairs: Vec<scanner::FilePair>,
    /// Original scan statistics (skipped, orphaned, etc.).
    stats: scanner::ScanStats,
    /// Estimated total merge duration.
    estimated_duration: Duration,
}

impl ScanContext {
    fn format_time_estimate(&self) -> Option<String> {
        if self.estimated_duration.is_zero() {
            None
        } else {
            Some(tf(
                "dry_run_time_estimate",
                &[&progress::format_duration(self.estimated_duration)],
            ))
        }
    }
}

/// Scan source directory and filter pairs based on resume state.
/// Returns the scan context and initialized merge state.
fn scan_and_filter(args: &Args) -> Result<Option<(ScanContext, state::MergeState)>> {
    // Check for resume state
    let existing_state = if args.resume {
        state::MergeState::load(&args.source)?
    } else {
        None
    };

    let scan_result = scanner::scan_directory(&args.source, args.recursive).map_err(|e| {
        // Scanner domain errors (e.g. output-name collision) already carry a
        // user-facing message — propagate as-is. Only wrap raw IO failures
        // with the source path context.
        if e.downcast_ref::<std::io::Error>().is_some() {
            AppError::UnreadableSource {
                path: args.source.display().to_string(),
            }
            .into()
        } else {
            e
        }
    })?;

    // Warn about completed outputs whose size differs from their recorded
    // anchor (possible external modification). Non-blocking: resume proceeds.
    if let Some(ref state) = existing_state {
        for (stem, recorded, actual) in check_modified_outputs(state, &args.output, args.format) {
            let output_path = merger::output_path(&args.output, &stem, args.format);
            eprintln!(
                "{}",
                tf(
                    "output_modified_warning",
                    &[
                        &output_path.display().to_string(),
                        &format!("{recorded}→{actual}")
                    ]
                )
            );
        }
    }

    // Filter out already completed if resuming
    let pairs_to_process: Vec<_> = if let Some(ref state) = existing_state {
        scan_result
            .pairs
            .into_iter()
            .filter(|p| !state.is_completed(&p.stem))
            .collect()
    } else {
        scan_result.pairs
    };

    if pairs_to_process.is_empty() {
        if existing_state.is_some() {
            println!("{}", t("all_merged").green());
        } else {
            println!("{}", t("no_pairs").yellow());
        }
        return Ok(None);
    }

    // Initialize state for tracking
    let mut merge_state =
        existing_state.unwrap_or_else(|| state::MergeState::new(&args.source, &args.output));

    // Add pending items
    for pair in &pairs_to_process {
        merge_state.add_pending(&pair.stem);
    }

    // Save state before starting (in case of interruption)
    if !args.dry_run {
        merge_state.save(&args.source)?;
    }

    let ctx = ScanContext {
        pairs: pairs_to_process,
        stats: scan_result.stats.clone(),
        estimated_duration: scan_result.estimated_duration,
    };

    Ok(Some((ctx, merge_state)))
}

/// Parse user response for (Y/n) prompt. Returns true if the user wants to proceed.
/// When `destructive` is true (e.g. --sdel), only explicit "y"/"yes" confirms (default-reject).
/// Otherwise, only "n"/"no" rejects (default-accept), matching the (Y/n) convention.
fn parse_prompt_response(input: &str, destructive: bool) -> bool {
    let response = input.trim().to_lowercase();
    if destructive {
        response == "y" || response == "yes"
    } else {
        !(response == "n" || response == "no")
    }
}

/// Return (stem, recorded_size, actual_size) for completed stems whose output
/// file size no longer matches the recorded anchor. Non-blocking diagnostic.
fn check_modified_outputs(
    state: &state::MergeState,
    output_dir: &std::path::Path,
    format: OutputFormat,
) -> Vec<(String, u64, u64)> {
    let mut modified = Vec::new();
    for stem in &state.completed {
        if let Some(recorded) = state.anchor_for(stem) {
            let output_path = merger::output_path(output_dir, stem, format);
            if let Ok(m) = std::fs::metadata(&output_path) {
                if m.len() != recorded {
                    modified.push((stem.clone(), recorded, m.len()));
                }
            }
        }
    }
    modified.sort_by(|a, b| a.0.cmp(&b.0));
    modified
}

/// --once mode: merge a single pair identified by stem.
/// No scanning, no state file, no progress bar — just merge and exit.
fn run_once(args: &Args, format: OutputFormat, stem: &str) -> Result<()> {
    // Validate stem before using it
    cli::validate_stem(stem)?;

    let video_path = args.source.join(format!("{stem}.mp4"));
    let audio_path = args.source.join(format!("{stem}.m4a"));

    if !video_path.exists() {
        anyhow::bail!(
            "{}",
            tf("once_missing_video", &[&video_path.display().to_string()])
        );
    }
    if !audio_path.exists() {
        anyhow::bail!(
            "{}",
            tf("once_missing_audio", &[&audio_path.display().to_string()])
        );
    }

    let pair = scanner::FilePair {
        video: video_path,
        audio: audio_path,
        stem: stem.to_string(),
    };

    if args.dry_run {
        print_dry_run(args, format, std::slice::from_ref(&pair), None);
        return Ok(());
    }

    let result = merger::merge_pair(
        &pair,
        0,
        &merger::MergeOptions {
            output_dir: &args.output,
            format,
            progress: None,
            verbose: args.verbose,
            max_retries: args.retry,
            runner: None,
        },
    );

    if result.success {
        if args.sdel {
            if let Err(e) = merger::delete_source_files(&pair) {
                eprintln!("{} {}", t("warning_prefix").yellow(), e);
            }
        }
        return Ok(());
    }

    // Clean up the partial output left by a failed or interrupted merge.
    let output_path = merger::output_path(&args.output, stem, format);
    if output_path.exists() {
        if let Err(e) = std::fs::remove_file(&output_path) {
            eprintln!(
                "{} {}",
                t("warning_prefix").yellow(),
                tf(
                    "failed_cleanup_partial",
                    &[&output_path.display().to_string(), &e.to_string()]
                )
            );
        }
    }

    // Ctrl+C is the user's doing, not a merge failure. `--once` writes no
    // checkpoint, so there is nothing to resume from, but it still stops the same
    // way the batch path does instead of reporting a failed merge.
    if result.was_interrupted {
        println!("{}", t("interrupted_cleanup").yellow());
        return Ok(());
    }

    Err(AppError::MergeFailed { count: 1 }.into())
}

/// Prints the dry-run preview for `pairs`: what would be merged, and what would
/// be deleted. Shared by the scan path and `--once`, so `-n` means the same thing
/// in both modes (and touches no files in either).
fn print_dry_run(
    args: &Args,
    format: OutputFormat,
    pairs: &[scanner::FilePair],
    time_estimate: Option<&str>,
) {
    if args.quiet {
        return;
    }
    crate::ui::print_preview_pairs(pairs, format, &t("dry_run_header"), time_estimate);
    if args.sdel {
        crate::ui::print_deletion_preview(pairs, &t("dry_run_sdel_header"));
    }
    println!("\n{}", tf("dry_run_summary", &[&pairs.len().to_string()]));
    println!("{}", t("dry_run_complete").cyan());
}

/// Merges between incremental state saves to limit progress loss on interrupt.
const STATE_SAVE_INTERVAL: usize = 5;

/// Execute the actual merge operations for all filtered pairs.
fn execute(
    args: &Args,
    ctx: ScanContext,
    merge_state: &mut state::MergeState,
    format: OutputFormat,
) -> Result<merger::MergeSummary> {
    if !args.quiet {
        if let Some(msg) = ctx.format_time_estimate() {
            println!("{}", msg);
        } else {
            println!("{}", tf("processing", &[&ctx.pairs.len().to_string()]));
        }
    }

    let progress = if args.progress && !args.quiet {
        Some(progress::MergeProgress::new(ctx.pairs.len()))
    } else {
        None
    };

    let results = merge_all_pairs(args, &ctx.pairs, merge_state, format, progress.as_ref());

    if let Some(p) = &progress {
        p.finish();
    }

    if INTERRUPTED.load(Ordering::SeqCst) {
        cleanup_interrupted_outputs(args, &ctx.pairs, format, &results);
    }

    let deletion_failures = if args.sdel {
        delete_succeeded_sources(&ctx.pairs, &results)
    } else {
        0
    };

    Ok(summarize(&ctx, &results, deletion_failures))
}

/// Run every pair in one parallel batch, updating the shared state after each
/// merge and saving a snapshot every [`STATE_SAVE_INTERVAL`] completions.
fn merge_all_pairs(
    args: &Args,
    pairs: &[scanner::FilePair],
    merge_state: &mut state::MergeState,
    format: OutputFormat,
    progress: Option<&progress::MergeProgress>,
) -> Vec<merger::MergeResult> {
    let options = merger::MergeOptions {
        output_dir: &args.output,
        format,
        progress,
        verbose: args.verbose,
        max_retries: args.retry,
        runner: None,
    };

    let state_mutex = std::sync::Arc::new(std::sync::Mutex::new(merge_state));
    let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let source = args.source.clone();

    let results: Vec<merger::MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            let result = merger::merge_pair(pair, idx, &options);

            // Update state and periodically save
            let snapshot = {
                // Handle mutex poisoning: recover data even if a worker panicked
                let mut state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
                if !result.was_interrupted {
                    if result.success {
                        state.mark_completed(&pair.stem);
                        // Record the merged output size as a resume-time anchor.
                        let output_path = merger::output_path(&args.output, &pair.stem, format);
                        if let Ok(m) = std::fs::metadata(&output_path) {
                            state.record_anchor(&pair.stem, m.len());
                        }
                    } else {
                        state.mark_failed(&pair.stem);
                    }
                }
                let count = completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                count
                    .is_multiple_of(STATE_SAVE_INTERVAL)
                    .then(|| state.clone())
            }; // lock dropped here

            // Save outside the lock so parallel workers aren't blocked by file I/O
            if let Some(s) = snapshot {
                if let Err(e) = s.save(&source) {
                    eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
                }
            }

            result
        })
        .collect();

    // Final state save
    {
        // Handle mutex poisoning: recover data even if a worker panicked
        let state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = state.save(&source) {
            eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
        }
    }

    results
}

/// Delete the half-written outputs of merges that Ctrl+C interrupted.
///
/// Source files are deliberately kept: `ffmpeg -c copy` never consumes them, so
/// they remain valid for `--resume`.
fn cleanup_interrupted_outputs(
    args: &Args,
    pairs: &[scanner::FilePair],
    format: OutputFormat,
    results: &[merger::MergeResult],
) {
    for result in results {
        if !result.was_interrupted {
            continue;
        }

        let pair = &pairs[result.pair_index];
        let output_path = merger::output_path(&args.output, &pair.stem, format);
        if !output_path.exists() {
            continue;
        }

        if let Err(e) = std::fs::remove_file(&output_path) {
            eprintln!("{} {}", t("warning_prefix").yellow(), e);
        } else if args.verbose {
            eprintln!(
                "{}",
                tf("cleanup_partial", &[&output_path.display().to_string()]).yellow()
            );
        }
    }
}

/// Delete the source files of every successful merge.
/// Returns how many deletions failed.
fn delete_succeeded_sources(pairs: &[scanner::FilePair], results: &[merger::MergeResult]) -> usize {
    let mut failures = 0;
    for result in results {
        if !result.success {
            continue;
        }
        if let Err(e) = merger::delete_source_files(&pairs[result.pair_index]) {
            eprintln!("{} {}", t("warning_prefix").yellow(), e);
            failures += 1;
        }
    }
    failures
}

/// Fold the per-pair results and scan statistics into the final report.
fn summarize(
    ctx: &ScanContext,
    results: &[merger::MergeResult],
    deletion_failures: usize,
) -> merger::MergeSummary {
    let mut summary = merger::MergeSummary {
        deletion_failures,
        skipped_count: ctx.stats.skipped,
        orphaned_count: ctx.stats.orphaned,
        ..Default::default()
    };

    for result in results {
        if result.success {
            summary.success_count += 1;
            summary.merge_count += 1;
            summary.total_duration += result.duration;
        } else if result.was_interrupted {
            // Not a failure: the pair stays pending and `--resume` retries it.
            summary.interrupted_count += 1;
        } else {
            summary.failed_count += 1;
            if let Some(ref err) = result.error {
                summary
                    .failures
                    .push((result.pair_name.clone(), err.clone()));
            }
        }
    }

    summary
}

/// Finalize: clear state on full success, save otherwise.
/// Note: execute() already marks completed/failed items in the state via the mutex,
/// so we only need to decide whether to clear or persist the state file.
fn finalize(
    args: &Args,
    merge_state: state::MergeState,
    summary: &merger::MergeSummary,
) -> Result<()> {
    if args.dry_run {
        return Ok(());
    }

    // The state file is the resume checkpoint, so it may only be dropped once
    // every scanned pair is finished. `all_success()` counts interrupted pairs as
    // unfinished: Ctrl+C during a batch leaves work pending, and deleting the
    // checkpoint would make the `--resume` hint printed right after it useless.
    if summary.all_success() {
        state::MergeState::clear(&args.source)?;
    } else {
        merge_state.save(&args.source)?;
    }
    Ok(())
}

/// Main workflow: init → scan → interactive/dry-run → execute → finalize/report.
pub fn run() -> Result<()> {
    // Reset SIGCHLD to SIG_DFL once, before any threads are created.
    // aria2 sets SIGCHLD to SIG_IGN in its on-download-complete hook,
    // which causes waitpid() to fail with ECHILD.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
    }

    // Phase 1: Initialize (parse args, verify ffmpeg)
    let (args, format) = init()?;

    // Set ASCII mode environment variable BEFORE creating any threads.
    // POSIX: setenv is UB in multi-threaded programs.
    if args.ascii {
        std::env::set_var("MIXBILIBILI_ASCII", "1");
    }

    // Set up Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .context(t("failed_set_signal").into_owned())?;

    // --once mode: merge a single pair by stem, skip scanning
    if let Some(ref stem) = args.once {
        return run_once(&args, format, stem);
    }

    // Configure rayon's global thread pool once at startup
    if let Err(e) = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build_global()
    {
        eprintln!(
            "{} {}",
            t("warning_prefix"),
            tf("failed_build_pool", &[&e.to_string()])
        );
    }

    // Phase 2: Scan and filter
    let Some((ctx, mut merge_state)) = scan_and_filter(&args)? else {
        return Ok(());
    };

    // Phase 2.5: Interactive preview + confirmation
    if args.interactive {
        use crate::ui::Colors;

        crate::ui::print_preview_pairs(
            &ctx.pairs,
            format,
            &t("interactive_preview_header"),
            ctx.format_time_estimate().as_deref(),
        );
        if args.sdel {
            println!("{}", Colors::warning(&t("dry_run_sdel_header")));
        }
        println!();

        let prompt = format!(
            "{} {} \u{203a} ({}/{})",
            Colors::info("?"),
            t("prompt_continue"),
            Colors::success(&t("prompt_default_yes")),
            Colors::secondary(&t("prompt_default_no")),
        );
        print!("{} ", prompt);
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !parse_prompt_response(&input, args.sdel) {
            println!("{}", Colors::warning(&t("interactive_cancelled")));
            state::MergeState::clear(&args.source)?;
            return Ok(());
        }
    }

    // Phase 3: Dry-run preview or execute merges
    if args.dry_run {
        print_dry_run(
            &args,
            format,
            &ctx.pairs,
            ctx.format_time_estimate().as_deref(),
        );
        return Ok(());
    }

    let summary = execute(&args, ctx, &mut merge_state, format)?;

    // Phase 4: Update state and report
    finalize(&args, merge_state, &summary)?;

    summary.print_report(args.quiet);

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// `Args` for a test: quiet, single-job, MKV, source and output both `dir`.
    fn test_args(dir: &std::path::Path, once: Option<&str>) -> cli::Args {
        cli::Args {
            source: dir.to_path_buf(),
            output: dir.to_path_buf(),
            recursive: false,
            format: cli::OutputFormat::Mkv,
            jobs: 1,
            sdel: false,
            dry_run: false,
            interactive: false,
            resume: false,
            progress: false,
            quiet: true,
            verbose: false,
            retry: 0,
            once: once.map(str::to_string),
            ascii: false,
        }
    }

    #[test]
    fn test_state_save_interval_constant() {
        // Ensure reasonable batch size for incremental saves
        const _: () = assert!(STATE_SAVE_INTERVAL >= 1 && STATE_SAVE_INTERVAL <= 20);
    }

    #[test]
    fn test_run_once_rejects_path_traversal() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Create dummy files
        File::create(dir.path().join("test.mp4")).unwrap();
        File::create(dir.path().join("test.m4a")).unwrap();

        let args = test_args(dir.path(), Some("../escape"));

        let result = run_once(&args, cli::OutputFormat::Mkv, "../escape");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid") || err.contains("traversal") || err.contains("path separator"),
            "Error should mention invalid stem: {}",
            err
        );
    }

    #[test]
    fn test_run_once_rejects_absolute_path() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        let args = test_args(dir.path(), Some("/etc/passwd"));

        let result = run_once(&args, cli::OutputFormat::Mkv, "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_once_rejects_backslash_traversal() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        let args = test_args(dir.path(), Some("..\\escape"));

        let result = run_once(&args, cli::OutputFormat::Mkv, "..\\escape");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_once_rejects_null_byte() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        let args = test_args(dir.path(), Some("evil\0path"));

        let result = run_once(&args, cli::OutputFormat::Mkv, "evil\0path");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("null") || err.contains("Invalid"),
            "Error should mention null byte or invalid: {}",
            err
        );
    }

    #[test]
    fn test_run_once_accepts_valid_stem() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Create valid pair
        File::create(dir.path().join("video.mp4")).unwrap();
        File::create(dir.path().join("video.m4a")).unwrap();

        let args = test_args(dir.path(), Some("video"));

        // Should not error on validation (may error on ffmpeg, but that's OK)
        let result = run_once(&args, cli::OutputFormat::Mkv, "video");
        // We're only testing validation, not actual merge
        // If it gets past validation, it will try to run ffmpeg
        // So we just check it doesn't error with "invalid stem"
        if let Err(e) = result {
            let err = e.to_string();
            assert!(
                !err.contains("invalid stem"),
                "Valid stem should not be rejected: {}",
                err
            );
        }
    }

    #[test]
    fn test_finalize_does_not_create_dummy_filepair() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(&source, &output);
        state.add_pending("video1");
        state.add_pending("video2");
        state.add_pending("video3");
        // Simulate execute() having already processed video1 and video2
        state.mark_completed("video1");
        state.mark_failed("video2");
        state.save(&source).unwrap();

        let mut args = test_args(dir.path(), None);
        args.source = source.clone();
        args.output = output.clone();

        // video3 remains pending (never processed, e.g. interrupted)
        let summary = merger::MergeSummary {
            success_count: 1,
            failed_count: 1,
            merge_count: 2,
            skipped_count: 0,
            orphaned_count: 0,
            interrupted_count: 0,
            deletion_failures: 0,
            total_duration: std::time::Duration::ZERO,
            failures: vec![("video2".to_string(), "mock error".to_string())],
        };

        finalize(&args, state.clone(), &summary).unwrap();

        // all_success() is false (1 failure), so state file is saved
        let loaded = state::MergeState::load(&source)
            .unwrap()
            .expect("state should be saved");

        // video1 was completed by execute()
        assert!(loaded.is_completed("video1"), "video1 should be completed");
        // video2 was failed by execute()
        assert!(
            loaded.failed.contains("video2"),
            "video2 should be in failed"
        );
        // video3 should NOT be completed — it was never actually merged
        assert!(
            !loaded.is_completed("video3"),
            "finalize should not mark unprocessed pending items as completed"
        );
    }

    #[test]
    fn test_summarize_separates_interrupted_from_failed() {
        let ctx = ScanContext {
            pairs: Vec::new(),
            stats: scanner::ScanStats {
                pairs: 3,
                skipped: 2,
                orphaned: 1,
            },
            estimated_duration: Duration::ZERO,
        };
        let results = vec![
            merger::MergeResult {
                pair_index: 0,
                pair_name: "merged".to_string(),
                success: true,
                error: None,
                duration: Duration::from_millis(10),
                was_interrupted: false,
            },
            merger::MergeResult {
                pair_index: 1,
                pair_name: "stopped".to_string(),
                success: false,
                error: None,
                duration: Duration::ZERO,
                was_interrupted: true,
            },
            merger::MergeResult {
                pair_index: 2,
                pair_name: "broken".to_string(),
                success: false,
                error: Some("boom".to_string()),
                duration: Duration::ZERO,
                was_interrupted: false,
            },
        ];

        let summary = summarize(&ctx, &results, 0);

        assert_eq!(summary.success_count, 1);
        assert_eq!(summary.interrupted_count, 1);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.merge_count, 1);
        assert_eq!(summary.skipped_count, 2);
        assert_eq!(summary.orphaned_count, 1);
        assert_eq!(
            summary.failures,
            vec![("broken".to_string(), "boom".to_string())],
            "an interrupted pair is not a failure and must not be reported as one"
        );
        assert!(
            !summary.all_success(),
            "an interrupted pair leaves the batch unfinished"
        );
    }

    /// Regression: Ctrl+C leaves pairs pending, so the checkpoint must survive.
    /// `run()` prints a `--resume` hint immediately after `finalize()`, which used
    /// to delete the state file because no merge had *failed*.
    #[test]
    fn test_finalize_keeps_checkpoint_when_run_was_interrupted() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let args = test_args(&source, None);

        let mut state = state::MergeState::new(&source, &source);
        state.add_pending("done");
        state.add_pending("unfinished");
        state.mark_completed("done");

        let summary = merger::MergeSummary {
            success_count: 1,
            interrupted_count: 1,
            merge_count: 1,
            ..Default::default()
        };
        finalize(&args, state, &summary).unwrap();

        let loaded = state::MergeState::load(&source)
            .unwrap()
            .expect("state file must survive an interrupt, or --resume redoes finished work");
        assert!(loaded.is_completed("done"), "finished pair stays recorded");
        assert!(
            loaded.pending.contains("unfinished"),
            "interrupted pair stays pending for --resume"
        );
    }

    /// The other direction: a batch with nothing left over drops the checkpoint.
    #[test]
    fn test_finalize_clears_checkpoint_when_everything_finished() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let args = test_args(&source, None);

        let mut state = state::MergeState::new(&source, &source);
        state.add_pending("a");
        state.mark_completed("a");
        state.save(&source).unwrap();

        let summary = merger::MergeSummary {
            success_count: 1,
            merge_count: 1,
            ..Default::default()
        };
        finalize(&args, state, &summary).unwrap();

        assert!(
            state::MergeState::load(&source).unwrap().is_none(),
            "a finished batch should leave no checkpoint behind"
        );
    }

    #[test]
    fn test_resume_with_wrong_source_dir_warns() {
        use tempfile::tempdir;

        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();

        // Save state pointing to dir_a
        let state = state::MergeState::new(dir_a.path(), dir_a.path());
        state.save(dir_a.path()).unwrap();

        // Copy state file to dir_b (simulating user moved files or used wrong path)
        let content =
            std::fs::read_to_string(state::MergeState::state_file_path(dir_a.path())).unwrap();
        std::fs::write(state::MergeState::state_file_path(dir_b.path()), &content).unwrap();

        // Loading from dir_b should detect the source_dir mismatch
        let result = state::MergeState::load(dir_b.path());
        assert!(
            result.is_err(),
            "should error when source_dir doesn't match load path"
        );
    }

    #[test]
    fn test_parse_prompt_response_empty_is_yes() {
        // Non-destructive: empty = yes
        assert!(parse_prompt_response("", false));
        // Destructive: empty = NO (default-reject)
        assert!(!parse_prompt_response("", true));
    }

    #[test]
    fn test_parse_prompt_response_whitespace_is_yes() {
        assert!(parse_prompt_response("   ", false));
        assert!(!parse_prompt_response("   ", true));
    }

    #[test]
    fn test_parse_prompt_response_y_is_yes() {
        assert!(parse_prompt_response("y", false));
        assert!(parse_prompt_response("Y", false));
        assert!(parse_prompt_response("yes", false));
        assert!(parse_prompt_response("Yes", false));
        // Also yes in destructive mode
        assert!(parse_prompt_response("y", true));
        assert!(parse_prompt_response("yes", true));
    }

    #[test]
    fn test_parse_prompt_response_n_is_no() {
        assert!(!parse_prompt_response("n", false));
        assert!(!parse_prompt_response("N", false));
        assert!(!parse_prompt_response("no", false));
        assert!(!parse_prompt_response("No", false));
        assert!(!parse_prompt_response("NO", false));
    }

    #[test]
    fn test_parse_prompt_response_destructive_rejects_unknown() {
        // In destructive mode, anything other than y/yes is rejection
        assert!(!parse_prompt_response("ok", true));
        assert!(!parse_prompt_response("sure", true));
        assert!(!parse_prompt_response("yy", true));
    }

    #[test]
    fn test_interrupted_flag_default() {
        assert!(!INTERRUPTED.load(Ordering::SeqCst));
    }

    #[test]
    fn test_interrupted_flag_can_be_set() {
        INTERRUPTED.store(true, Ordering::SeqCst);
        assert!(INTERRUPTED.load(Ordering::SeqCst));
        INTERRUPTED.store(false, Ordering::SeqCst); // Reset
    }

    #[test]
    fn test_check_modified_outputs_detects_size_change() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        state.completed.insert("video1".to_string());
        state.record_anchor("video1", 100);
        // Actual output is 50 bytes (different from recorded 100)
        std::fs::write(output.join("video1.mkv"), vec![0u8; 50]).unwrap();

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0, "video1");
        assert_eq!(modified[0].1, 100);
        assert_eq!(modified[0].2, 50);
    }

    #[test]
    fn test_check_modified_outputs_ignores_matching_size() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        state.completed.insert("video1".to_string());
        state.record_anchor("video1", 100);
        std::fs::write(output.join("video1.mkv"), vec![0u8; 100]).unwrap();

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert!(modified.is_empty());
    }

    #[test]
    fn test_check_modified_outputs_ignores_stem_without_anchor() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        // completed but no anchor recorded (e.g. migrated v1 state)
        state.completed.insert("video1".to_string());

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert!(modified.is_empty());
    }
}
