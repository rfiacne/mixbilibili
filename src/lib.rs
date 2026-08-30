//! Batch-merge Bilibili video (`.mp4`) and audio (`.m4a`) pairs with ffmpeg.
//!
//! [`run`] is the whole interface: it reads the arguments, merges, and reports.
//! `main.rs` is a thin adapter over it, and tests drive the same interface —
//! either through the library directly or through the spawned binary.

pub mod cli;
pub mod ffmpeg;
pub mod i18n;
pub mod interrupt;
pub mod merger;
pub mod pipeline;
pub mod progress;
pub mod scanner;
pub mod state;
pub mod ui;

use crate::cli::Args;
use crate::i18n::{t, tf};
use crate::interrupt::Interrupt;
use crate::ui::Colors;
use anyhow::{Context, Result};
use colored::Colorize;
use std::time::Duration;
use thiserror::Error;

pub mod exit_codes {
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

/// Application-level errors with typed variants for exit code mapping.
///
/// Using thiserror eliminates the fragile string-based error classification
/// that was previously done via error_category().
/// Errors the exit code depends on.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("ffmpeg not found")]
    FfmpegNotFound,

    #[error("{count} merge(s) failed")]
    MergeFailed { count: usize },

    #[error("source directory is not readable: {path}")]
    UnreadableSource { path: String },
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::FfmpegNotFound => exit_codes::FFMPEG_NOT_FOUND,
            AppError::MergeFailed { .. } => exit_codes::MERGE_FAILED,
            AppError::UnreadableSource { .. } => exit_codes::GENERAL_ERROR,
        }
    }

    /// User-facing text, in the running language.
    fn message(&self) -> String {
        match self {
            AppError::FfmpegNotFound => t("ffmpeg_not_found").to_string(),
            AppError::MergeFailed { count } => tf("merge_failed", &[&count.to_string()]),
            AppError::UnreadableSource { path } => tf("unreadable_source", &[path]),
        }
    }
}

/// A finished run that went wrong: what to show the user, what to exit with.
///
/// Pairing the two here means a new error cannot get a message without a code
/// or the reverse.
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub exit_code: i32,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        match err.downcast_ref::<AppError>() {
            Some(app) => Self {
                message: app.message(),
                exit_code: app.exit_code(),
            },
            None => Self {
                message: err.to_string(),
                exit_code: exit_codes::GENERAL_ERROR,
            },
        }
    }
}

/// Initialize the application: parse args, verify ffmpeg, ensure directories.
fn init() -> Result<Args> {
    let matches = cli::build_cli().get_matches();
    let mut args = cli::parse_args(&matches);
    args.validate()?;

    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }

    if !args.output.exists() {
        std::fs::create_dir_all(&args.output).context(t("failed_create_output").into_owned())?;
    }

    Ok(args)
}

/// Rough wall-clock estimate, or None when the batch is too small for the
/// number to mean anything.
fn estimate(pairs: &[scanner::FilePair]) -> Option<Duration> {
    // ponytail: one stat() per pair, sequentially. Re-parallelise if scans
    // routinely reach tens of thousands of pairs.
    let total: Duration = pairs
        .iter()
        .map(|pair| ffmpeg::estimate_merge_duration(&pair.video))
        .sum();
    (total >= Duration::from_secs(1)).then_some(total)
}

fn estimate_line(pairs: &[scanner::FilePair]) -> Option<String> {
    let seconds = estimate(pairs)?;
    Some(tf(
        "dry_run_time_estimate",
        &[&progress::format_duration(seconds)],
    ))
}

/// Scan source directory and filter pairs based on resume state.
/// Returns the scan context and initialized merge state.
fn scan_and_filter(
    args: &Args,
) -> Result<
    Option<(
        Vec<scanner::FilePair>,
        scanner::ScanStats,
        pipeline::StateSink,
    )>,
> {
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

    Ok(Some((
        pairs_to_process,
        scan_result.stats,
        pipeline::StateSink::new(merge_state, args.source.clone()),
    )))
}

/// The last component of a path, for the preview's two-column layout.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Compute display width of a path's file_name component.
/// Uses unicode-width for correct CJK character width calculation.
fn file_name_width(p: &std::path::Path) -> usize {
    use unicode_width::UnicodeWidthStr;
    p.file_name()
        .map(|n| UnicodeWidthStr::width(n.to_string_lossy().as_ref()))
        .unwrap_or_else(|| p.display().to_string().len())
}

/// Print preview of pairs to be processed, including header and time estimate.
fn print_preview_pairs(rows: &[pipeline::PreviewRow], header: &str) {
    use crate::ui::{Colors, Symbols};

    let symbols = Symbols::new(crate::ui::supports_unicode());

    // Compute max widths for two-column alignment (video | audio)
    let max_video_width = rows
        .iter()
        .map(|r| file_name_width(&r.video))
        .max()
        .unwrap_or(0);
    let max_audio_width = rows
        .iter()
        .map(|r| file_name_width(&r.audio))
        .max()
        .unwrap_or(0);

    // Compute inner content width for border
    // Format: " N. video -> audio " => idx_width + 2 + max_video + 1 + arrow_len + 1 + max_audio + 1
    let idx_width = rows.len().to_string().len();
    let arrow_len = symbols.arrow.chars().count();
    let inner_width = idx_width + 2 + max_video_width + 1 + arrow_len + 1 + max_audio_width;
    let border = symbols.horizontal_line.repeat(inner_width);

    // Header with box-drawing top
    println!(
        "{} {} {}",
        Colors::secondary(symbols.top_left),
        Colors::info(header),
        Colors::secondary(&border)
    );

    // Rows
    for (idx, row) in rows.iter().enumerate() {
        let video_name = file_name(&row.video);
        let audio_name = file_name(&row.audio);

        // Use unicode-width for correct CJK character padding
        let video_display_width = unicode_width::UnicodeWidthStr::width(video_name.as_str());
        let audio_display_width = unicode_width::UnicodeWidthStr::width(audio_name.as_str());
        let video_pad = " ".repeat(max_video_width.saturating_sub(video_display_width));
        let audio_pad = " ".repeat(max_audio_width.saturating_sub(audio_display_width));

        let num = Colors::secondary(&format!("{:>width$}.", idx + 1, width = idx_width));
        let arrow = Colors::info(symbols.arrow);

        // The real output name, flattened — the preview must not promise a
        // path the run never writes.
        println!(
            "{} {} {}{} {} {}{} {}",
            Colors::secondary(symbols.vertical_line),
            num,
            video_name,
            video_pad,
            arrow,
            audio_name,
            audio_pad,
            Colors::info(&row.output)
        );
    }

    // Bottom border
    println!(
        "{}{}",
        Colors::secondary(symbols.bottom_left),
        Colors::secondary(&border)
    );
}

/// Parse arguments, merge, report. Errors come back as [`Failure`].
pub fn run() -> Result<(), Failure> {
    run_inner().map_err(Failure::from)
}

fn run_inner() -> Result<()> {
    let interrupt = Interrupt::new();

    // Reset SIGCHLD to SIG_DFL once, before any threads are created.
    // aria2 sets SIGCHLD to SIG_IGN in its on-download-complete hook,
    // which causes waitpid() to fail with ECHILD.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
    }

    // Phase 1: Initialize (parse args, verify ffmpeg)
    let args = init()?;
    let runner = ffmpeg::FfmpegRunner::default();

    // Set ASCII mode environment variable BEFORE creating any threads.
    // POSIX: setenv is UB in multi-threaded programs.
    if args.ascii {
        std::env::set_var("MIXBILIBILI_ASCII", "1");
    }

    // Set up Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(interrupt.ctrlc_handler()).context(t("failed_set_signal").into_owned())?;

    match args.once.as_deref() {
        Some(stem) => run_once(&args, stem, &interrupt, &runner),
        None => run_batch(&args, &interrupt, &runner),
    }
}

fn run_batch(args: &Args, interrupt: &Interrupt, runner: &dyn ffmpeg::MergeRunner) -> Result<()> {
    let Some((pairs, stats, sink)) = scan_and_filter(args)? else {
        return Ok(());
    };
    if args.interactive && !confirm(args, &pairs)? {
        println!("{}", Colors::warning(&t("interactive_cancelled")));
        state::MergeState::clear(&args.source)?;
        return Ok(());
    }

    if args.dry_run {
        return print_dry_run(args, &pairs);
    }

    // Size the shared pool to --jobs; only the batch path needs it.
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

    if !args.quiet {
        let count = pairs.len().to_string();
        let header = match estimate(&pairs) {
            Some(seconds) => tf(
                "starting_merges",
                &[&count, &progress::format_duration(seconds)],
            ),
            None => tf("processing", &[&count]),
        };
        println!("{header}");
    }

    let bar = if args.progress && !args.quiet {
        Some(progress::MergeProgress::new(pairs.len()))
    } else {
        None
    };

    let mut opts = pipeline::BatchOpts::from_args(args, interrupt);
    opts.progress = bar.as_ref();
    opts.state = Some(&sink);

    let summary = pipeline::process(&pairs, &stats, &opts, runner)?;

    summary.print_report(args.quiet);

    if interrupt.is_triggered() {
        println!("{}", t("interrupted_cleanup").yellow());
        println!("{}", t("interrupted_resume_hint").cyan());
        return Ok(());
    }

    if let Some(err) = merge_error(&summary) {
        return Err(err);
    }
    Ok(())
}

/// Show the plan and read a y/N. Returns false if the user declined.
fn confirm(args: &Args, pairs: &[scanner::FilePair]) -> Result<bool> {
    let rows = pipeline::preview_rows(pairs, args.format);
    print_preview_pairs(&rows, &t("interactive_preview_header"));
    if args.sdel {
        println!("{}", Colors::warning(&t("dry_run_sdel_header")));
    }
    if let Some(msg) = estimate_line(pairs) {
        println!("{}", Colors::secondary(&msg));
    }
    println!();

    let prompt = format!(
        "{} {} \u{203a} ({}/{})",
        Colors::info("?"),
        t("prompt_continue"),
        Colors::success(&t("prompt_default_yes")),
        Colors::secondary(&t("prompt_default_no")),
    );
    print!("{prompt} ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(parse_prompt_response(&input, args.sdel))
}

fn print_dry_run(args: &Args, pairs: &[scanner::FilePair]) -> Result<()> {
    if args.quiet {
        return Ok(());
    }
    let rows = pipeline::preview_rows(pairs, args.format);

    print_preview_pairs(&rows, &t("dry_run_header"));
    if args.sdel {
        println!("\n{}", t("dry_run_sdel_header").yellow().bold());
        for row in rows {
            println!(
                "  {} (video)\n  {} (audio)",
                row.video.display(),
                row.audio.display()
            );
        }
    }
    println!("\n{}", tf("dry_run_summary", &[&pairs.len().to_string()]));
    if let Some(msg) = estimate_line(pairs) {
        println!("{}", Colors::secondary(&msg));
    }
    println!("{}", t("dry_run_complete").cyan());
    Ok(())
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

/// --once mode: merge a single pair identified by stem.
/// No scanning, no state file, no progress bar — just merge and exit.
fn run_once(
    args: &Args,
    stem: &str,
    interrupt: &Interrupt,
    runner: &dyn ffmpeg::MergeRunner,
) -> Result<()> {
    let pair = scanner::FilePair::sources_in(&args.source, stem)?;
    let opts = pipeline::BatchOpts::from_args(args, interrupt);

    let summary = pipeline::process(
        std::slice::from_ref(&pair),
        &scanner::ScanStats::default(),
        &opts,
        runner,
    )?;

    if let Some(err) = merge_error(&summary) {
        return Err(err);
    }
    Ok(())
}

/// A batch that did not finish cleanly exits with MERGE_FAILED.
fn merge_error(summary: &merger::MergeSummary) -> Option<anyhow::Error> {
    (!summary.all_success()).then(|| {
        AppError::MergeFailed {
            count: summary.failed_count(),
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_estimate_worth_acting_on_is_shown() {
        // Empty files estimate at zero, which used to render as an empty
        // "Estimated time:" line.
        assert!(estimate_line(&[]).is_none());
    }

    #[test]
    fn test_exit_codes_constants() {
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::FFMPEG_NOT_FOUND, 2);
        assert_eq!(exit_codes::MERGE_FAILED, 3);
    }

    #[test]
    fn test_state_save_interval_constant() {
        // Ensure reasonable batch size for incremental saves
        const _: () =
            assert!(pipeline::STATE_SAVE_INTERVAL >= 1 && pipeline::STATE_SAVE_INTERVAL <= 20);
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
    fn app_errors_survive_the_anyhow_round_trip() {
        let app_err = AppError::FfmpegNotFound;
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(anyhow_err.to_string(), "ffmpeg not found");
    }

    #[test]
    fn app_errors_carry_their_own_exit_code() {
        let failure = Failure::from(anyhow::Error::from(AppError::FfmpegNotFound));
        assert_eq!(failure.exit_code, exit_codes::FFMPEG_NOT_FOUND);
        assert_eq!(failure.message, t("ffmpeg_not_found"));
    }

    #[test]
    fn merge_failures_map_to_the_merge_exit_code() {
        let app_err = AppError::MergeFailed { count: 3 };
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(
            Failure::from(anyhow_err).exit_code,
            exit_codes::MERGE_FAILED
        );
    }

    #[test]
    fn unrelated_errors_keep_their_message_and_the_general_code() {
        let failure = Failure::from(anyhow::anyhow!("Something went wrong"));
        assert_eq!(failure.exit_code, exit_codes::GENERAL_ERROR);
        assert_eq!(failure.message, "Something went wrong");
    }

    #[test]
    fn test_run_once_accepts_valid_stem() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Create valid pair
        File::create(dir.path().join("video.mp4")).unwrap();
        File::create(dir.path().join("video.m4a")).unwrap();

        let args = cli::Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
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
            once: Some("video".to_string()),
            ascii: false,
        };

        // Should not error on validation (may error on ffmpeg, but that's OK)
        let result = run_once(
            &args,
            "video",
            &Interrupt::new(),
            &ffmpeg::FfmpegRunner::default(),
        );
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
    fn test_file_name_width() {
        use std::path::PathBuf;
        // file_name component only
        assert_eq!(file_name_width(&PathBuf::from("short.mp4")), 9);
        assert_eq!(
            file_name_width(&PathBuf::from("very_long_filename.mp4")),
            22
        );
        // path prefix stripped
        assert_eq!(file_name_width(&PathBuf::from("/some/dir/file.mp4")), 8);
    }

    #[test]
    fn test_file_name_width_cjk() {
        use std::path::PathBuf;
        // CJK characters are 2 columns wide each
        // "第1集.mp4" = 2+1+2+1+1+1+1 = 9 display columns
        assert_eq!(file_name_width(&PathBuf::from("第1集.mp4")), 9);
        // "正片.mp4" = 2+2+1+1+1+1 = 8 display columns
        assert_eq!(file_name_width(&PathBuf::from("正片.mp4")), 8);
    }

    #[test]
    fn test_preview_list_alignment() {
        let pairs = vec![
            crate::scanner::FilePair {
                video: std::path::PathBuf::from("short_v.mp4"),
                audio: std::path::PathBuf::from("short_a.m4a"),
                stem: "short".to_string(),
            },
            crate::scanner::FilePair {
                video: std::path::PathBuf::from("very_long_filename_v.mp4"),
                audio: std::path::PathBuf::from("very_long_filename_a.m4a"),
                stem: "very_long_filename".to_string(),
            },
        ];

        let max_video_width = pairs
            .iter()
            .map(|p| file_name_width(&p.video))
            .max()
            .unwrap_or(0);
        let max_audio_width = pairs
            .iter()
            .map(|p| file_name_width(&p.audio))
            .max()
            .unwrap_or(0);

        assert_eq!(max_video_width, 24); // very_long_filename_v.mp4
        assert_eq!(max_audio_width, 24); // very_long_filename_a.m4a

        // Verify padding produces equal-width columns
        for pair in &pairs {
            let vw = file_name_width(&pair.video);
            let aw = file_name_width(&pair.audio);
            let padded_video_len = vw + (max_video_width - vw);
            let padded_audio_len = aw + (max_audio_width - aw);
            assert_eq!(padded_video_len, max_video_width);
            assert_eq!(padded_audio_len, max_audio_width);
        }
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
}
