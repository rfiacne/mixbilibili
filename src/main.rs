mod cli;
mod ffmpeg;
mod merger;
mod progress;
mod scanner;
mod state;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Args;
use colored::Colorize;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// Tries downcasting to AppError first, then falls back to string matching.
fn get_exit_code(e: &anyhow::Error) -> i32 {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        return app_err.exit_code();
    }

    let msg = e.to_string();
    if msg.contains("ffmpeg") || msg.contains("FFmpeg") {
        return exit_codes::FFMPEG_NOT_FOUND;
    }
    if msg.contains("merge") || msg.contains("Merge") {
        return exit_codes::MERGE_FAILED;
    }

    exit_codes::GENERAL_ERROR
}

/// Initialize the application: parse args, verify ffmpeg, ensure directories.
fn init() -> Result<(Args, cli::OutputFormat)> {
    let mut args = Args::parse();
    args.validate()?;

    let format = args.format;

    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }

    if !args.output.exists() {
        std::fs::create_dir_all(&args.output).context("Failed to create output directory")?;
    }

    Ok((args, format))
}

/// Result of the scan-and-filter phase.
struct ScanContext {
    /// Pairs ready for merging (filtered by resume state).
    pairs: Vec<scanner::FilePair>,
    /// Original scan statistics (skipped, orphaned, etc.).
    stats: scanner::ScanStats,
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

    let scan_result =
        scanner::scan_directory(&args.source).map_err(|_| AppError::UnreadableSource {
            path: args.source.display().to_string(),
        })?;

    // Filter out already completed if resuming
    let pairs_to_process: Vec<_> = if let Some(ref state) = existing_state {
        scan_result
            .pairs
            .iter()
            .filter(|p| !state.is_completed(&p.stem))
            .cloned()
            .collect()
    } else {
        scan_result.pairs.clone()
    };

    if pairs_to_process.is_empty() {
        if existing_state.is_some() {
            println!(
                "{}",
                "All files already merged from previous session".green()
            );
        } else {
            println!("{}", "No file pairs to merge".yellow());
        }
        return Ok(None);
    }

    // Initialize state for tracking
    let mut merge_state =
        existing_state.unwrap_or_else(|| state::MergeState::new(&args.source, &args.output, ""));

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
        stats: scan_result.stats,
    };

    Ok(Some((ctx, merge_state)))
}

/// Shared flag set by the Ctrl+C handler.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);
        std::process::exit(get_exit_code(&e));
    }
}

fn run() -> Result<()> {
    // Set up Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .context("Failed to set signal handler")?;

    // Phase 1: Initialize
    let (args, format) = init()?;

    // Phase 2: Scan and filter
    let Some((ctx, mut merge_state)) = scan_and_filter(&args)? else {
        return Ok(());
    };

    // Phase 3: Execute merges (with incremental state saves)
    let summary = execute(&args, &ctx, &mut merge_state, format)?;

    // Check if interrupted during execution
    if INTERRUPTED.load(Ordering::SeqCst) {
        println!("{}", "\nInterrupted. State saved for resume.".yellow());
    }

    // Phase 4: Update state and report
    finalize(&args, merge_state, &summary)?;

    if args.dry_run {
        println!("{}", "Dry-run complete. No files were modified.".cyan());
    }

    summary.print_report();

    if INTERRUPTED.load(Ordering::SeqCst) {
        // User requested shutdown - exit gracefully without error
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
    ctx: &ScanContext,
    merge_state: &mut state::MergeState,
    format: cli::OutputFormat,
) -> Result<merger::MergeSummary> {
    println!("Processing {} file pairs...", ctx.pairs.len());

    let progress = if args.progress {
        Some(progress::MergeProgress::new(ctx.pairs.len()))
    } else {
        None
    };

    let mut final_summary = merger::MergeSummary::default();

    for chunk in ctx.pairs.chunks(STATE_SAVE_INTERVAL) {
        if INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }

        let scan_result = scanner::ScanResult {
            pairs: chunk.to_vec(),
            stats: scanner::ScanStats::default(),
            skipped_names: vec![],
        };

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
        );

        accumulate_summary(&mut final_summary, &batch_summary);

        if !args.dry_run {
            update_state_from_batch(merge_state, chunk, &batch_summary);
            if let Err(e) = merge_state.save(&args.source) {
                eprintln!("Warning: failed to save incremental state: {e}");
            }
        }
    }

    final_summary.skipped_count = ctx.stats.skipped;
    final_summary.orphaned_count = ctx.stats.orphaned;

    Ok(final_summary)
}

fn accumulate_summary(final_summary: &mut merger::MergeSummary, batch: &merger::MergeSummary) {
    final_summary.success_count += batch.success_count;
    final_summary.failed_count += batch.failed_count;
    final_summary
        .durations
        .extend(batch.durations.iter().copied());
    final_summary
        .failures
        .extend(batch.failures.iter().cloned());
}

fn update_state_from_batch(
    state: &mut state::MergeState,
    chunk: &[scanner::FilePair],
    batch: &merger::MergeSummary,
) {
    for (name, _) in &batch.failures {
        state.mark_failed(name);
    }
    for pair in chunk {
        if !batch.failures.iter().any(|(n, _)| n == &pair.stem) {
            state.mark_completed(&pair.stem);
        }
    }
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

    for (name, _) in &summary.failures {
        merge_state.mark_failed(name);
    }
    for pair in merge_state.pending.clone() {
        if !summary.failures.iter().any(|(name, _)| name == &pair) {
            merge_state.mark_completed(&pair);
        }
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
        assert!(STATE_SAVE_INTERVAL >= 1 && STATE_SAVE_INTERVAL <= 20);
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
    fn test_get_exit_code_generic() {
        let err = anyhow::anyhow!("Something went wrong");
        assert_eq!(get_exit_code(&err), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn test_get_exit_code_fallback_string_match() {
        // Fallback: anyhow errors containing "ffmpeg" get FFMPEG_NOT_FOUND
        let err = anyhow::anyhow!("ffmpeg not found in PATH");
        assert_eq!(get_exit_code(&err), exit_codes::FFMPEG_NOT_FOUND);

        let err2 = anyhow::anyhow!("Some merges failed to complete");
        assert_eq!(get_exit_code(&err2), exit_codes::MERGE_FAILED);
    }
}
