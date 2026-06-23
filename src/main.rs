mod cli;
mod ffmpeg;
mod i18n;
use crate::i18n::{t, tf};
mod merger;
mod progress;
mod scanner;
mod state;

use anyhow::{Context, Result};
use cli::Args;
use colored::Colorize;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;

mod exit_codes {
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

/// Application-level errors with typed variants for exit code mapping.
///
/// Using thiserror eliminates the fragile string-based error classification
/// that was previously done via error_category().
#[derive(Debug, Error)]
enum AppError {
    #[error("ffmpeg not found")]
    FfmpegNotFound,

    #[error("{count} merge(s) failed")]
    MergeFailed { count: usize },

    #[error("source directory is not readable: {path}")]
    UnreadableSource { path: String },
}

impl AppError {
    fn exit_code(&self) -> i32 {
        match self {
            AppError::FfmpegNotFound => exit_codes::FFMPEG_NOT_FOUND,
            AppError::MergeFailed { .. } => exit_codes::MERGE_FAILED,
            AppError::UnreadableSource { .. } => exit_codes::GENERAL_ERROR,
        }
    }
}

/// Extract the exit code from an anyhow error.
/// Tries downcasting to AppError first, then returns GENERAL_ERROR.
fn get_exit_code(e: &anyhow::Error) -> i32 {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        return app_err.exit_code();
    }

    exit_codes::GENERAL_ERROR
}

/// Translate AppError messages for user-facing output.
/// Returns the original error string for non-AppError types.
fn translate_error(e: &anyhow::Error) -> String {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        match app_err {
            AppError::FfmpegNotFound => t("ffmpeg_not_found").to_string(),
            AppError::MergeFailed { count } => tf("merge_failed", &[&count.to_string()]),
            AppError::UnreadableSource { path } => tf("unreadable_source", &[path]),
        }
    } else {
        e.to_string()
    }
}

/// Initialize the application: parse args, verify ffmpeg, ensure directories.
fn init() -> Result<(Args, cli::OutputFormat)> {
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

    let scan_result = scanner::scan_directory(&args.source, args.recursive).map_err(|_| {
        AppError::UnreadableSource {
            path: args.source.display().to_string(),
        }
    })?;

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

/// Print preview of pairs to be processed, including header and time estimate.
fn print_preview_pairs(ctx: &ScanContext, format: &cli::OutputFormat, header: &str) {
    println!("{}", header.cyan().bold());
    for pair in &ctx.pairs {
        println!(
            "  {} + {} -> {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    if let Some(msg) = ctx.format_time_estimate() {
        println!("{}", msg);
    }
}

/// Shared flag set by the Ctrl+C handler.
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", t("error_prefix").red(), translate_error(&e));
        std::process::exit(get_exit_code(&e));
    }
}

fn run() -> Result<()> {
    // Set up Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .context(t("failed_set_signal").into_owned())?;

    // Phase 1: Initialize
    let (args, format) = init()?;

    // Configure rayon's global thread pool once at startup
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build_global()
        .ok();

    // Phase 2: Scan and filter
    let Some((ctx, mut merge_state)) = scan_and_filter(&args)? else {
        return Ok(());
    };

    // Phase 2.5: Interactive preview + confirmation
    if args.interactive {
        print_preview_pairs(&ctx, &format, &t("interactive_preview_header"));
        if args.sdel {
            println!("{}", t("dry_run_sdel_header").yellow().bold());
        }
        println!();
        print!("{} ", t("interactive_confirm").bold());
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();
        if response != "y" && response != "yes" {
            println!("{}", t("interactive_cancelled").yellow());
            state::MergeState::clear(&args.source)?;
            return Ok(());
        }
    }

    // Phase 3: Dry-run preview or execute merges
    if args.dry_run {
        if !args.quiet {
            print_preview_pairs(&ctx, &format, &t("dry_run_header"));
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
        }
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

/// Merges between incremental state saves to limit progress loss on interrupt.
const STATE_SAVE_INTERVAL: usize = 5;

/// Execute the actual merge operations for all filtered pairs.
fn execute(
    args: &Args,
    ctx: ScanContext,
    merge_state: &mut state::MergeState,
    format: cli::OutputFormat,
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

    let mut final_summary = merger::MergeSummary::default();

    // Run all pairs in one parallel batch with periodic state saves
    let state_mutex = std::sync::Arc::new(std::sync::Mutex::new(merge_state));
    let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let save_interval = STATE_SAVE_INTERVAL;
    let source = args.source.clone();

    let results: Vec<merger::MergeResult> = ctx
        .pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            let result = merger::merge_pair(
                pair,
                idx,
                &args.output,
                format,
                progress.as_ref(),
                args.verbose,
                args.retry,
            );

            // Update state and periodically save
            let snapshot = {
                let mut state = state_mutex.lock().unwrap();
                if !result.was_interrupted {
                    if result.success {
                        state.mark_completed(&pair.stem);
                    } else {
                        state.mark_failed(&pair.stem);
                    }
                }
                let count = completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if count.is_multiple_of(save_interval) {
                    Some(state.clone())
                } else {
                    None
                }
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
        let state = state_mutex.lock().unwrap();
        if let Err(e) = state.save(&source) {
            eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
        }
    }

    // Finish progress bar
    if let Some(p) = &progress {
        p.finish();
    }

    // Cleanup partial output files on interrupt
    if INTERRUPTED.load(Ordering::SeqCst) {
        for result in &results {
            if result.was_interrupted {
                let pair = &ctx.pairs[result.pair_index];
                let output_path = args.output.join(format!("{}.{}", pair.stem, format.extension()));
                if output_path.exists() {
                    if let Err(e) = std::fs::remove_file(&output_path) {
                        eprintln!("{} {}", t("warning_prefix").yellow(), e);
                    } else if args.verbose {
                        eprintln!("{}", tf("cleanup_partial", &[&output_path.display().to_string()]).yellow());
                    }
                }
                // Also clean up source audio file when --sdel is active
                if args.sdel && pair.audio.exists() {
                    if let Err(e) = std::fs::remove_file(&pair.audio) {
                        eprintln!("{} {}", t("warning_prefix").yellow(), e);
                    } else if args.verbose {
                        eprintln!("{}", tf("cleanup_partial", &[&pair.audio.display().to_string()]).yellow());
                    }
                }
            }
        }
    }

    // Handle source file deletion if requested
    let mut deletion_failures = 0;
    if args.sdel {
        for result in &results {
            if result.success {
                let pair = &ctx.pairs[result.pair_index];
                if let Err(e) = merger::delete_source_files(pair) {
                    eprintln!("{} {}", t("warning_prefix").yellow(), e);
                    deletion_failures += 1;
                }
            }
        }
    }

    // Accumulate results into final_summary
    for result in &results {
        if result.success {
            final_summary.success_count += 1;
            final_summary.merge_count += 1;
            final_summary.total_duration += result.duration;
        } else if !result.was_interrupted {
            // Only count non-interrupted failures; interrupted items stay pending for resume
            final_summary.failed_count += 1;
            if let Some(ref err) = result.error {
                final_summary
                    .failures
                    .push((result.pair_name.clone(), err.clone()));
            }
        }
    }
    final_summary.deletion_failures = deletion_failures;

    final_summary.skipped_count = ctx.stats.skipped;
    final_summary.orphaned_count = ctx.stats.orphaned;

    Ok(final_summary)
}

/// Finalize: update state based on results, clear on full success or save otherwise.
fn finalize(
    args: &Args,
    mut merge_state: state::MergeState,
    summary: &merger::MergeSummary,
) -> Result<()> {
    if args.dry_run {
        return Ok(());
    }

    // Mark all failures (interrupted items are already excluded from summary.failures)
    for (name, _error) in &summary.failures {
        merge_state.mark_failed(name);
    }

    if summary.all_success() {
        state::MergeState::clear(&args.source)?;
    } else {
        merge_state.save(&args.source)?;
    }
    Ok(())
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_exit_codes_constants() {
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::FFMPEG_NOT_FOUND, 2);
        assert_eq!(exit_codes::MERGE_FAILED, 3);
    }

    #[test]
    fn test_state_save_interval_constant() {
        // Ensure reasonable batch size for incremental saves
        const _: () = assert!(STATE_SAVE_INTERVAL >= 1 && STATE_SAVE_INTERVAL <= 20);
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
    fn test_app_error_display() {
        assert_eq!(AppError::FfmpegNotFound.to_string(), "ffmpeg not found");
        assert_eq!(
            AppError::MergeFailed { count: 3 }.to_string(),
            "3 merge(s) failed"
        );
        assert_eq!(
            AppError::UnreadableSource {
                path: "/tmp/test".to_string()
            }
            .to_string(),
            "source directory is not readable: /tmp/test"
        );
    }

    #[test]
    fn test_app_error_exit_codes() {
        assert_eq!(
            AppError::FfmpegNotFound.exit_code(),
            exit_codes::FFMPEG_NOT_FOUND
        );
        assert_eq!(
            AppError::MergeFailed { count: 1 }.exit_code(),
            exit_codes::MERGE_FAILED
        );
        assert_eq!(
            AppError::UnreadableSource {
                path: "test".to_string()
            }
            .exit_code(),
            exit_codes::GENERAL_ERROR
        );
    }

    #[test]
    fn test_app_error_into_anyhow() {
        let app_err = AppError::FfmpegNotFound;
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(anyhow_err.to_string(), "ffmpeg not found");
    }

    #[test]
    fn test_get_exit_code_typed_app_error() {
        let app_err = AppError::FfmpegNotFound;
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(get_exit_code(&anyhow_err), exit_codes::FFMPEG_NOT_FOUND);
    }

    #[test]
    fn test_get_exit_code_merge_failed() {
        let app_err = AppError::MergeFailed { count: 3 };
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(get_exit_code(&anyhow_err), exit_codes::MERGE_FAILED);
    }

    #[test]
    fn test_get_exit_code_non_app_error() {
        let err = anyhow::anyhow!("Something went wrong");
        assert_eq!(get_exit_code(&err), exit_codes::GENERAL_ERROR);
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

        let args = cli::Args {
            source: source.clone(),
            output: output.clone(),
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
        };

        // video3 remains pending (never processed, e.g. interrupted)
        let summary = merger::MergeSummary {
            success_count: 1,
            failed_count: 1,
            merge_count: 2,
            skipped_count: 0,
            orphaned_count: 0,
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
}
