// src/merger.rs
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::scanner::FilePair;
use colored::Colorize;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;

/// Default timeout for ffmpeg process (5 minutes)
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);

/// Extension trait for waiting with timeout
trait ChildExt {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, std::io::Error>;
}

impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, std::io::Error> {
        let start = std::time::Instant::now();

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Result of a single merge operation
#[derive(Debug)]
pub struct MergeResult {
    /// Index of the pair in the original pairs vector
    pub pair_index: usize,
    /// The file pair that was processed
    pub pair_name: String,
    /// Whether the merge succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Summary of all merge operations
#[derive(Debug, Default)]
pub struct MergeSummary {
    /// Number of successful merges
    pub success_count: usize,
    /// Number of failed merges
    pub failed_count: usize,
    /// Number of skipped pairs (aria2)
    pub skipped_count: usize,
    /// Number of orphaned files
    pub orphaned_count: usize,
    /// List of failed merges with errors
    pub failures: Vec<(String, String)>,
}

impl MergeSummary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all operations succeeded
    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }
}

/// Merge a single file pair
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
) -> MergeResult {
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
            println!("{} {}: ffmpeg exited with code {:?}", "✗".red(), pair.stem, status.code());
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
                error: Some(e),
            }
        }
    }
}

/// Run a command with timeout
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus, String> {
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("ffmpeg process timed out after 5 minutes".to_string())
        }
        Err(e) => Err(format!("Failed to wait for ffmpeg: {}", e)),
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