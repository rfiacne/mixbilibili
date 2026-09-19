use crate::cli::{self, Args, OutputFormat};
use crate::error::AppError;
use crate::i18n::{t, tf};
use crate::INTERRUPTED;
use crate::{ffmpeg, merger, progress, scanner, state};
use anyhow::{Context, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Initialize the application: parse args, verify ffmpeg, ensure directories.
fn init() -> Result<(Args, OutputFormat)> {
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

    let scan_result = scanner::scan_directory(&args.source, args.recursive).map_err(|e| {
        // Scanner domain errors (e.g. output-name collision) already carry a
        // user-facing message — propagate as-is. Only wrap raw IO failures
        // with the source path context.
        if e.downcast_ref::<std::io::Error>().is_some() {
            AppError::UnreadableSource {
                path: args.source.display().to_string(),
            }
            .into()
        } else {
            e
        }
    })?;

    // Warn about completed outputs whose size differs from their recorded
    // anchor (possible external modification). Non-blocking: resume proceeds.
    if let Some(ref state) = existing_state {
        for (stem, recorded, actual) in check_modified_outputs(state, &args.output, args.format) {
            let safe_stem = scanner::safe_stem(&stem);
            let output_path =
                args.output
                    .join(format!("{}.{}", safe_stem, args.format.extension()));
            eprintln!(
                "{}",
                tf(
                    "output_modified_warning",
                    &[
                        &output_path.display().to_string(),
                        &format!("{recorded}→{actual}")
                    ]
                )
            );
        }
    }

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

/// Return (stem, recorded_size, actual_size) for completed stems whose output
/// file size no longer matches the recorded anchor. Non-blocking diagnostic.
fn check_modified_outputs(
    state: &state::MergeState,
    output_dir: &std::path::Path,
    format: OutputFormat,
) -> Vec<(String, u64, u64)> {
    let mut modified = Vec::new();
    for stem in &state.completed {
        if let Some(recorded) = state.anchor_for(stem) {
            let output_path = output_dir.join(format!(
                "{}.{}",
                scanner::safe_stem(stem),
                format.extension()
            ));
            if let Ok(m) = std::fs::metadata(&output_path) {
                if m.len() != recorded {
                    modified.push((stem.clone(), recorded, m.len()));
                }
            }
        }
    }
    modified.sort_by(|a, b| a.0.cmp(&b.0));
    modified
}

/// --once mode: merge a single pair identified by stem.
/// No scanning, no state file, no progress bar — just merge and exit.
fn run_once(args: &Args, format: OutputFormat, stem: &str) -> Result<()> {
    // Validate stem before using it
    cli::validate_stem(stem)?;

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

    let result = merger::merge_pair(
        &pair,
        0,
        &args.output,
        format,
        None,
        args.verbose,
        args.retry,
        None,
    );

    if result.success {
        if args.sdel {
            if let Err(e) = merger::delete_source_files(&pair) {
                eprintln!("{} {}", t("warning_prefix").yellow(), e);
            }
        }
        Ok(())
    } else {
        // Clean up partial output file if merge failed
        let output_path = args.output.join(format!(
            "{}.{}",
            scanner::safe_stem(stem),
            format.extension()
        ));
        if output_path.exists() {
            if let Err(e) = std::fs::remove_file(&output_path) {
                eprintln!(
                    "{} Failed to clean up partial output: {}",
                    t("warning_prefix").yellow(),
                    e
                );
            }
        }
        Err(AppError::MergeFailed { count: 1 }.into())
    }
}

/// Merges between incremental state saves to limit progress loss on interrupt.
const STATE_SAVE_INTERVAL: usize = 5;

/// Execute the actual merge operations for all filtered pairs.
fn execute(
    args: &Args,
    ctx: ScanContext,
    merge_state: &mut state::MergeState,
    format: OutputFormat,
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
                None,
            );

            // Update state and periodically save
            let snapshot = {
                // Handle mutex poisoning: recover data even if a worker panicked
                let mut state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
                if !result.was_interrupted {
                    if result.success {
                        state.mark_completed(&pair.stem);
                        // Record the merged output size as a resume-time anchor.
                        let safe_stem = scanner::safe_stem(&pair.stem);
                        let output_path =
                            args.output
                                .join(format!("{}.{}", safe_stem, format.extension()));
                        if let Ok(m) = std::fs::metadata(&output_path) {
                            state.record_anchor(&pair.stem, m.len());
                        }
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
        // Handle mutex poisoning: recover data even if a worker panicked
        let state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
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
                // stem is composite key "parent/stem", safe_stem flattens it for the filename
                let safe_stem = scanner::safe_stem(&pair.stem);
                let output_path = args
                    .output
                    .join(format!("{}.{}", safe_stem, format.extension()));
                if output_path.exists() {
                    if let Err(e) = std::fs::remove_file(&output_path) {
                        eprintln!("{} {}", t("warning_prefix").yellow(), e);
                    } else if args.verbose {
                        eprintln!(
                            "{}",
                            tf("cleanup_partial", &[&output_path.display().to_string()]).yellow()
                        );
                    }
                }
                // NOTE: Do NOT delete source files on interrupt.
                // ffmpeg -c copy doesn't consume source files, so they remain valid.
                // Deleting them would cause data loss. Keep them for --resume.
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

/// Finalize: clear state on full success, save otherwise.
/// Note: execute() already marks completed/failed items in the state via the mutex,
/// so we only need to decide whether to clear or persist the state file.
fn finalize(
    args: &Args,
    merge_state: state::MergeState,
    summary: &merger::MergeSummary,
) -> Result<()> {
    if args.dry_run {
        return Ok(());
    }

    if summary.all_success() {
        state::MergeState::clear(&args.source)?;
    } else {
        merge_state.save(&args.source)?;
    }
    Ok(())
}

/// Main workflow: init → scan → interactive/dry-run → execute → finalize/report.
pub fn run() -> Result<()> {
    // Reset SIGCHLD to SIG_DFL once, before any threads are created.
    // aria2 sets SIGCHLD to SIG_IGN in its on-download-complete hook,
    // which causes waitpid() to fail with ECHILD.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
    }

    // Phase 1: Initialize (parse args, verify ffmpeg)
    let (args, format) = init()?;

    // Set ASCII mode environment variable BEFORE creating any threads.
    // POSIX: setenv is UB in multi-threaded programs.
    if args.ascii {
        std::env::set_var("MIXBILIBILI_ASCII", "1");
    }

    // Set up Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .context(t("failed_set_signal").into_owned())?;

    // --once mode: merge a single pair by stem, skip scanning
    if let Some(ref stem) = args.once {
        return run_once(&args, format, stem);
    }

    // Configure rayon's global thread pool once at startup
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

    // Phase 2: Scan and filter
    let Some((ctx, mut merge_state)) = scan_and_filter(&args)? else {
        return Ok(());
    };

    // Phase 2.5: Interactive preview + confirmation
    if args.interactive {
        use crate::ui::Colors;

        crate::ui::print_preview_pairs(
            &ctx.pairs,
            format.extension(),
            &t("interactive_preview_header"),
            ctx.format_time_estimate().as_deref(),
        );
        if args.sdel {
            println!("{}", Colors::warning(&t("dry_run_sdel_header")));
        }
        println!();

        let prompt = format!(
            "{} {} \u{203a} ({}/{})",
            Colors::info("?"),
            t("prompt_continue"),
            Colors::success(&t("prompt_default_yes")),
            Colors::secondary(&t("prompt_default_no")),
        );
        print!("{} ", prompt);
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !parse_prompt_response(&input, args.sdel) {
            println!("{}", Colors::warning(&t("interactive_cancelled")));
            state::MergeState::clear(&args.source)?;
            return Ok(());
        }
    }

    // Phase 3: Dry-run preview or execute merges
    if args.dry_run {
        if !args.quiet {
            crate::ui::print_preview_pairs(
                &ctx.pairs,
                format.extension(),
                &t("dry_run_header"),
                ctx.format_time_estimate().as_deref(),
            );
            if args.sdel {
                crate::ui::print_deletion_preview(&ctx.pairs, &t("dry_run_sdel_header"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_state_save_interval_constant() {
        // Ensure reasonable batch size for incremental saves
        const _: () = assert!(STATE_SAVE_INTERVAL >= 1 && STATE_SAVE_INTERVAL <= 20);
    }

    #[test]
    fn test_run_once_rejects_path_traversal() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Create dummy files
        File::create(dir.path().join("test.mp4")).unwrap();
        File::create(dir.path().join("test.m4a")).unwrap();

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
            once: Some("../escape".to_string()),
            ascii: false,
        };

        let result = run_once(&args, cli::OutputFormat::Mkv, "../escape");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid") || err.contains("traversal") || err.contains("path separator"),
            "Error should mention invalid stem: {}",
            err
        );
    }

    #[test]
    fn test_run_once_rejects_absolute_path() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

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
            once: Some("/etc/passwd".to_string()),
            ascii: false,
        };

        let result = run_once(&args, cli::OutputFormat::Mkv, "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_once_rejects_backslash_traversal() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

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
            once: Some("..\\escape".to_string()),
            ascii: false,
        };

        let result = run_once(&args, cli::OutputFormat::Mkv, "..\\escape");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_once_rejects_null_byte() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

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
            once: Some("evil\0path".to_string()),
            ascii: false,
        };

        let result = run_once(&args, cli::OutputFormat::Mkv, "evil\0path");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("null") || err.contains("Invalid"),
            "Error should mention null byte or invalid: {}",
            err
        );
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
        let result = run_once(&args, cli::OutputFormat::Mkv, "video");
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
            once: None,
            ascii: false,
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
    fn test_check_modified_outputs_detects_size_change() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        state.completed.insert("video1".to_string());
        state.record_anchor("video1", 100);
        // Actual output is 50 bytes (different from recorded 100)
        std::fs::write(output.join("video1.mkv"), vec![0u8; 50]).unwrap();

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0, "video1");
        assert_eq!(modified[0].1, 100);
        assert_eq!(modified[0].2, 50);
    }

    #[test]
    fn test_check_modified_outputs_ignores_matching_size() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        state.completed.insert("video1".to_string());
        state.record_anchor("video1", 100);
        std::fs::write(output.join("video1.mkv"), vec![0u8; 100]).unwrap();

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert!(modified.is_empty());
    }

    #[test]
    fn test_check_modified_outputs_ignores_stem_without_anchor() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();

        let mut state = state::MergeState::new(dir.path(), &output);
        // completed but no anchor recorded (e.g. migrated v1 state)
        state.completed.insert("video1".to_string());

        let modified = check_modified_outputs(&state, &output, OutputFormat::Mkv);
        assert!(modified.is_empty());
    }
}
