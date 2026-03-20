// src/merger.rs
use std::path::PathBuf;

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