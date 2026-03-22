// src/scanner.rs
use anyhow::{Result, Context};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A pair of video and audio files to merge.
///
/// Represents matched video (.mp4) and audio (.m4a) files
/// with the same filename stem.
#[derive(Debug, Clone)]
pub struct FilePair {
    /// The video file path (.mp4)
    pub video: PathBuf,
    /// The audio file path (.m4a)
    pub audio: PathBuf,
    /// The filename stem (filename without extension)
    pub stem: String,
}

/// Statistics from directory scanning.
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Number of valid file pairs found
    pub pairs: usize,
    /// Number of pairs skipped due to aria2 control files
    pub skipped: usize,
    /// Number of orphaned files (mp4 or m4a without matching pair)
    pub orphaned: usize,
}

/// Result of scanning a directory for media file pairs.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ScanResult {
    /// Valid file pairs ready for merging
    pub pairs: Vec<FilePair>,
    /// Scanning statistics
    pub stats: ScanStats,
    /// Names of skipped pairs (aria2 downloads in progress)
    pub skipped_names: Vec<String>,
}

/// Scan a directory for matching mp4/m4a file pairs.
///
/// # Arguments
///
/// * `source_dir` - The directory to scan for media files
///
/// # Returns
///
/// A `ScanResult` containing matched pairs and statistics.
///
/// # Errors
///
/// Returns an error if:
/// - The source directory does not exist
/// - The source path is not a directory
/// - The directory cannot be read
///
/// # Note
///
/// Files with non-UTF8 names are silently skipped during scanning.
pub fn scan_directory(source_dir: &Path) -> Result<ScanResult> {
    if !source_dir.exists() {
        anyhow::bail!("Source directory does not exist: {}", source_dir.display());
    }

    if !source_dir.is_dir() {
        anyhow::bail!("Source path is not a directory: {}", source_dir.display());
    }

    // Check read permission
    let entries = fs::read_dir(source_dir)
        .context("Failed to read directory")?;

    // Collect all mp4 and m4a files
    let mut mp4_files: HashMap<String, PathBuf> = HashMap::new();
    let mut m4a_files: HashMap<String, PathBuf> = HashMap::new();
    let mut aria2_files: HashSet<String> = HashSet::new();

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Get filename as string
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        // Check for aria2 control files
        if filename.ends_with(".aria2") {
            let base = filename.strip_suffix(".aria2").unwrap();
            aria2_files.insert(base.to_string());
            continue;
        }

        // Categorize media files
        if filename.ends_with(".mp4") {
            let stem = filename.strip_suffix(".mp4").unwrap();
            mp4_files.insert(stem.to_string(), path);
        } else if filename.ends_with(".m4a") {
            let stem = filename.strip_suffix(".m4a").unwrap();
            m4a_files.insert(stem.to_string(), path);
        }
    }

    // Find pairs and track stats
    let mut pairs = Vec::new();
    let mut stats = ScanStats::default();
    let mut skipped_names = Vec::new();

    let all_stems: HashSet<_> = mp4_files.keys()
        .chain(m4a_files.keys())
        .collect();

    for stem in all_stems {
        let has_aria2 = aria2_files.contains(stem.as_str())
            || aria2_files.contains(&format!("{}.mp4", stem))
            || aria2_files.contains(&format!("{}.m4a", stem));

        if has_aria2 {
            stats.skipped += 1;
            skipped_names.push(stem.clone());
            continue;
        }

        match (mp4_files.get(stem), m4a_files.get(stem)) {
            (Some(video), Some(audio)) => {
                pairs.push(FilePair {
                    video: video.clone(),
                    audio: audio.clone(),
                    stem: stem.clone(),
                });
                stats.pairs += 1;
            }
            _ => {
                stats.orphaned += 1;
            }
        }
    }

    Ok(ScanResult { pairs, stats, skipped_names })
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

#[cfg(test)]
mod scan_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_scan_empty_directory() {
        let dir = tempdir().unwrap();
        let result = scan_directory(dir.path()).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.pairs, 0);
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let result = scan_directory(Path::new("/nonexistent/path/12345"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_scan_file_not_directory() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notadir.txt");
        File::create(&file_path).unwrap();

        let result = scan_directory(&file_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_scan_with_valid_pairs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 2);
        assert_eq!(result.stats.pairs, 2);
        assert_eq!(result.stats.skipped, 0);
        assert_eq!(result.stats.orphaned, 0);
    }

    #[test]
    fn test_scan_skips_aria2_files_stem() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.aria2")).unwrap();

        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.stats.skipped, 1);
        assert!(result.skipped_names.contains(&"video1".to_string()));
    }

    #[test]
    fn test_scan_skips_aria2_files_mp4_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.mp4.aria2")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_skips_aria2_files_m4a_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.m4a.aria2")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_mp4() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        // No matching m4a

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_m4a() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.m4a")).unwrap();
        // No matching mp4

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_ignores_subdirectories() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create subdirectory with files
        let subdir = path.join("subdir");
        fs::create_dir(&subdir).unwrap();
        File::create(subdir.join("video.mp4")).unwrap();
        File::create(subdir.join("video.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
    }
}