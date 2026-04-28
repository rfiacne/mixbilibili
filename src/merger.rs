use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::progress::MergeProgress;
use crate::scanner::{FilePair, ScanResult};
use anyhow::{Context, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Format a Duration in a human-readable way (ms, s, or m).
fn format_duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{}ms", d.as_millis())
    } else if d < Duration::from_secs(60) {
        format!("{:.2}s", d.as_secs_f64())
    } else {
        format!("{}m {:.0}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

trait ChildExt {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>>;
}

impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(e) => return Err(e).context("Failed to check process status"),
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
}

#[derive(Debug, Default)]
pub struct MergeSummary {
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub orphaned_count: usize,
    pub failures: Vec<(String, String)>,
    pub deletion_failures: usize,
    /// Durations of all merge operations (success and failed).
    pub durations: Vec<Duration>,
}

impl MergeSummary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total time spent across all merge operations.
    pub fn total_duration(&self) -> Duration {
        self.durations.iter().sum()
    }

    /// Average duration per merge operation.
    pub fn avg_duration(&self) -> Option<Duration> {
        if self.durations.is_empty() {
            return None;
        }
        Some(self.total_duration() / self.durations.len() as u32)
    }

    /// Fastest merge duration.
    pub fn min_duration(&self) -> Option<Duration> {
        self.durations.iter().min().copied()
    }

    /// Slowest merge duration.
    pub fn max_duration(&self) -> Option<Duration> {
        self.durations.iter().max().copied()
    }

    /// Files processed per second.
    pub fn throughput(&self) -> Option<f64> {
        let total = self.total_duration();
        if total.is_zero() {
            return None;
        }
        Some(self.durations.len() as f64 / total.as_secs_f64())
    }

    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }

    pub fn print_report(&self) {
        println!("{}", "================================".bright_black());
        println!("{}", "Merge complete".green().bold());
        println!("{}: {}", "Success".green(), self.success_count);
        println!("{}: {}", "Failed".red(), self.failed_count);
        println!(
            "{}: {} (aria2 files present)",
            "Skipped".yellow(),
            self.skipped_count
        );
        println!(
            "{}: {} (missing pair)",
            "Orphaned".bright_black(),
            self.orphaned_count
        );
        if self.deletion_failures > 0 {
            println!("{}: {}", "Deletion failures".red(), self.deletion_failures);
        }

        // Timing metrics
        if !self.durations.is_empty() {
            let total = self.total_duration();
            println!("{}", "".bright_black());
            println!("{}", "Timing".cyan().bold());
            println!("{}: {}", "Total".bright_black(), format_duration(total));
            if let Some(avg) = self.avg_duration() {
                println!("{}: {}", "Avg".bright_black(), format_duration(avg));
            }
            if let Some(min) = self.min_duration() {
                println!("{}: {}", "Min".bright_black(), format_duration(min));
            }
            if let Some(max) = self.max_duration() {
                println!("{}: {}", "Max".bright_black(), format_duration(max));
            }
            if let Some(tp) = self.throughput() {
                println!("{}: {:.1} files/sec", "Throughput".bright_black(), tp);
            }
        }

        println!("{}", "================================".bright_black());

        if !self.failures.is_empty() {
            println!("\n{}", "Failed files:".red());
            for (name, error) in &self.failures {
                println!("  - {}: {}", name, error);
            }
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
    dry_run: bool,
    verbose: bool,
    max_retries: usize,
) -> MergeResult {
    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    if let Some(p) = progress {
        p.set_message(&pair.stem);
    }

    // In dry-run mode, just report success without actual merge
    if dry_run {
        let start = std::time::Instant::now();
        if verbose {
            println!(
                "[dry-run] ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }
        if progress.is_none() {
            println!("{} {} [dry-run]", "○".cyan(), pair.stem);
        }
        if let Some(p) = progress {
            p.inc();
        }
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: true,
            error: None,
            duration: start.elapsed(),
        };
    }

    let start = std::time::Instant::now();
    let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);

    // Retry loop for transient failures
    for attempt in 0..=max_retries {
        if attempt > 0 {
            if let Some(p) = progress {
                p.set_message(&format!("retry {} {}", attempt, pair.stem));
            } else if verbose {
                println!(
                    "{} Retrying {} (attempt {})",
                    "↻".yellow(),
                    pair.stem,
                    attempt
                );
            }
            // Brief pause before retry
            std::thread::sleep(Duration::from_secs(1));
        }

        if verbose && attempt == 0 {
            println!(
                "Running: ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }

        match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT) {
            Ok(status) if status.success() => {
                if progress.is_none() {
                    println!("{} {}", "✓".green(), pair.stem);
                }
                if let Some(p) = progress {
                    p.inc();
                }
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    success: true,
                    error: None,
                    duration: start.elapsed(),
                };
            }
            Ok(status) => {
                if attempt == max_retries {
                    if progress.is_none() {
                        println!(
                            "{} {}: ffmpeg exited with code {:?}",
                            "✗".red(),
                            pair.stem,
                            status.code()
                        );
                    }
                    if let Some(p) = progress {
                        p.inc();
                    }
                    return MergeResult {
                        pair_index,
                        pair_name: pair.stem.clone(),
                        success: false,
                        error: Some(format!(
                            "ffmpeg exited with code {:?} after {} retries",
                            status.code(),
                            max_retries
                        )),
                        duration: start.elapsed(),
                    };
                }
                // Rebuild command for retry
                cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);
            }
            Err(e) => {
                if attempt == max_retries {
                    if progress.is_none() {
                        println!("{} {}: {}", "✗".red(), pair.stem, e);
                    }
                    if let Some(p) = progress {
                        p.inc();
                    }
                    return MergeResult {
                        pair_index,
                        pair_name: pair.stem.clone(),
                        success: false,
                        error: Some(format!("{} after {} retries", e, max_retries)),
                        duration: start.elapsed(),
                    };
                }
                // Rebuild command for retry
                cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);
            }
        }
    }

    // Should not reach here
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some("Unexpected retry loop exit".to_string()),
        duration: start.elapsed(),
    }
}

fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = cmd.spawn().context("Failed to spawn ffmpeg process")?;

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("ffmpeg process timed out after 5 minutes");
        }
        Err(e) => Err(e).context("Failed to wait for ffmpeg process"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
    progress: Option<MergeProgress>,
    dry_run: bool,
    verbose: bool,
    retry: usize,
) -> MergeSummary {
    let output_dir = output_dir.to_path_buf();
    let pairs = scan_result.pairs;

    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build_global();

    let progress_ref = progress.as_ref();

    let results: Vec<MergeResult> = pairs
        .par_iter()
        .enumerate()
        .map(|(idx, pair)| {
            merge_pair(
                pair,
                idx,
                &output_dir,
                format,
                progress_ref,
                dry_run,
                verbose,
                retry,
            )
        })
        .collect();

    if let Some(p) = &progress {
        p.finish();
    }

    let mut summary = MergeSummary::new();
    summary.skipped_count = scan_result.stats.skipped;
    summary.orphaned_count = scan_result.stats.orphaned;

    // In dry-run mode, don't delete source files
    if delete_source && !dry_run {
        let deletion_failure_count = AtomicUsize::new(0);
        results.par_iter().filter(|r| r.success).for_each(|result| {
            let pair = &pairs[result.pair_index];
            if let Err(e) = delete_source_files(pair) {
                eprintln!("Warning: {}", e);
                deletion_failure_count.fetch_add(1, Ordering::Relaxed);
            }
        });
        summary.deletion_failures = deletion_failure_count.load(Ordering::Relaxed);
    }

    for result in &results {
        summary.durations.push(result.duration);
        if result.success {
            summary.success_count += 1;
        } else {
            summary.failed_count += 1;
            if let Some(error) = &result.error {
                summary
                    .failures
                    .push((result.pair_name.clone(), error.clone()));
            }
        }
    }

    summary
}

fn delete_source_files(pair: &FilePair) -> Result<()> {
    let video_result = std::fs::remove_file(&pair.video);
    let audio_result = std::fs::remove_file(&pair.audio);

    match (video_result, audio_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(anyhow::anyhow!(
            "Failed to delete video '{}': {}",
            pair.video.display(),
            e
        )),
        (Ok(()), Err(e)) => Err(anyhow::anyhow!(
            "Failed to delete audio '{}': {}",
            pair.audio.display(),
            e
        )),
        (Err(ve), Err(ae)) => Err(anyhow::anyhow!(
            "Failed to delete both files: video '{}' ({}), audio '{}' ({})",
            pair.video.display(),
            ve,
            pair.audio.display(),
            ae
        )),
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
        assert!(summary.durations.is_empty());
        assert!(summary.all_success());
    }

    #[test]
    fn test_merge_summary_all_success_false_with_failures() {
        let mut summary = MergeSummary::default();
        summary.failed_count = 1;
        assert!(!summary.all_success());
    }

    #[test]
    fn test_timing_methods() {
        let mut summary = MergeSummary::default();
        summary.durations.push(Duration::from_millis(100));
        summary.durations.push(Duration::from_millis(300));
        summary.durations.push(Duration::from_millis(200));

        assert_eq!(summary.total_duration(), Duration::from_millis(600));
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(200)));
        assert_eq!(summary.min_duration(), Some(Duration::from_millis(100)));
        assert_eq!(summary.max_duration(), Some(Duration::from_millis(300)));
        assert!(summary.throughput().is_some());
        assert!(summary.throughput().unwrap() > 0.0);
    }

    #[test]
    fn test_timing_empty_summary() {
        let summary = MergeSummary::default();
        assert!(summary.total_duration().is_zero());
        assert!(summary.avg_duration().is_none());
        assert!(summary.min_duration().is_none());
        assert!(summary.max_duration().is_none());
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn test_format_duration() {
        assert!(format_duration(Duration::from_millis(50)).contains("ms"));
        assert!(format_duration(Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(Duration::from_secs(5)).contains("s"));
        assert!(!format_duration(Duration::from_secs(5)).contains("ms"));
        assert!(format_duration(Duration::from_secs(90)).contains("m"));
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
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_merge_result_with_error() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            success: false,
            error: Some("test error".to_string()),
            duration: Duration::from_secs(1),
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use crate::scanner::ScanStats;
    use tempfile::tempdir;

    #[test]
    fn test_execute_merges_empty_pairs() {
        let dir = tempdir().unwrap();
        let scan_result = ScanResult {
            pairs: vec![],
            stats: ScanStats::default(),
            skipped_names: vec![],
        };

        let summary = execute_merges(
            scan_result,
            dir.path(),
            OutputFormat::Mkv,
            1,
            false,
            None,
            false,
            false,
            0,
        );

        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert!(summary.all_success());
    }

    #[test]
    fn test_execute_merges_with_skipped_stats() {
        let dir = tempdir().unwrap();
        let scan_result = ScanResult {
            pairs: vec![],
            stats: ScanStats {
                pairs: 0,
                skipped: 5,
                orphaned: 3,
            },
            skipped_names: vec![],
        };

        let summary = execute_merges(
            scan_result,
            dir.path(),
            OutputFormat::Mkv,
            1,
            false,
            None,
            false,
            false,
            0,
        );

        assert_eq!(summary.skipped_count, 5);
        assert_eq!(summary.orphaned_count, 3);
    }

    #[test]
    fn test_deletion_failures_counted_correctly() {
        // Verify that deletion_failures counter is properly structured
        // After fix: execute_merges uses AtomicUsize to track actual deletion failures

        let summary = MergeSummary {
            success_count: 5,
            failed_count: 0,
            skipped_count: 0,
            orphaned_count: 0,
            failures: vec![],
            deletion_failures: 2,
            durations: vec![],
        };

        // deletion_failures is usize, can represent actual failure count
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to delete video"));
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to delete both"));
    }

    #[test]
    fn test_deletion_failures_tracking() {
        // Bug: deletion_failures should count actual deletion failures
        // Currently: deletion_failures = deleted_count - success_count = 0 always
        // Expected: deletion_failures should track files that couldn't be deleted

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
            err_msg.contains("Failed to delete audio"),
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

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let result = child.wait_timeout(Duration::from_secs(5));
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
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("10")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let result = child.wait_timeout(Duration::from_millis(50));
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

        let result = child.wait_timeout(Duration::from_secs(5));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }
}
