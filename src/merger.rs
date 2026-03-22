// src/merger.rs
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::scanner::{FilePair, ScanResult};
use anyhow::{Context, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;

/// Default timeout for ffmpeg process (5 minutes)
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Polling interval for checking process status
const POLL_INTERVAL_MILLIS: u64 = 100;

/// Default timeout for ffmpeg process
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(DEFAULT_TIMEOUT_SECS);

/// Extension trait for waiting with timeout
trait ChildExt {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>>;
}

impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(POLL_INTERVAL_MILLIS);

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(e).context("Failed to check process status"),
            }
        }
    }
}

/// Result of a single merge operation.
#[derive(Debug)]
pub struct MergeResult {
    /// Index of the pair in the original pairs vector
    pub pair_index: usize,
    /// The stem name of the processed pair
    pub pair_name: String,
    /// Whether the merge succeeded
    pub success: bool,
    /// Error message if the merge failed
    pub error: Option<String>,
}

/// Summary of all merge operations.
#[derive(Debug, Default)]
pub struct MergeSummary {
    /// Number of successful merges
    pub success_count: usize,
    /// Number of failed merges
    pub failed_count: usize,
    /// Number of skipped pairs (aria2 files present)
    pub skipped_count: usize,
    /// Number of orphaned files
    pub orphaned_count: usize,
    /// List of failed merges with error messages
    pub failures: Vec<(String, String)>,
    /// Number of source file deletion failures
    pub deletion_failures: usize,
}

impl MergeSummary {
    /// Create a new empty merge summary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all operations succeeded
    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }

    /// Print a formatted summary report
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
        println!("{}", "================================".bright_black());

        if !self.failures.is_empty() {
            println!("\n{}", "Failed files:".red());
            for (name, error) in &self.failures {
                println!("  - {}: {}", name, error);
            }
        }
    }
}

/// Merge a single file pair using ffmpeg.
///
/// # Arguments
///
/// * `pair` - The file pair to merge
/// * `pair_index` - Index for tracking in results
/// * `output_dir` - Directory for output file
/// * `format` - Output format (MKV, MP4, MOV)
///
/// # Returns
///
/// A `MergeResult` indicating success or failure.
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
) -> MergeResult {
    // Validate stem doesn't contain path separators
    if pair.stem.contains('/') || pair.stem.contains('\\') {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: false,
            error: Some(format!("Invalid characters in filename: {}", pair.stem)),
        };
    }

    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);

    match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT) {
        Ok(status) if status.success() => {
            println!("{} {}", "✓".green(), pair.stem);
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: true,
                error: None,
            }
        }
        Ok(status) => {
            println!(
                "{} {}: ffmpeg exited with code {:?}",
                "✗".red(),
                pair.stem,
                status.code()
            );
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(format!("ffmpeg exited with code {:?}", status.code())),
            }
        }
        Err(e) => {
            println!("{} {}: {}", "✗".red(), pair.stem, e);
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(e.to_string()),
            }
        }
    }
}

/// Run a command with timeout
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

/// Execute parallel merges with controlled concurrency.
///
/// # Arguments
///
/// * `scan_result` - Result from directory scanning
/// * `output_dir` - Directory for output files
/// * `format` - Output format
/// * `jobs` - Number of parallel processes
/// * `delete_source` - Whether to delete source files after success
///
/// # Returns
///
/// A `MergeSummary` with results of all operations.
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
) -> MergeSummary {
    let output_dir = output_dir.to_path_buf();

    // Use reference instead of clone
    let pairs = &scan_result.pairs;

    // Configure thread pool
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .unwrap();

    // Execute merges in parallel with indices
    let results: Vec<MergeResult> = pool.install(|| {
        pairs
            .par_iter()
            .enumerate()
            .map(|(idx, pair)| merge_pair(pair, idx, &output_dir, format))
            .collect()
    });

    // Build summary
    let mut summary = MergeSummary::new();
    summary.skipped_count = scan_result.stats.skipped;
    summary.orphaned_count = scan_result.stats.orphaned;

    for result in results {
        if result.success {
            summary.success_count += 1;

            // Delete source files if requested
            if delete_source {
                let pair = &pairs[result.pair_index];
                if let Err(e) = delete_source_files(pair) {
                    eprintln!("Warning: {}", e);
                    summary.deletion_failures += 1;
                }
            }
        } else {
            summary.failed_count += 1;
            if let Some(error) = result.error {
                summary.failures.push((result.pair_name, error));
            }
        }
    }

    summary
}

/// Delete source files after successful merge
/// Returns Ok(()) if both files deleted, or Err with details of any failures
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
        assert!(summary.all_success());
    }

    #[test]
    fn test_merge_summary_all_success_false_with_failures() {
        let mut summary = MergeSummary::default();
        summary.failed_count = 1;
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

        let summary = execute_merges(scan_result, dir.path(), OutputFormat::Mkv, 1, false);

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

        let summary = execute_merges(scan_result, dir.path(), OutputFormat::Mkv, 1, false);

        assert_eq!(summary.skipped_count, 5);
        assert_eq!(summary.orphaned_count, 3);
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
