use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::i18n::{t, tf};
use crate::progress::{format_duration, MergeProgress};
use crate::scanner::{FilePair, ScanStats};
use crate::state::StateTracker;
use anyhow::{Context, Result};
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

/// Extension trait for `std::process::Child` that adds timeout-aware waiting.
trait ChildExt {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>>;
}

impl ChildExt for Child {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[derive(Debug)]
pub struct MergeResult {
    pub pair_index: usize,
    pub pair_name: String,
    pub success: bool,
    pub error: Option<String>,
    /// Duration of the ffmpeg merge operation.
    pub duration: Duration,
    /// Set to true when the merge was skipped due to INTERRUPTED flag.
    pub was_interrupted: bool,
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

    pub fn print_report(&self, quiet: bool) {
        if quiet {
            self.print_quiet_report();
        } else {
            self.print_full_report();
        }
    }

    fn print_quiet_report(&self) {
        let total = self.success_count + self.failed_count;
        if self.failed_count > 0 {
            println!(
                "{}",
                tf(
                    "merged_summary_fail",
                    &[
                        &self.success_count.to_string(),
                        &total.to_string(),
                        &self.failed_count.to_string(),
                    ]
                )
            );
        } else {
            println!(
                "{}",
                tf(
                    "merged_summary_ok",
                    &[&self.success_count.to_string(), &total.to_string(),]
                )
            );
        }
    }

    fn print_full_report(&self) {
        println!("{}", t("separator").bright_black());
        println!("{}", t("merge_report").cyan().bold());
        println!("{}", t("separator").bright_black());

        let success_str = format!(
            "{} {}",
            t("checkmark").green(),
            tf("succeeded_fmt", &[&self.success_count.to_string()])
        );
        let fail_str = if self.failed_count > 0 {
            format!(
                "{} {}",
                t("cross").red(),
                tf("failed_fmt", &[&self.failed_count.to_string()])
            )
            .red()
            .to_string()
        } else {
            format!(
                "{} {}",
                t("cross"),
                tf("failed_fmt", &[&self.failed_count.to_string()])
            )
        };
        println!("  {}    {}", success_str, fail_str);

        if self.skipped_count > 0 {
            println!(
                "  {} {}",
                tf("skipped_fmt", &[&self.skipped_count.to_string()]),
                "(aria2)".bright_black()
            );
        }
        if self.orphaned_count > 0 {
            println!(
                "  {} {}",
                tf("orphaned_fmt", &[&self.orphaned_count.to_string()]),
                "(orphan)".bright_black()
            );
        }

        if self.merge_count > 0 {
            let total = self.total_duration;
            println!(
                "  {}: {}",
                t("duration").bright_black(),
                format_duration(total)
            );
            if let Some(avg) = self.avg_duration() {
                println!("  {}: {}", t("avg").bright_black(), format_duration(avg));
            }
            if let Some(tp) = self.throughput() {
                println!("  {}: {:.2} pairs/sec", t("throughput").bright_black(), tp);
            }
        }

        if self.deletion_failures > 0 {
            println!(
                "  {} {}",
                tf("deletion_failures", &[&self.deletion_failures.to_string()]),
                "(warn)".yellow()
            );
        }

        println!("{}", t("separator").bright_black());

        if !self.failures.is_empty() {
            println!("\n{}", t("failed_files").red().bold());
            for (name, error) in &self.failures {
                println!("  {} {}: {}", t("cross").red(), name, error);
            }
            println!();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    max_retries: usize,
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
        };
    }

    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    do_merge(
        pair,
        &output_path,
        pair_index,
        format,
        progress,
        verbose,
        max_retries,
    )
}

fn record_failure(
    pair: &FilePair,
    pair_index: usize,
    error_msg: String,
    duration: Duration,
    progress: Option<&MergeProgress>,
) -> MergeResult {
    if let Some(p) = progress {
        p.record(&pair.stem, false, duration, Some(&error_msg), None);
    } else {
        println!("{} {}: {}", t("cross").red(), pair.stem, error_msg);
    }
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(error_msg),
        duration,
        was_interrupted: false,
    }
}

fn do_merge(
    pair: &FilePair,
    output_path: &Path,
    pair_index: usize,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    max_retries: usize,
) -> MergeResult {
    let start = std::time::Instant::now();

    for attempt in 0..=max_retries {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1));
            if let Some(p) = progress {
                p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
            } else if verbose {
                println!(
                    "{} {}",
                    "↻".yellow(),
                    tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
                );
            }
        }

        if verbose && attempt == 0 {
            println!(
                "Running: ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }

        let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, output_path, format);
        match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT) {
            Ok(status) if status.success() => {
                let duration = start.elapsed();
                if let Some(p) = progress {
                    p.record(&pair.stem, true, duration, None, None);
                } else {
                    println!("{} {}", t("checkmark").green(), pair.stem);
                }
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    success: true,
                    error: None,
                    duration,
                    was_interrupted: false,
                };
            }
            Ok(_) if attempt < max_retries => continue,
            Ok(status) => {
                let error_msg = tf(
                    "merge_failed_exit",
                    &[
                        &status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        &max_retries.to_string(),
                    ],
                );
                return record_failure(pair, pair_index, error_msg, start.elapsed(), progress);
            }
            Err(_) if attempt < max_retries => continue,
            Err(e) => {
                let error_msg = tf(
                    "merge_failed_io",
                    &[&e.to_string(), &max_retries.to_string()],
                );
                return record_failure(pair, pair_index, error_msg, start.elapsed(), progress);
            }
        }
    }

    unreachable!("retry loop should always return")
}

fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = cmd.spawn().context(t("failed_to_spawn"))?;

    match child.wait_with_timeout(timeout, POLL_INTERVAL) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("{}", t("timed_out"));
        }
        Err(e) => Err(e).context(t("failed_to_wait")),
    }
}

/// Run the whole batch behind one interface: merge every pair in parallel,
/// record outcomes in the tracker, remove partial outputs after a cancel,
/// delete sources when asked, and fold the results into a single summary.
/// Callers bring pairs and get facts back — no save protocol to know.
#[allow(clippy::too_many_arguments)]
pub fn run_batch(
    pairs: &[FilePair],
    stats: &ScanStats,
    output_dir: &Path,
    format: OutputFormat,
    sdel: bool,
    verbose: bool,
    retry: usize,
    progress: Option<&MergeProgress>,
    tracker: &StateTracker,
    cancel: &Cancel,
) -> MergeSummary {
    let results: Vec<MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            let result = merge_pair(
                pair, idx, output_dir, format, progress, verbose, retry, cancel,
            );

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

    // Interrupted merges leave partial outputs; remove them.
    if cancel.is_cancelled() {
        for result in &results {
            if !result.was_interrupted {
                continue;
            }
            let pair = &pairs[result.pair_index];
            let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));
            remove_quiet(&output_path, verbose);
            if sdel && pair.audio.exists() {
                remove_quiet(&pair.audio, verbose);
            }
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

fn remove_quiet(path: &Path, verbose: bool) {
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
    use tempfile::tempdir;

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
    fn test_merge_summary_all_success_false_with_failures() {
        let summary = MergeSummary {
            failed_count: 1,
            ..Default::default()
        };
        assert!(!summary.all_success());
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
    fn test_timing_empty_summary() {
        let summary = MergeSummary::default();
        assert!(summary.total_duration.is_zero());
        assert!(summary.avg_duration().is_none());
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn test_merge_summary_avg_duration_none_when_zero_merges() {
        let summary = MergeSummary::default();
        assert_eq!(summary.merge_count, 0);
        assert!(summary.avg_duration().is_none());
    }

    #[test]
    fn test_merge_summary_avg_duration_some_when_merges_exist() {
        let summary = MergeSummary {
            total_duration: Duration::from_secs(6),
            merge_count: 3,
            ..Default::default()
        };
        assert_eq!(summary.avg_duration(), Some(Duration::from_secs(2)));
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
    fn test_merge_summary_throughput_computes_rate() {
        let summary = MergeSummary {
            merge_count: 10,
            total_duration: Duration::from_secs(2),
            ..Default::default()
        };
        let tp = summary.throughput().unwrap();
        assert!((tp - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_format_duration() {
        assert!(format_duration(Duration::from_millis(50)).contains("ms"));
        assert!(format_duration(Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(Duration::from_secs(5)).contains("s"));
        assert!(!format_duration(Duration::from_secs(5)).contains("ms"));
        assert!(format_duration(Duration::from_secs(90)).contains("m"));
    }

    #[test]
    fn test_merge_summary_accumulate() {
        let mut summary = MergeSummary::default();
        summary.success_count += 3;
        summary.failed_count += 1;
        summary.total_duration += Duration::from_secs(10);
        summary.merge_count += 4;

        assert_eq!(summary.success_count, 3);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.merge_count, 4);
        assert_eq!(summary.total_duration, Duration::from_secs(10));
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(2500)));
    }

    #[test]
    fn test_merge_summary_all_success_when_no_failures() {
        let summary = MergeSummary {
            success_count: 5,
            failed_count: 0,
            ..Default::default()
        };
        assert!(summary.all_success());
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

    #[test]
    fn test_delete_source_files_both_succeed() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("test_v.mp4");
        let audio_path = dir.path().join("test_a.m4a");
        std::fs::write(&video_path, b"fake video").unwrap();
        std::fs::write(&audio_path, b"fake audio").unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "test".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_video_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("missing.mp4");
        let audio_path = dir.path().join("exists.m4a");
        std::fs::write(&audio_path, b"fake audio").unwrap();

        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "missing".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("video"), "Error should mention video: {}", err);
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
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
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

        let result = merge_pair(
            &pair,
            0,
            std::path::Path::new("."),
            OutputFormat::Mkv,
            None,
            false,
            0,
            &cancel,
        );

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

        let result = merge_pair(
            &pair,
            0,
            dir.path(),
            OutputFormat::Mkv,
            None,
            false,
            0,
            &cancel,
        );

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
        StateTracker::new(MergeState::new(dir, dir), dir, 5)
    }

    #[test]
    fn run_batch_empty_passes_stats_through() {
        let dir = tempdir().unwrap();
        let summary = run_batch(
            &[],
            &ScanStats {
                pairs: 0,
                skipped: 5,
                orphaned: 3,
            },
            dir.path(),
            OutputFormat::Mkv,
            false,
            false,
            0,
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
            dir.path(),
            OutputFormat::Mkv,
            false,
            false,
            0,
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
            dir.path(),
            OutputFormat::Mkv,
            false,
            false,
            0,
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
    fn test_deletion_failures_counted_correctly() {
        let summary = MergeSummary {
            success_count: 5,
            failed_count: 0,
            skipped_count: 0,
            orphaned_count: 0,
            failures: vec![],
            deletion_failures: 2,
            total_duration: Duration::ZERO,
            merge_count: 0,
        };
        assert_eq!(summary.deletion_failures, 2);
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

        let result = child.wait_with_timeout(Duration::from_secs(5), POLL_INTERVAL);
        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.is_some());
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

        let result = child.wait_with_timeout(Duration::from_millis(50), POLL_INTERVAL);
        assert!(result.is_ok());
        // Should return None indicating timeout
        let status = result.unwrap();
        assert!(status.is_none());
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

        let result = child.wait_with_timeout(Duration::from_secs(5), POLL_INTERVAL);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }
}
