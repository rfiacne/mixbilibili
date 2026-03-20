// src/main.rs
mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use clap::Parser;
use cli::Args;
use colored::Colorize;

fn main() {
    // Parse and validate arguments
    let mut args = Args::parse();
    if let Err(e) = args.validate() {
        eprintln!("{} {}", "Error:".red(), e);
        std::process::exit(1);
    }

    // Parse format early
    let format = match args.parsed_format() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            std::process::exit(1);
        }
    };

    // Phase 1: Check ffmpeg
    if !ffmpeg::ensure_ffmpeg() {
        std::process::exit(1);
    }

    // Phase 2: Scan directory
    let scan_result = match scanner::scan_directory(&args.source) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            std::process::exit(1);
        }
    };

    // Check if we have anything to do
    if scan_result.pairs.is_empty() {
        println!("{}", "No file pairs to merge".yellow());
        std::process::exit(0);
    }

    // Validate/create output directory
    if !args.output.exists() {
        if let Err(e) = std::fs::create_dir_all(&args.output) {
            eprintln!("{} Failed to create output directory: {}", "Error:".red(), e);
            std::process::exit(1);
        }
    }

    // Check output directory is writable
    if args.output.exists() {
        // Try to write a test file
        let test_file = args.output.join(".mixbilibili_write_test");
        if std::fs::File::create(&test_file).is_err() {
            eprintln!("{} Output directory is not writable: {}", "Error:".red(), args.output.display());
            std::process::exit(1);
        }
        let _ = std::fs::remove_file(&test_file);
    }

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
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}