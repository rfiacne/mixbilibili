// src/scanner.rs
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FilePair {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub stem: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    pub pairs: usize,
    pub skipped: usize,
    pub orphaned: usize,
}

#[derive(Debug)]
pub struct ScanResult {
    pub pairs: Vec<FilePair>,
    pub stats: ScanStats,
    #[allow(dead_code)]
    pub skipped_names: Vec<String>,
}

pub fn scan_directory(source_dir: &Path) -> Result<ScanResult> {
    let entries = fs::read_dir(source_dir).context("Failed to read directory")?;

    let mut mp4_files: HashMap<String, PathBuf> = HashMap::new();
    let mut m4a_files: HashMap<String, PathBuf> = HashMap::new();
    let mut aria2_files: HashSet<String> = HashSet::new();

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        if filename.ends_with(".aria2") {
            let base = filename.strip_suffix(".aria2").unwrap();
            aria2_files.insert(base.to_string());
            continue;
        }

        if filename.ends_with(".mp4") {
            let stem = filename.strip_suffix(".mp4").unwrap();
            mp4_files.insert(stem.to_string(), path);
        } else if filename.ends_with(".m4a") {
            let stem = filename.strip_suffix(".m4a").unwrap();
            m4a_files.insert(stem.to_string(), path);
        }
    }

    let mut pairs = Vec::new();
    let mut stats = ScanStats::default();
    let mut skipped_names = Vec::new();
    let mut processed_stems: HashSet<String> = HashSet::new();

    for stem in mp4_files.keys().chain(m4a_files.keys()) {
        if processed_stems.contains(stem) {
            continue;
        }
        processed_stems.insert(stem.clone());

        let has_aria2 = aria2_files.contains(stem)
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

    Ok(ScanResult {
        pairs,
        stats,
        skipped_names,
    })
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
    use std::fs::File;
    use tempfile::tempdir;

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
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_scan_file_not_directory() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notadir.txt");
        File::create(&file_path).unwrap();

        let result = scan_directory(&file_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
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
