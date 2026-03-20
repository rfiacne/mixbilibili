// src/scanner.rs
use std::path::PathBuf;

/// A pair of video and audio files to merge
#[derive(Debug, Clone)]
pub struct FilePair {
    /// The video file (.mp4)
    pub video: PathBuf,
    /// The audio file (.m4a)
    pub audio: PathBuf,
    /// The stem (filename without extension)
    pub stem: String,
}

/// Statistics from scanning
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Number of valid pairs found
    pub pairs: usize,
    /// Number of pairs skipped due to aria2 files
    pub skipped: usize,
    /// Number of orphaned files (mp4 or m4a without pair)
    pub orphaned: usize,
}

/// Result of scanning a directory
#[derive(Debug)]
pub struct ScanResult {
    /// Valid file pairs ready for merging
    pub pairs: Vec<FilePair>,
    /// Statistics
    pub stats: ScanStats,
    /// Names of skipped pairs (for aria2)
    pub skipped_names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_pair_debug() {
        let pair = FilePair {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("video.m4a"),
            stem: "video".to_string(),
        };
        let debug_str = format!("{:?}", pair);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_scan_stats_default() {
        let stats = ScanStats::default();
        assert_eq!(stats.pairs, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.orphaned, 0);
    }
}