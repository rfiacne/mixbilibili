// src/main.rs
mod cli;
mod ffmpeg;
mod merger;
mod progress;
mod scanner;

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

    let summary = merger::execute_merges(scan_result, &args.output, format, args.jobs, args.sdel, progress);

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
