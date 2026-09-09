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

/// Render the scan's time estimate, or None when the scan produced nothing
/// measurable to estimate from.
fn format_time_estimate(scan: &scanner::ScanResult) -> Option<String> {
    if scan.estimated_duration.is_zero() {
        None
    } else {
        Some(tf(
            "dry_run_time_estimate",
            &[&progress::format_duration(scan.estimated_duration)],
        ))
    }
}

/// Scan source directory and filter pairs based on resume state.
/// Returns the surviving scan result and the resume state to extend.
fn scan_and_filter(args: &Args) -> Result<Option<(scanner::ScanResult, state::MergeState)>> {
    // Check for resume state
    let existing_state = if args.resume {
        state::MergeState::load(&args.source)?
    } else {
        None
    };

    let mut scan_result = scanner::scan_directory(&args.source, args.recursive).map_err(|_| {
        AppError::UnreadableSource {
            path: args.source.display().to_string(),
        }
    })?;

    // Filter out already completed if resuming
    if let Some(ref state) = existing_state {
        scan_result.pairs.retain(|p| !state.is_completed(&p.stem));
    }

    if scan_result.pairs.is_empty() {
        if existing_state.is_some() {
            println!("{}", t("all_merged").green());
        } else {
            println!("{}", t("no_pairs").yellow());
        }
        return Ok(None);
    }

    let merge_state =
        existing_state.unwrap_or_else(|| state::MergeState::new(&args.source, &args.output));

    Ok(Some((scan_result, merge_state)))
}

/// Print preview of pairs to be processed, including header and time estimate.
fn print_preview_pairs(scan: &scanner::ScanResult, format: &cli::OutputFormat, header: &str) {
    println!("{}", header.cyan().bold());
    for pair in &scan.pairs {
        println!(
            "  {} + {} -> {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    if let Some(msg) = format_time_estimate(scan) {
        println!("{}", msg);
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", t("error_prefix").red(), translate_error(&e));
        std::process::exit(get_exit_code(&e));
    }
}

fn run() -> Result<()> {
    // Cancellation signal; the Ctrl+C handler holds its own clone.
    let cancel = merger::Cancel::new();

    // Set up Ctrl+C handler for graceful shutdown
    let cancel_on_signal = cancel.clone();
    ctrlc::set_handler(move || {
        cancel_on_signal.cancel();
    })
    .context(t("failed_set_signal").into_owned())?;

    // Phase 1: Initialize
    let (args, format) = init()?;
    let opts = merger::MergeOptions {
        output_dir: &args.output,
        format,
        verbose: args.verbose,
        retry: args.retry,
    };

    // --once mode: merge a single pair by stem. It honours --dry-run like
    // every other mode — a preview never merges or deletes.
    if let cli::Mode::Once(ref stem) = args.mode {
        return run_once(&args, &opts, stem, &cancel);
    }

    // Configure rayon's global thread pool once at startup
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build_global()
        .ok();

    // Phase 2: Scan and filter
    let Some((scan, merge_state)) = scan_and_filter(&args)? else {
        return Ok(());
    };

    // Phase 2.5: Interactive preview + confirmation
    if args.interactive {
        print_preview_pairs(&scan, &format, &t("interactive_preview_header"));
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
            // Cancel leaves an existing resume state untouched: nothing ran.
            println!("{}", t("interactive_cancelled").yellow());
            return Ok(());
        }
    }

    // Phase 3: Dry-run preview or execute merges
    if args.dry_run {
        if !args.quiet {
            print_preview_pairs(&scan, &format, &t("dry_run_header"));
            if args.sdel {
                println!("\n{}", t("dry_run_sdel_header").yellow().bold());
                for pair in &scan.pairs {
                    println!(
                        "  {} (video)\n  {} (audio)",
                        pair.video.display(),
                        pair.audio.display()
                    );
                }
            }
            println!(
                "\n{}",
                tf("dry_run_summary", &[&scan.pairs.len().to_string()])
            );
            println!("{}", t("dry_run_complete").cyan());
        }
        return Ok(());
    }

    // Phase 3: merge, tracking outcomes for resume
    let tracker = state::StateTracker::new(merge_state, &args.source, STATE_SAVE_INTERVAL);
    let summary = execute(&args, &scan, &opts, &cancel, &tracker)?;

    // Phase 4: Update state and report. On interrupt the flushed state is
    // exactly what --resume needs, so leave it alone.
    if !cancel.is_cancelled() {
        finalize(&args, &summary)?;
    }

    render_report(&summary, args.quiet);

    if cancel.is_cancelled() {
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

/// --once mode: merge a single pair identified by stem.
/// No scanning, no state file, no progress bar — just merge and exit.
/// --dry-run turns it into a pure preview: same checks, no side effects.
fn run_once(
    args: &Args,
    opts: &merger::MergeOptions,
    stem: &str,
    cancel: &merger::Cancel,
) -> Result<()> {
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
        println!("{}", t("dry_run_header").cyan().bold());
        println!(
            "  {} + {} -> {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            opts.format.extension()
        );
        if args.sdel {
            println!("{}", t("dry_run_sdel_header").yellow().bold());
        }
        println!("{}", tf("dry_run_summary", &["1"]));
        println!("{}", t("dry_run_complete").cyan());
        return Ok(());
    }

    let result = merger::merge_pair(&pair, 0, opts, cancel);
    merger::announce(&result, None, args.verbose);

    if result.success {
        if args.sdel {
            if let Err(e) = merger::delete_source_files(&pair) {
                eprintln!("{} {}", t("warning_prefix").yellow(), e);
            }
        }
        Ok(())
    } else {
        Err(AppError::MergeFailed { count: 1 }.into())
    }
}

/// Merges between incremental state saves to limit progress loss on interrupt.
const STATE_SAVE_INTERVAL: usize = 5;

/// Run the merge batch: announce it, build the progress renderer, and hand
/// everything to merger::run_batch, which owns the parallel execution.
fn execute(
    args: &Args,
    scan: &scanner::ScanResult,
    opts: &merger::MergeOptions,
    cancel: &merger::Cancel,
    tracker: &state::StateTracker,
) -> Result<merger::MergeSummary> {
    if !args.quiet {
        if let Some(msg) = format_time_estimate(scan) {
            println!("{}", msg);
        } else {
            println!("{}", tf("processing", &[&scan.pairs.len().to_string()]));
        }
    }

    let progress = if args.progress && !args.quiet {
        Some(progress::MergeProgress::new(scan.pairs.len()))
    } else {
        None
    };

    Ok(merger::run_batch(
        &scan.pairs,
        &scan.stats,
        opts,
        args.sdel,
        progress.as_ref(),
        tracker,
        cancel,
    ))
}

/// Print the merge report. Display lives here, beside the CLI — the merger
/// module only produces facts.
fn render_report(summary: &merger::MergeSummary, quiet: bool) {
    use merger::MergeSummary;

    fn line(text: String) {
        println!("{text}");
    }

    if quiet {
        let total = summary.success_count + summary.failed_count;
        let key = if summary.failed_count > 0 {
            "merged_summary_fail"
        } else {
            "merged_summary_ok"
        };
        line(tf(
            key,
            &[
                &summary.success_count.to_string(),
                &total.to_string(),
                &summary.failed_count.to_string(),
            ],
        ));
        return;
    }

    let MergeSummary {
        success_count,
        failed_count,
        skipped_count,
        orphaned_count,
        failures,
        deletion_failures,
        total_duration,
        merge_count: _,
    } = summary;

    line(t("separator").bright_black().to_string());
    line(t("merge_report").cyan().bold().to_string());
    line(t("separator").bright_black().to_string());

    let success_str = format!(
        "{} {}",
        t("checkmark").green(),
        tf("succeeded_fmt", &[&success_count.to_string()])
    );
    let fail_str = if *failed_count > 0 {
        format!(
            "{} {}",
            t("cross").red(),
            tf("failed_fmt", &[&failed_count.to_string()])
        )
        .red()
        .to_string()
    } else {
        format!(
            "{} {}",
            t("cross"),
            tf("failed_fmt", &[&failed_count.to_string()])
        )
    };
    line(format!("  {success_str}    {fail_str}"));

    if *skipped_count > 0 {
        line(format!(
            "  {} {}",
            tf("skipped_fmt", &[&skipped_count.to_string()]),
            "(aria2)".bright_black()
        ));
    }
    if *orphaned_count > 0 {
        line(format!(
            "  {} {}",
            tf("orphaned_fmt", &[&orphaned_count.to_string()]),
            "(orphan)".bright_black()
        ));
    }

    if summary.merge_count > 0 {
        line(format!(
            "  {}: {}",
            t("duration").bright_black(),
            progress::format_duration(*total_duration)
        ));
        if let Some(avg) = summary.avg_duration() {
            line(format!(
                "  {}: {}",
                t("avg").bright_black(),
                progress::format_duration(avg)
            ));
        }
        if let Some(tp) = summary.throughput() {
            line(format!(
                "  {}: {:.2} pairs/sec",
                t("throughput").bright_black(),
                tp
            ));
        }
    }

    if *deletion_failures > 0 {
        line(format!(
            "  {} {}",
            tf("deletion_failures", &[&deletion_failures.to_string()]),
            "(warn)".yellow()
        ));
    }

    line(t("separator").bright_black().to_string());

    if !failures.is_empty() {
        line(format!("\n{}", t("failed_files").red().bold()));
        for (name, error) in failures {
            line(format!("  {} {}: {}", t("cross").red(), name, error));
        }
        line(String::new());
    }
}

/// Finalize: drop the resume state once everything merged; a partial run
/// leaves the state execute() already flushed to disk.
fn finalize(args: &Args, summary: &merger::MergeSummary) -> Result<()> {
    if summary.all_success() {
        state::MergeState::clear(&args.source)?;
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
    fn test_cancel_token_default() {
        let cancel = merger::Cancel::new();
        assert!(!cancel.is_cancelled());
    }

    #[test]
    fn test_cancel_token_is_shared_between_clones() {
        let cancel = merger::Cancel::new();
        let handler_clone = cancel.clone();
        handler_clone.cancel();
        assert!(cancel.is_cancelled());
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
    fn test_finalize_keeps_state_on_partial_failure() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&output).unwrap();

        // Simulate execute() having flushed state for video1/video2.
        let mut state = state::MergeState::new(&source, &output);
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
            mode: cli::Mode::Batch,
        };

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

        finalize(&args, &summary).unwrap();

        // Partial failure: state file survives with execute()'s bookkeeping.
        let loaded = state::MergeState::load(&source)
            .unwrap()
            .expect("state should be saved");
        assert!(loaded.is_completed("video1"));
        assert!(loaded.failed.contains("video2"));
    }

    #[test]
    fn test_finalize_clears_state_on_full_success() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&output).unwrap();

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
            mode: cli::Mode::Batch,
        };

        state::MergeState::new(&source, &output)
            .save(&source)
            .unwrap();

        let summary = merger::MergeSummary {
            success_count: 2,
            ..Default::default()
        };
        finalize(&args, &summary).unwrap();

        assert!(state::MergeState::load(&source).unwrap().is_none());
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
