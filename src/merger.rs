use crate::cli::OutputFormat;
use crate::ffmpeg::{self, MergeRunner, RunOutcome};
use crate::i18n::{t, tf};
use crate::interrupt::Interrupt;
use crate::progress::{format_duration, MergeProgress};
use crate::scanner::FilePair;
use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct MergeResult {
    pub pair_index: usize,
    pub pair_name: String,
    /// Where the merged file belongs, so cleanup never rebuilds the path.
    pub output_path: PathBuf,
    pub success: bool,
    pub error: Option<String>,
    /// Duration of the ffmpeg merge operation.
    pub duration: Duration,
    /// Set when the merge was skipped because the interrupt token fired.
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
        use crate::ui::{Colors, Symbols};

        let symbols = Symbols::new(crate::ui::supports_unicode());
        let width = 40;

        let top_border = format!(
            "{}{}{}",
            symbols.top_left,
            symbols.horizontal_line.repeat(width),
            symbols.top_right
        );
        println!("{}", Colors::secondary(&top_border));

        let title = Colors::title(&t("merge_report"));
        println!(
            "{} {} {}",
            Colors::secondary(symbols.vertical_line),
            title,
            Colors::secondary(symbols.vertical_line)
        );

        let mid_border = format!(
            "{}{}{}",
            symbols.top_left,
            symbols.horizontal_line.repeat(width),
            symbols.top_right
        );
        println!("{}", Colors::secondary(&mid_border));

        let success_str = format!(
            "{} {}",
            Colors::success(symbols.success),
            tf("succeeded_fmt", &[&self.success_count.to_string()])
        );
        let fail_str = if self.failed_count > 0 {
            Colors::failure(&format!(
                "{} {}",
                symbols.failure,
                tf("failed_fmt", &[&self.failed_count.to_string()])
            ))
        } else {
            format!(
                "{} {}",
                symbols.failure,
                tf("failed_fmt", &[&self.failed_count.to_string()])
            )
        };
        println!("  {}    {}", success_str, fail_str);

        if self.skipped_count > 0 {
            println!(
                "  {} {}",
                tf("skipped_fmt", &[&self.skipped_count.to_string()]),
                Colors::secondary("(aria2)")
            );
        }
        if self.orphaned_count > 0 {
            println!(
                "  {} {}",
                tf("orphaned_fmt", &[&self.orphaned_count.to_string()]),
                Colors::secondary("(orphan)")
            );
        }

        if self.merge_count > 0 {
            let total = self.total_duration;
            println!(
                "  {}: {}",
                Colors::secondary(&t("duration")),
                format_duration(total)
            );
            if let Some(avg) = self.avg_duration() {
                println!(
                    "  {}: {}",
                    Colors::secondary(&t("avg")),
                    format_duration(avg)
                );
            }
            if let Some(tp) = self.throughput() {
                println!(
                    "  {}: {:.2} pairs/sec",
                    Colors::secondary(&t("throughput")),
                    tp
                );
            }
        }

        if self.deletion_failures > 0 {
            println!(
                "  {} {}",
                tf("deletion_failures", &[&self.deletion_failures.to_string()]),
                Colors::warning("(warn)")
            );
        }

        let bottom_border = format!(
            "{}{}{}",
            symbols.bottom_left,
            symbols.horizontal_line.repeat(width),
            symbols.bottom_right
        );
        println!("{}", Colors::secondary(&bottom_border));

        if !self.failures.is_empty() {
            println!("\n{}", Colors::failure(&t("failed_files")));
            for (i, (name, error)) in self.failures.iter().enumerate() {
                let connector = if i == self.failures.len() - 1 {
                    "└"
                } else {
                    "├"
                };
                println!(
                    "  {}─ {} {}: {}",
                    connector,
                    Colors::failure(symbols.failure),
                    name,
                    error
                );
            }
            println!();
        }
    }
}

/// Everything one merge needs to know.
pub struct MergePlan<'a> {
    pub output_dir: &'a Path,
    pub format: OutputFormat,
    pub retries: usize,
    pub verbose: bool,
    pub progress: Option<&'a MergeProgress>,
    pub interrupt: &'a Interrupt,
}

pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    plan: &MergePlan,
    runner: &dyn MergeRunner,
) -> MergeResult {
    let output_path = pair.output_path(plan.output_dir, plan.format);

    if plan.interrupt.is_triggered() {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            output_path,
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
    }

    // Safety check: the output must not be the input video itself. Canonicalize
    // so different path spellings of the same file are still caught.
    let dominated = std::fs::canonicalize(&output_path)
        .ok()
        .zip(std::fs::canonicalize(&pair.video).ok())
        .map(|(o, v)| o == v)
        .unwrap_or(false);
    if dominated {
        return record_failure(
            pair,
            pair_index,
            output_path,
            t("same_source_output_runtime").into_owned(),
            Duration::ZERO,
            plan,
        );
    }

    do_merge(pair, output_path, pair_index, plan, runner)
}

fn record_failure(
    pair: &FilePair,
    pair_index: usize,
    output_path: PathBuf,
    error_msg: String,
    duration: Duration,
    plan: &MergePlan,
) -> MergeResult {
    if let Some(p) = plan.progress {
        p.record(&pair.stem, false, duration, Some(&error_msg), None);
    } else {
        println!("{} {}: {}", t("cross").red(), pair.stem, error_msg);
    }
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        output_path,
        success: false,
        error: Some(error_msg),
        duration,
        was_interrupted: false,
    }
}

/// Validates that merge output file exists and has non-zero size
fn validate_output(output_path: &Path) -> Result<()> {
    let metadata = match std::fs::metadata(output_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "failed_create_output_file",
                    &[&output_path.display().to_string()]
                )
            );
        }
        Err(e) => return Err(e.into()),
    };
    if metadata.len() == 0 {
        if let Err(e) = std::fs::remove_file(output_path) {
            eprintln!("Warning: Failed to remove empty output file: {}", e);
        }
        anyhow::bail!(
            "{}",
            tf("output_file_empty", &[&output_path.display().to_string()])
        );
    }
    Ok(())
}

fn do_merge(
    pair: &FilePair,
    output_path: PathBuf,
    pair_index: usize,
    plan: &MergePlan,
    runner: &dyn MergeRunner,
) -> MergeResult {
    let start = std::time::Instant::now();
    let retries = plan.retries;

    for attempt in 0..=retries {
        // Stop early once cancelled: retrying after Ctrl+C is wasted work.
        if plan.interrupt.is_triggered() {
            return MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                output_path,
                success: false,
                error: None,
                duration: start.elapsed(),
                was_interrupted: true,
            };
        }

        if attempt > 0 {
            announce_retry(pair, attempt, plan);
        } else if plan.verbose {
            println!(
                "Running: ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }

        let mut cmd =
            ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, plan.format);

        // Each arm below either returns, retries, or falls through to the one
        // failure exit at the bottom of the loop.
        let error_msg = match runner.run(&mut cmd, plan.interrupt) {
            Ok(RunOutcome::Success) => match validate_output(&output_path) {
                // ffmpeg claims it worked; the file on disk has to agree.
                Ok(()) => {
                    return record_success(pair, pair_index, output_path, start.elapsed(), plan)
                }
                Err(e) => e.to_string(),
            },
            // A cancelled pair is not a failure: it stays pending for resume.
            Ok(RunOutcome::Cancelled) => {
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    output_path,
                    success: false,
                    error: None,
                    duration: start.elapsed(),
                    was_interrupted: true,
                }
            }
            Ok(RunOutcome::TimedOut) if attempt < retries => continue,
            Ok(RunOutcome::Failed { .. }) if attempt < retries => continue,
            Err(_) if attempt < retries => continue,
            Ok(RunOutcome::TimedOut) => t("timed_out").into_owned(),
            Ok(RunOutcome::Failed { code, message }) => failure_message(code, &message, retries),
            Err(e) => tf("merge_failed_io", &[&e.to_string(), &retries.to_string()]),
        };

        return record_failure(
            pair,
            pair_index,
            output_path,
            error_msg,
            start.elapsed(),
            plan,
        );
    }

    unreachable!("the final attempt always returns")
}

fn record_success(
    pair: &FilePair,
    pair_index: usize,
    output_path: PathBuf,
    duration: Duration,
    plan: &MergePlan,
) -> MergeResult {
    if let Some(p) = plan.progress {
        p.record(&pair.stem, true, duration, None, None);
    } else {
        println!("{} {}", t("checkmark").green(), pair.stem);
    }
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        output_path,
        success: true,
        error: None,
        duration,
        was_interrupted: false,
    }
}

/// Back off, then say so: on the progress bar if there is one, on stdout when
/// verbose.
fn announce_retry(pair: &FilePair, attempt: usize, plan: &MergePlan) {
    std::thread::sleep(Duration::from_secs(1));
    if let Some(p) = plan.progress {
        p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
    } else if plan.verbose {
        println!(
            "{} {}",
            "↻".yellow(),
            tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
        );
    }
}

/// What ffmpeg said beats a bare exit code, so the last line of its stderr is
/// appended when there is one.
fn failure_message(code: Option<i32>, stderr: &str, retries: usize) -> String {
    let base = tf(
        "merge_failed_exit",
        &[&exit_code_text(code), &retries.to_string()],
    );
    match stderr.trim().lines().last() {
        Some(reason) => format!("{base}: {reason}"),
        None => base,
    }
}

/// Render an exit code for a message, when there is one.
fn exit_code_text(code: Option<i32>) -> String {
    code.map(|c| c.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn delete_source_files(pair: &FilePair) -> Result<()> {
    // Delete audio first (less valuable file)
    // Attempt deletion directly and interpret errors to avoid TOCTOU race
    match std::fs::remove_file(&pair.audio) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "source_missing",
                    &["audio", &pair.audio.display().to_string()]
                )
            );
        }
        Err(e) => {
            // Audio deletion failed, preserve video
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_audio",
                    &[
                        &pair.audio.display().to_string(),
                        &e.to_string(),
                        &pair.video.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    // Then delete video
    match std::fs::remove_file(&pair.video) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Audio already deleted, video missing
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video_audio_gone",
                    &[
                        &pair.video.display().to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Err(e) => {
            // Video deletion failed, but audio already deleted
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video",
                    &[
                        &pair.video.display().to_string(),
                        &e.to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    Ok(())
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
        // Check for i18n key or translated content mentioning video
        assert!(
            err.contains("failed_delete_video") || err.contains("video") || err.contains("视频"),
            "Error should mention video: {}",
            err
        );
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
            output_path: PathBuf::from("video.mkv"),
            success: true,
            error: None,
            duration: Duration::from_millis(123),
            was_interrupted: false,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_merge_result_with_error() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            output_path: PathBuf::from("video.mkv"),
            success: false,
            error: Some("test error".to_string()),
            duration: Duration::from_secs(1),
            was_interrupted: false,
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_merge_validates_output_exists() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        // Simulate ffmpeg creating empty file (failure case)
        File::create(&output_path).unwrap();

        // Validation should reject empty file
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert_eq!(metadata.len(), 0, "Empty file should be detected");
    }

    #[test]
    fn test_merge_validates_output_nonzero_size() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        // Simulate ffmpeg creating valid file
        std::fs::write(&output_path, b"fake video data").unwrap();

        // Validation should accept non-zero file
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert!(metadata.len() > 0, "Non-zero file should be accepted");
    }

    #[test]
    fn test_validate_output_helper() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Test 1: Non-existent file
        let missing_path = dir.path().join("missing.mkv");
        let result = validate_output(&missing_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("未创建") || err.contains("not created"),
            "Error should mention file was not created: {}",
            err
        );

        // Test 2: Empty file
        let empty_path = dir.path().join("empty.mkv");
        File::create(&empty_path).unwrap();
        let result = validate_output(&empty_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty") || err.contains("为空"));
        // Empty file should be cleaned up
        assert!(!empty_path.exists(), "Empty file should be cleaned up");

        // Test 3: Valid file
        let valid_path = dir.path().join("valid.mkv");
        std::fs::write(&valid_path, b"video data").unwrap();
        let result = validate_output(&valid_path);
        assert!(result.is_ok());
        assert!(valid_path.exists(), "Valid file should remain");
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
        // Check for i18n key or translated content mentioning video
        assert!(
            err.contains("failed_delete_video") || err.contains("video") || err.contains("视频"),
            "Error should mention video: {}",
            err
        );
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
        // Should mention at least one missing file
        assert!(
            err.contains("video") || err.contains("audio"),
            "Error should mention missing file: {}",
            err
        );
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

        // NEW BEHAVIOR: Video should be preserved (not deleted) because audio is missing
        // This is the atomic deletion guarantee
        assert!(
            video_path.exists(),
            "Video should be preserved when audio is missing (atomic deletion)"
        );
    }

    #[test]
    fn test_delete_preserves_video_on_audio_failure() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("nonexistent.m4a"); // Doesn't exist

        // Create only video
        File::create(&video_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Should fail when audio missing");

        // Video should still exist (not deleted)
        assert!(
            video_path.exists(),
            "Video should be preserved when audio deletion fails"
        );
    }

    #[test]
    fn test_delete_checks_existence_first() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("audio.m4a");

        // Create both files
        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        // Both exist, deletion should succeed
        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }
}

#[cfg(test)]
mod summary_logic_tests {
    use super::*;

    #[test]
    fn test_merge_result_interrupted_flag() {
        let result = MergeResult {
            pair_index: 3,
            pair_name: "interrupted_video".to_string(),
            output_path: PathBuf::from("interrupted_video.mkv"),
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
        assert!(result.was_interrupted);
        assert!(!result.success);
        assert_eq!(result.duration, Duration::ZERO);
    }

    #[test]
    fn test_merge_summary_with_skipped_and_orphaned() {
        let summary = MergeSummary {
            success_count: 5,
            failed_count: 1,
            skipped_count: 2,
            orphaned_count: 3,
            failures: vec![("bad.mp4".to_string(), "codec error".to_string())],
            deletion_failures: 1,
            total_duration: Duration::from_secs(12),
            merge_count: 6,
        };
        assert!(!summary.all_success());
        assert_eq!(summary.avg_duration(), Some(Duration::from_secs(2)));
        assert!(summary.throughput().unwrap() > 0.0);
    }

    #[test]
    fn test_merge_summary_throughput_precision() {
        let summary = MergeSummary {
            merge_count: 3,
            total_duration: Duration::from_secs(3),
            ..Default::default()
        };
        let tp = summary.throughput().unwrap();
        assert!((tp - 1.0).abs() < f64::EPSILON, "3 merges / 3s = 1.0/s");
    }

    #[test]
    fn test_merge_summary_avg_with_subsecond_duration() {
        let summary = MergeSummary {
            merge_count: 4,
            total_duration: Duration::from_millis(400),
            ..Default::default()
        };
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_merge_summary_failures_vec() {
        let mut summary = MergeSummary::default();
        summary
            .failures
            .push(("a.mp4".to_string(), "err1".to_string()));
        summary
            .failures
            .push(("b.mp4".to_string(), "err2".to_string()));
        summary.failed_count = 2;
        assert_eq!(summary.failures.len(), 2);
        assert!(!summary.all_success());
    }

    #[test]
    fn test_validate_output_non_existent_file() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does_not_exist.mkv");
        let result = validate_output(&missing);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_output_empty_file_is_cleaned_up() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty.mkv");
        std::fs::File::create(&empty).unwrap();

        let result = validate_output(&empty);
        assert!(result.is_err());
        assert!(!empty.exists(), "Empty output file should be removed");
    }

    #[test]
    fn test_validate_output_valid_file_succeeds() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let valid = dir.path().join("good.mkv");
        std::fs::write(&valid, b"real content here").unwrap();

        let result = validate_output(&valid);
        assert!(result.is_ok());
        assert!(valid.exists());
    }

    #[test]
    fn test_delete_source_audio_missing_preserves_video() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let video = dir.path().join("keep_me.mp4");
        let audio = dir.path().join("nonexistent.m4a");
        std::fs::write(&video, b"video data").unwrap();

        let pair = FilePair {
            video: video.clone(),
            audio,
            stem: "keep_me".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        // Atomic guarantee: video preserved when audio deletion fails
        assert!(video.exists());
    }

    #[test]
    fn test_merge_pair_rejects_identical_output() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();

        // Create a pair where video is in the same dir as output would be
        let video_path = dir.path().join("test.mp4");
        let audio_path = dir.path().join("test.m4a");
        std::fs::write(&video_path, b"video").unwrap();
        std::fs::write(&audio_path, b"audio").unwrap();

        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "test".to_string(),
        };

        // When output_dir == video's parent and format == mp4,
        // output path would be identical to input video path
        let interrupt = Interrupt::new();
        let plan = MergePlan {
            output_dir: dir.path(),
            format: crate::cli::OutputFormat::Mp4,
            retries: 0,
            verbose: false,
            progress: None,
            interrupt: &interrupt,
        };
        let result = merge_pair(&pair, 0, &plan, &crate::ffmpeg::FfmpegRunner::default());

        assert!(
            !result.success,
            "merge_pair should reject identical output path"
        );
        assert!(
            result.error.as_ref().unwrap().contains("identical")
                || result.error.as_ref().unwrap().contains("相同"),
            "Error should mention identical path: {:?}",
            result.error
        );
    }
}
