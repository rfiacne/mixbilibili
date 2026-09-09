use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::i18n::{t, tf};
use crate::progress::MergeProgress;
use crate::scanner::{FilePair, ScanStats};
use crate::state::StateTracker;
use anyhow::Result;
use colored::Colorize;
use rayon::prelude::*;
use std::io;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Cooperative cancellation signal. The Ctrl+C handler in main holds a clone
/// and flips it; merge workers poll it. Tests flip it directly — no global
/// state, so parallel tests can each own a token.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// How a timeout/cancel-aware child wait ended.
#[derive(Debug)]
enum Outcome {
    Done(ExitStatus),
    TimedOut,
    Cancelled,
}

/// Timeout- and cancel-aware `Child::wait`: kills nothing itself but reports
/// the deadline (`TimedOut`) or the cancel signal (`Cancelled`) so the caller
/// can reap the child.
fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
    poll_interval: Duration,
    cancel: &Cancel,
) -> io::Result<Outcome> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Outcome::Done(status)),
            Ok(None) => {
                if cancel.is_cancelled() {
                    return Ok(Outcome::Cancelled);
                }
                if start.elapsed() >= timeout {
                    return Ok(Outcome::TimedOut);
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Output path for a pair — the single place that maps stem+format to a file.
pub(crate) fn output_path(pair: &FilePair, opts: &MergeOptions) -> std::path::PathBuf {
    opts.output_dir
        .join(format!("{}.{}", pair.stem, opts.format.extension()))
}

#[derive(Debug)]
pub struct MergeResult {
    pub pair_index: usize,
    pub pair_name: String,
    pub success: bool,
    pub error: Option<String>,
    /// Duration of the ffmpeg merge operation.
    pub duration: Duration,
    /// True when the merge never ran because the batch was cancelled.
    pub was_interrupted: bool,
    /// Retries used before this outcome (0 = decided on the first attempt).
    pub retries: usize,
}

/// Everything the merge module needs beyond the pair itself: the whole input
/// contract of `merge_pair` / `run_batch` besides pairs, tracker, cancel,
/// and progress.
#[derive(Debug, Clone, Copy)]
pub struct MergeOptions<'a> {
    pub output_dir: &'a Path,
    pub format: OutputFormat,
    pub verbose: bool,
    pub retry: usize,
}

#[derive(Debug, Default)]
pub struct MergeSummary {
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub orphaned_count: usize,
    pub failures: Vec<(String, String)>,
    pub deletion_failures: usize,
    pub total_duration: Duration,
    pub merge_count: usize,
}

impl MergeSummary {
    /// Average duration per merge operation.
    pub fn avg_duration(&self) -> Option<Duration> {
        if self.merge_count == 0 {
            return None;
        }
        Some(self.total_duration / self.merge_count as u32)
    }

    /// Files processed per second.
    pub fn throughput(&self) -> Option<f64> {
        if self.total_duration.is_zero() {
            return None;
        }
        Some(self.merge_count as f64 / self.total_duration.as_secs_f64())
    }

    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }
}

/// Merge one pair. Pure fact producer: it never prints, never touches
/// progress — the caller displays the returned result.
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    opts: &MergeOptions,
    cancel: &Cancel,
) -> MergeResult {
    if cancel.is_cancelled() {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
            retries: 0,
        };
    }

    let output_path = output_path(pair, opts);

    // Recursive scans key pairs by relative path, so the output may name a
    // subdir the user hasn't created yet. ffmpeg won't mkdir for us.
    if let Some(parent) = output_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return failure(pair, pair_index, e.to_string(), Duration::ZERO, 0);
        }
    }

    let start = std::time::Instant::now();

    for attempt in 0..=opts.retry {
        if attempt > 0 {
            if cancel.is_cancelled() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }

        let mut cmd =
            ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, opts.format);
        match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT, cancel) {
            Ok(status) if status.success() => {
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    success: true,
                    error: None,
                    duration: start.elapsed(),
                    was_interrupted: false,
                    retries: attempt,
                };
            }
            Ok(_) if attempt < opts.retry => continue,
            Ok(status) => {
                let error_msg = tf(
                    "merge_failed_exit",
                    &[
                        &status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        &opts.retry.to_string(),
                    ],
                );
                return failure(pair, pair_index, error_msg, start.elapsed(), attempt);
            }
            // Cancelled mid-merge: the pair never got a verdict, so it joins
            // the interrupted path — unrecorded, partial output cleaned up.
            Err(RunError::Cancelled) => {
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    success: false,
                    error: Some("Skipped (interrupted)".to_string()),
                    duration: start.elapsed(),
                    was_interrupted: true,
                    retries: attempt,
                };
            }
            Err(RunError::Timeout) if attempt < opts.retry => continue,
            Err(RunError::Timeout) => {
                return failure(
                    pair,
                    pair_index,
                    t("timed_out").into_owned(),
                    start.elapsed(),
                    attempt,
                );
            }
            Err(RunError::Spawn(_)) if attempt < opts.retry => continue,
            Err(RunError::Spawn(e)) => {
                let error_msg = tf(
                    "merge_failed_io",
                    &[&e.to_string(), &opts.retry.to_string()],
                );
                return failure(pair, pair_index, error_msg, start.elapsed(), attempt);
            }
        }
    }

    // Cancel broke out of the retry loop.
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some("Skipped (interrupted)".to_string()),
        duration: start.elapsed(),
        was_interrupted: true,
        retries: 0,
    }
}

fn failure(
    pair: &FilePair,
    pair_index: usize,
    error_msg: String,
    duration: Duration,
    retries: usize,
) -> MergeResult {
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(error_msg),
        duration,
        was_interrupted: false,
        retries,
    }
}

/// Display for one finished result — lives in the orchestration layer so the
/// merge core stays silent.
pub fn announce(result: &MergeResult, progress: Option<&MergeProgress>, verbose: bool) {
    if result.was_interrupted {
        return;
    }
    match progress {
        Some(p) => p.record(
            &result.pair_name,
            result.success,
            result.duration,
            result.error.as_deref(),
            (result.retries > 0).then_some(result.retries),
        ),
        None => {
            if verbose && result.retries > 0 {
                println!(
                    "{} {}",
                    "↻".yellow(),
                    tf(
                        "verbose_retry",
                        &[&result.pair_name, &result.retries.to_string()]
                    )
                );
            }
            if result.success {
                println!("{} {}", t("checkmark").green(), result.pair_name);
            } else {
                println!(
                    "{} {}: {}",
                    t("cross").red(),
                    result.pair_name,
                    result.error.as_deref().unwrap_or("")
                );
            }
        }
    }
}

enum RunError {
    Timeout,
    Cancelled,
    Spawn(anyhow::Error),
}

fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
    cancel: &Cancel,
) -> std::result::Result<ExitStatus, RunError> {
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Err(RunError::Spawn(e.into())),
    };

    match wait_with_timeout(&mut child, timeout, POLL_INTERVAL, cancel) {
        Ok(Outcome::Done(status)) => Ok(status),
        Ok(Outcome::TimedOut) | Ok(Outcome::Cancelled) => {
            // Never leave an orphaned ffmpeg behind; reap it either way.
            let _ = child.kill();
            let _ = child.wait();
            Err(if cancel.is_cancelled() {
                RunError::Cancelled
            } else {
                RunError::Timeout
            })
        }
        Err(e) => Err(RunError::Spawn(e.into())),
    }
}

/// Run the whole batch behind one interface: merge every pair in parallel,
/// record outcomes in the tracker, remove partial outputs after a cancel,
/// delete sources when asked, and fold the results into a single summary.
/// Callers bring pairs and get facts back — no save protocol to know.
pub fn run_batch(
    pairs: &[FilePair],
    stats: &ScanStats,
    opts: &MergeOptions,
    sdel: bool,
    progress: Option<&MergeProgress>,
    tracker: &StateTracker,
    cancel: &Cancel,
) -> MergeSummary {
    let results: Vec<MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            let result = merge_pair(pair, idx, opts, cancel);
            announce(&result, progress, opts.verbose);

            if let Err(e) = tracker.record(&pair.stem, result.success, result.was_interrupted) {
                eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
            }

            result
        })
        .collect();

    if let Err(e) = tracker.flush() {
        eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
    }

    if let Some(p) = progress {
        p.finish();
    }

    // Interrupted merges leave partial outputs; remove those. Source files of
    // an interrupted pair are never deleted: the stem stays unrecorded so a
    // resume re-picks it, and deleting one source now would orphan its partner.
    if cancel.is_cancelled() {
        for result in &results {
            if !result.was_interrupted {
                continue;
            }
            let pair = &pairs[result.pair_index];
            remove_quiet(&output_path(pair, opts), opts.verbose);
        }
    }

    let mut deletion_failures = 0;
    if sdel {
        for result in &results {
            if result.success {
                if let Err(e) = delete_source_files(&pairs[result.pair_index]) {
                    eprintln!("{} {}", t("warning_prefix").yellow(), e);
                    deletion_failures += 1;
                }
            }
        }
    }

    let mut summary = MergeSummary {
        skipped_count: stats.skipped,
        orphaned_count: stats.orphaned,
        deletion_failures,
        ..Default::default()
    };
    for result in &results {
        if result.success {
            summary.success_count += 1;
            summary.merge_count += 1;
            summary.total_duration += result.duration;
        } else if !result.was_interrupted {
            // Interrupted outcomes stay uncounted so a resume re-picks them.
            summary.failed_count += 1;
            if let Some(ref err) = result.error {
                summary
                    .failures
                    .push((result.pair_name.clone(), err.clone()));
            }
        }
    }
    summary
}

pub(crate) fn remove_quiet(path: &Path, verbose: bool) {
    if path.exists() {
        if let Err(e) = std::fs::remove_file(path) {
            eprintln!("{} {}", t("warning_prefix").yellow(), e);
        } else if verbose {
            eprintln!(
                "{}",
                tf("cleanup_partial", &[&path.display().to_string()]).yellow()
            );
        }
    }
}

pub(crate) fn delete_source_files(pair: &FilePair) -> Result<()> {
    let video_result = std::fs::remove_file(&pair.video);
    let audio_result = std::fs::remove_file(&pair.audio);

    let mut errors = Vec::new();
    if let Err(e) = video_result {
        errors.push(format!("video '{}' ({})", pair.video.display(), e));
    }
    if let Err(e) = audio_result {
        errors.push(format!("audio '{}' ({})", pair.audio.display(), e));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{}",
            tf("failed_delete", &[&errors.join(", ")])
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_summary_default() {
        let summary = MergeSummary::default();
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert_eq!(summary.merge_count, 0);
        assert!(summary.total_duration.is_zero());
        assert!(summary.all_success());
    }

    #[test]
    fn test_timing_methods() {
        let mut summary = MergeSummary::default();
        summary.total_duration += Duration::from_millis(100);
        summary.merge_count += 1;
        summary.total_duration += Duration::from_millis(300);
        summary.merge_count += 1;
        summary.total_duration += Duration::from_millis(200);
        summary.merge_count += 1;

        assert_eq!(summary.total_duration, Duration::from_millis(600));
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(200)));
        assert!(summary.throughput().is_some());
        assert!(summary.throughput().unwrap() > 0.0);
    }

    #[test]
    fn test_merge_summary_avg_duration_none_when_zero_merges() {
        let summary = MergeSummary::default();
        assert_eq!(summary.merge_count, 0);
        assert!(summary.avg_duration().is_none());
    }

    #[test]
    fn test_merge_summary_throughput_none_when_zero_duration() {
        let summary = MergeSummary {
            merge_count: 5,
            total_duration: Duration::ZERO,
            ..Default::default()
        };
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn test_format_duration() {
        use crate::progress::format_duration;
        assert!(format_duration(Duration::from_millis(50)).contains("ms"));
        assert!(format_duration(Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(Duration::from_secs(5)).contains("s"));
        assert!(!format_duration(Duration::from_secs(5)).contains("ms"));
        assert!(format_duration(Duration::from_secs(90)).contains("m"));
    }

    #[test]
    fn test_merge_summary_all_success_false_when_failures() {
        let summary = MergeSummary {
            success_count: 3,
            failed_count: 2,
            ..Default::default()
        };
        assert!(!summary.all_success());
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    // Note: These tests require ffmpeg to be installed
    // They test the function structure, not actual ffmpeg execution

    #[test]
    fn test_merge_result_debug() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            success: true,
            error: None,
            duration: Duration::from_millis(123),
            was_interrupted: false,
            retries: 0,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
    }

    fn opts_for<'a>(dir: &'a Path) -> MergeOptions<'a> {
        MergeOptions {
            output_dir: dir,
            format: OutputFormat::Mkv,
            verbose: false,
            retry: 0,
        }
    }

    #[test]
    fn test_merge_pair_skips_when_cancelled() {
        // A cancelled token short-circuits before ffmpeg is ever spawned, so
        // this needs no real media files and no ffmpeg on PATH.
        let cancel = Cancel::new();
        cancel.cancel();
        let pair = FilePair {
            video: std::path::PathBuf::from("does-not-exist.mp4"),
            audio: std::path::PathBuf::from("does-not-exist.m4a"),
            stem: "does-not-exist".to_string(),
        };

        let result = merge_pair(&pair, 0, &opts_for(std::path::Path::new(".")), &cancel);

        assert!(!result.success);
        assert!(result.was_interrupted);
    }

    #[test]
    fn test_merge_pair_fails_when_sources_missing() {
        // Not cancelled: reaches ffmpeg and fails on missing inputs rather than
        // short-circuiting — proves the cancel check is the only thing gating it.
        let cancel = Cancel::new();
        let dir = tempfile::tempdir().unwrap();
        let pair = FilePair {
            video: dir.path().join("missing.mp4"),
            audio: dir.path().join("missing.m4a"),
            stem: "missing".to_string(),
        };

        let result = merge_pair(&pair, 0, &opts_for(dir.path()), &cancel);

        assert!(!result.was_interrupted);
    }

    #[test]
    fn test_merge_result_with_error() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            success: false,
            error: Some("test error".to_string()),
            duration: Duration::from_secs(1),
            was_interrupted: false,
            retries: 0,
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use crate::state::MergeState;
    use tempfile::tempdir;

    fn missing_pair(dir: &Path, stem: &str) -> FilePair {
        FilePair {
            video: dir.join(format!("{stem}.mp4")),
            audio: dir.join(format!("{stem}.m4a")),
            stem: stem.to_string(),
        }
    }

    fn tracker_for(dir: &Path) -> StateTracker {
        StateTracker::new(MergeState::new(dir), dir, 5)
    }

    fn opts_for<'a>(dir: &'a Path) -> MergeOptions<'a> {
        MergeOptions {
            output_dir: dir,
            format: OutputFormat::Mkv,
            verbose: false,
            retry: 0,
        }
    }

    #[test]
    fn run_batch_empty_passes_stats_through() {
        let dir = tempdir().unwrap();
        let summary = run_batch(
            &[],
            &ScanStats {
                skipped: 5,
                orphaned: 3,
            },
            &opts_for(dir.path()),
            false,
            None,
            &tracker_for(dir.path()),
            &Cancel::new(),
        );

        assert_eq!(summary.skipped_count, 5);
        assert_eq!(summary.orphaned_count, 3);
        assert!(summary.all_success());
    }

    #[test]
    fn run_batch_counts_missing_sources_as_failures() {
        let dir = tempdir().unwrap();
        let pairs = vec![missing_pair(dir.path(), "a"), missing_pair(dir.path(), "b")];
        let tracker = tracker_for(dir.path());

        let summary = run_batch(
            &pairs,
            &ScanStats::default(),
            &opts_for(dir.path()),
            false,
            None,
            &tracker,
            &Cancel::new(),
        );

        assert_eq!(summary.failed_count, 2);
        assert!(!summary.all_success());
        // Outcomes are recorded for resume: both stems marked failed on disk.
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(loaded.failed.contains("a") && loaded.failed.contains("b"));
    }

    #[test]
    fn run_batch_cancelled_records_nothing_and_fails_nothing() {
        let dir = tempdir().unwrap();
        let pairs = vec![missing_pair(dir.path(), "a")];
        let cancel = Cancel::new();
        cancel.cancel();

        let summary = run_batch(
            &pairs,
            &ScanStats::default(),
            &opts_for(dir.path()),
            false,
            None,
            &tracker_for(dir.path()),
            &cancel,
        );

        // Interrupted outcomes stay uncounted and unrecorded for resume.
        assert_eq!(summary.failed_count, 0);
        assert!(summary.all_success());
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(!loaded.failed.contains("a"));
    }

    #[test]
    fn run_batch_cancelled_with_sdel_keeps_sources() {
        // Regression: interrupting a batch with --sdel used to delete the
        // source audio of pairs that never merged, orphaning the video on
        // resume. Cancelled pairs must keep both sources.
        let dir = tempdir().unwrap();
        let pair = missing_pair(dir.path(), "a");
        std::fs::write(&pair.video, b"v").unwrap();
        std::fs::write(&pair.audio, b"a").unwrap();
        let cancel = Cancel::new();
        cancel.cancel();

        run_batch(
            std::slice::from_ref(&pair),
            &ScanStats::default(),
            &opts_for(dir.path()),
            true, // sdel on
            None,
            &tracker_for(dir.path()),
            &cancel,
        );

        assert!(pair.video.exists(), "cancelled pair must keep its video");
        assert!(pair.audio.exists(), "cancelled pair must keep its audio");
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_delete_source_files_success() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_video_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Only create audio, video is missing
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("video"), "Error should mention video: {}", err);
    }

    #[test]
    fn test_delete_source_files_both_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Neither file exists
        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("video"), "Error should mention video: {}", err);
        assert!(err.contains("audio"), "Error should mention audio: {}", err);
    }

    #[test]
    fn test_deletion_failures_tracking() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Create video but NOT audio - deletion will fail for audio
        File::create(&video_path).unwrap();
        // audio_path does NOT exist

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        // delete_source_files should fail because audio doesn't exist
        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Deletion should fail when audio missing");

        // The error message should indicate which file failed
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("audio"),
            "Error should mention audio: {}",
            err_msg
        );

        // Video should be deleted (even though overall operation failed)
        assert!(
            !video_path.exists(),
            "Video should be deleted despite partial failure"
        );
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[test]
    fn wait_reports_cancel_promptly() {
        // A long-running child must be killed shortly after cancel fires,
        // not left to run until the 300 s timeout.
        #[cfg(unix)]
        let mut cmd = std::process::Command::new("sleep");
        #[cfg(unix)]
        cmd.arg("30");

        #[cfg(windows)]
        let mut cmd = std::process::Command::new("ping");
        #[cfg(windows)]
        cmd.args(["-n", "300", "127.0.0.1"]);

        let cancel = Cancel::new();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c.cancel();
        });

        let start = std::time::Instant::now();
        let outcome = run_with_timeout(&mut cmd, Duration::from_secs(300), &cancel);
        assert!(matches!(outcome, Err(RunError::Cancelled)));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "cancel must kill the child promptly"
        );
    }

    #[test]
    fn test_wait_timeout_normal_completion() {
        // Use cross-platform sleep command
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.1")
            .spawn()
            .expect("sleep command should be available");

        // `timeout` refuses to run when stdin is redirected (as under cargo test),
        // so ping is used as the portable delay on Windows.
        #[cfg(windows)]
        let mut child = std::process::Command::new("ping")
            .args(["-n", "2", "127.0.0.1"])
            .spawn()
            .expect("ping command should be available");

        let result = wait_with_timeout(
            &mut child,
            Duration::from_secs(5),
            POLL_INTERVAL,
            &Cancel::new(),
        );
        assert!(matches!(result.unwrap(), Outcome::Done(_)));
    }

    #[test]
    fn test_wait_timeout_exceeded() {
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("ping")
            .args(["-n", "6", "127.0.0.1"])
            .spawn()
            .expect("ping command should be available");

        let result = wait_with_timeout(
            &mut child,
            Duration::from_millis(50),
            POLL_INTERVAL,
            &Cancel::new(),
        );
        assert!(matches!(result.unwrap(), Outcome::TimedOut));
    }

    #[test]
    fn test_wait_timeout_already_finished() {
        // Very short sleep that finishes almost immediately
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.01")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        // Wait a bit for it to finish
        std::thread::sleep(Duration::from_millis(150));

        let result = wait_with_timeout(
            &mut child,
            Duration::from_secs(5),
            POLL_INTERVAL,
            &Cancel::new(),
        );
        assert!(matches!(result.unwrap(), Outcome::Done(_)));
    }
}
