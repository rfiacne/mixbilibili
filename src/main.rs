// src/main.rs
mod cli;
mod ffmpeg;
mod merger;
mod progress;
mod scanner;
mod state;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use colored::Colorize;

mod exit_codes {
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

#[derive(Debug)]
enum AppError {
    FfmpegNotFound,
    MergeFailed,
    Other(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::FfmpegNotFound => write!(f, "ffmpeg not found"),
            AppError::MergeFailed => write!(f, "Some merges failed"),
            AppError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<AppError> for anyhow::Error {
    fn from(err: AppError) -> Self {
        anyhow::Error::msg(err.to_string())
    }
}

fn error_category(error: &anyhow::Error) -> AppError {
    let err_str = error.to_string();
    if err_str.contains("ffmpeg") || err_str.contains("FFmpeg") {
        AppError::FfmpegNotFound
    } else if err_str.contains("failed") || err_str.contains("Failed") {
        AppError::MergeFailed
    } else {
        AppError::Other(err_str)
    }
}

fn exit_code_for_error(error: &AppError) -> i32 {
    match error {
        AppError::FfmpegNotFound => exit_codes::FFMPEG_NOT_FOUND,
        AppError::MergeFailed => exit_codes::MERGE_FAILED,
        AppError::Other(_) => exit_codes::GENERAL_ERROR,
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);
        let category = error_category(&e);
        std::process::exit(exit_code_for_error(&category));
    }
}

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
        return Ok(());
    }

    if !args.output.exists() {
        std::fs::create_dir_all(&args.output)
            .map_err(|e| anyhow::anyhow!("Failed to create output directory: {}", e))?;
    }

    // Initialize state for tracking
    let mut merge_state = existing_state
        .unwrap_or_else(|| state::MergeState::new(&args.source, &args.output, &format.to_string()));

    // Add pending items
    for pair in &pairs_to_process {
        merge_state.add_pending(&pair.stem);
    }

    // Save state before starting (in case of interruption)
    if !args.dry_run {
        merge_state.save(&args.source)?;
    }

    println!("Processing {} file pairs...", pairs_to_process.len());

    let progress = if args.progress {
        Some(progress::MergeProgress::new(pairs_to_process.len()))
    } else {
        None
    };

    // Create a modified ScanResult with filtered pairs
    let filtered_scan_result = scanner::ScanResult {
        pairs: pairs_to_process,
        stats: scan_result.stats.clone(),
        skipped_names: scan_result.skipped_names.clone(),
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

    // Update state based on results
    if !args.dry_run {
        for result in &summary.failures {
            merge_state.mark_failed(&result.0);
        }
        // Mark completed based on success count
        for pair in &scan_result.pairs {
            if summary.failures.iter().any(|(name, _)| name == &pair.stem) {
                // Already marked as failed above
            } else if summary.success_count > 0 {
                merge_state.mark_completed(&pair.stem);
            }
        }

        // Clear state if all successful
        if summary.all_success() {
            state::MergeState::clear(&args.source)?;
        } else {
            merge_state.save(&args.source)?;
        }
    }

    if args.dry_run {
        println!("{}", "Dry-run complete. No files were modified.".cyan());
    }

    summary.print_report();

    if summary.all_success() {
        Ok(())
    } else {
        Err(AppError::MergeFailed.into())
    }
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
    fn test_error_category_general() {
        let err = anyhow::anyhow!("Some random error");
        let category = error_category(&err);
        assert!(matches!(category, AppError::Other(_)));
        assert_eq!(exit_code_for_error(&category), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn test_error_category_ffmpeg() {
        let err = anyhow::anyhow!("ffmpeg not found in PATH");
        let category = error_category(&err);
        assert!(matches!(category, AppError::FfmpegNotFound));
        assert_eq!(exit_code_for_error(&category), exit_codes::FFMPEG_NOT_FOUND);
    }

    #[test]
    fn test_error_category_merge() {
        let err = anyhow::anyhow!("Some merges failed to complete");
        let category = error_category(&err);
        assert!(matches!(category, AppError::MergeFailed));
        assert_eq!(exit_code_for_error(&category), exit_codes::MERGE_FAILED);
    }

    #[test]
    fn test_app_error_display() {
        assert_eq!(AppError::FfmpegNotFound.to_string(), "ffmpeg not found");
        assert_eq!(AppError::MergeFailed.to_string(), "Some merges failed");
        assert_eq!(AppError::Other("test".to_string()).to_string(), "test");
    }
}
