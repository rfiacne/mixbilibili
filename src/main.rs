// src/main.rs
mod cli;
mod ffmpeg;
mod merger;
mod scanner;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use colored::Colorize;

mod exit_codes {
    #[allow(dead_code)]
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);
        std::process::exit(determine_exit_code(&e));
    }
}

fn run() -> Result<()> {
    let mut args = Args::parse();
    args.validate()?;

    let format = args.format;

    if !ffmpeg::ensure_ffmpeg() {
        std::process::exit(exit_codes::FFMPEG_NOT_FOUND);
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
    let summary = merger::execute_merges(scan_result, &args.output, format, args.jobs, args.sdel);

    summary.print_report();

    if summary.all_success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Some merges failed"))
    }
}

fn determine_exit_code(error: &anyhow::Error) -> i32 {
    let err_str = error.to_string();
    if err_str.contains("ffmpeg") || err_str.contains("FFmpeg") {
        exit_codes::FFMPEG_NOT_FOUND
    } else if err_str.contains("merge") || err_str.contains("failed") {
        exit_codes::MERGE_FAILED
    } else {
        exit_codes::GENERAL_ERROR
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_exit_codes_constants() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::FFMPEG_NOT_FOUND, 2);
        assert_eq!(exit_codes::MERGE_FAILED, 3);
    }

    #[test]
    fn test_determine_exit_code_general() {
        let err = anyhow::anyhow!("Some random error");
        assert_eq!(determine_exit_code(&err), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn test_determine_exit_code_ffmpeg() {
        let err = anyhow::anyhow!("ffmpeg not found in PATH");
        assert_eq!(determine_exit_code(&err), exit_codes::FFMPEG_NOT_FOUND);
    }

    #[test]
    fn test_determine_exit_code_merge() {
        let err = anyhow::anyhow!("Some merges failed to complete");
        assert_eq!(determine_exit_code(&err), exit_codes::MERGE_FAILED);
    }
}
