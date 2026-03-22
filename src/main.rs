// src/main.rs
mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use colored::Colorize;

/// Exit codes
#[allow(dead_code)]
mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);

        // Determine exit code based on error type
        let exit_code = determine_exit_code(&e);
        std::process::exit(exit_code);
    }
}

fn run() -> Result<()> {
    // Parse and validate arguments
    let mut args = Args::parse();
    args.validate()?;

    // Parse format early
    let format = args.parsed_format()?;

    // Phase 1: Check ffmpeg
    if !ffmpeg::ensure_ffmpeg() {
        std::process::exit(exit_codes::FFMPEG_NOT_FOUND);
    }

    // Phase 2: Scan directory
    let scan_result = scanner::scan_directory(&args.source)?;

    // Check if we have anything to do
    if scan_result.pairs.is_empty() {
        println!("{}", "No file pairs to merge".yellow());
        return Ok(());
    }

    // Validate/create output directory
    if !args.output.exists() {
        std::fs::create_dir_all(&args.output)
            .map_err(|e| anyhow::anyhow!("Failed to create output directory: {}", e))?;
    }

    // Check output directory is writable
    check_output_writable(&args.output)?;

    // Phase 3: Execute merges
    println!("Processing {} file pairs...", scan_result.pairs.len());
    let summary = merger::execute_merges(
        scan_result,
        &args.output,
        format,
        args.jobs,
        args.sdel,
    );

    // Phase 4: Print report
    summary.print_report();

    // Exit with appropriate code
    if summary.all_success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Some merges failed"))
    }
}

fn check_output_writable(output: &std::path::Path) -> Result<()> {
    if output.exists() {
        let test_file = output.join(".mixbilibili_write_test");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&test_file)
        {
            Ok(_) => { let _ = std::fs::remove_file(&test_file); }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = std::fs::remove_file(&test_file);
            }
            Err(_) => {
                anyhow::bail!("Output directory is not writable: {}", output.display());
            }
        }
    }
    Ok(())
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
    use tempfile::tempdir;

    #[test]
    fn test_check_output_writable_success() {
        let dir = tempdir().unwrap();
        let result = check_output_writable(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_output_writable_nonexistent_creates_test_file() {
        let dir = tempdir().unwrap();
        // Directory exists, should be writable
        let result = check_output_writable(dir.path());
        assert!(result.is_ok());
        // Test file should be cleaned up
        assert!(!dir.path().join(".mixbilibili_write_test").exists());
    }

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