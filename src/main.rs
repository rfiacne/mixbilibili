// src/main.rs
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

    #[error("{0}")]
    #[allow(dead_code)]
    Other(anyhow::Error),
}

impl AppError {
    fn exit_code(&self) -> i32 {
        match self {
            AppError::FfmpegNotFound => exit_codes::FFMPEG_NOT_FOUND,
            AppError::MergeFailed { .. } => exit_codes::MERGE_FAILED,
            AppError::UnreadableSource { .. } => exit_codes::GENERAL_ERROR,
            AppError::Other(_) => exit_codes::GENERAL_ERROR,
        }
    }
}

/// Extract the exit code from an anyhow error.
///
/// This replaces the old `error_category()` function that did string matching.
/// It tries downcasting to AppError first (typed match), then falls back to
/// minimal string inspection for errors from internal modules (scanner, state,
/// merger) that still return anyhow.
fn get_exit_code(e: &anyhow::Error) -> i32 {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        return app_err.exit_code();
    }

    // Fallback: internal modules return anyhow errors.
    // We inspect the string only for those, not for external/library errors.
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
        std::fs::create_dir_all(&args.output)
            .context("Failed to create output directory")?;
    }

    Ok((args, format))
}

/// Scan source directory and filter pairs based on resume state.
fn scan_and_filter(args: &Args) -> Result<(Vec<scanner::FilePair>, state::MergeState, usize)> {
    // Check for resume state
    let existing_state = if args.resume {
        state::MergeState::load(&args.source)?
    } else {
        None
    };

    let scan_result = scanner::scan_directory(&args.source)
        .map_err(|_e| AppError::UnreadableSource {
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
        // Return empty result with zero total
        return Ok((vec![], state::MergeState::new(&args.source, &args.output, ""), 0));
    }

    let total_count = pairs_to_process.len();

    // Initialize state for tracking
    let mut merge_state = existing_state
        .unwrap_or_else(|| state::MergeState::new(&args.source, &args.output, ""));

    // Add pending items
    for pair in &pairs_to_process {
        merge_state.add_pending(&pair.stem);
    }

    // Save state before starting (in case of interruption)
    if !args.dry_run {
        merge_state.save(&args.source)?;
    }

    Ok((pairs_to_process, merge_state, total_count))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);
        std::process::exit(get_exit_code(&e));
    }
}

fn run() -> Result<()> {
    // Phase 1: Initialize
    let (args, format) = init()?;

    // Phase 2: Scan and filter
    let (pairs_to_process, mut merge_state, total_count) = scan_and_filter(&args)?;
    if total_count == 0 {
        return Ok(());
    }

    // Phase 3: Execute merges
    println!("Processing {} file pairs...", pairs_to_process.len());

    let progress = if args.progress {
        Some(progress::MergeProgress::new(pairs_to_process.len()))
    } else {
        None
    };

    let filtered_scan_result = scanner::ScanResult {
        pairs: pairs_to_process,
        stats: scanner::ScanStats::default(),
        skipped_names: vec![],
    };

    let summary = merger::execute_merges(
        filtered_scan_result,
        &args.output,
        format,
        args.jobs,
        args.sdel,
        progress,
        args.dry_run,
        args.verbose,
        args.retry,
    );

    // Phase 4: Update state and report
    finalize(&args, &mut merge_state, &summary)?;

    if args.dry_run {
        println!("{}", "Dry-run complete. No files were modified.".cyan());
    }

    summary.print_report();

    if summary.all_success() {
        Ok(())
    } else {
        Err(AppError::MergeFailed { count: summary.failed_count }.into())
    }
}

/// Update merge state based on results and persist or clear as appropriate.
fn finalize(args: &Args, merge_state: &mut state::MergeState, summary: &merger::MergeSummary) -> Result<()> {
    if !args.dry_run {
        for (name, _) in &summary.failures {
            merge_state.mark_failed(name);
        }
        // Failures already marked above

        // Mark all non-failed pairs as completed
        for pair in &merge_state.pending.clone() {
            if !summary.failures.iter().any(|(name, _)| name == pair) {
                merge_state.mark_completed(pair);
            }
        }

        // Clear state if all successful
        if summary.all_success() {
            state::MergeState::clear(&args.source)?;
        } else {
            merge_state.save(&args.source)?;
        }
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
        assert_eq!(
            AppError::Other(anyhow::anyhow!("oops")).exit_code(),
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
