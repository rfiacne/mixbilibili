use crate::ffmpeg;
use crate::i18n::t;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FilePair {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub stem: String,
}

#[derive(Debug, Default, Clone)]
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
    pub estimated_duration: Duration,
}

fn is_downloading(stem: &str, aria2_files: &HashSet<String>) -> bool {
    aria2_files.contains(stem)
}

fn collect_files(
    dir: &Path,
    mp4_files: &mut HashMap<String, PathBuf>,
    m4a_files: &mut HashMap<String, PathBuf>,
    aria2_files: &mut HashSet<String>,
    recursive: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir).context(t("failed_read_dir").into_owned())?;

    for entry in entries {
        let entry = entry.context(t("failed_read_entry").into_owned())?;
        let path = entry.path();

        if path.is_dir() {
            if recursive {
                collect_files(&path, mp4_files, m4a_files, aria2_files, true)?;
            }
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        if let Some(stem) = filename.strip_suffix(".aria2") {
            // Normalize: strip .mp4/.m4a suffix so is_downloading needs only one lookup
            let bare = stem
                .strip_suffix(".mp4")
                .or_else(|| stem.strip_suffix(".m4a"))
                .unwrap_or(stem);
            aria2_files.insert(bare.to_string());
        } else if let Some(stem) = filename.strip_suffix(".mp4") {
            mp4_files.insert(stem.to_string(), path);
        } else if let Some(stem) = filename.strip_suffix(".m4a") {
            m4a_files.insert(stem.to_string(), path);
        }
    }

    Ok(())
}

pub fn scan_directory(source_dir: &Path, recursive: bool) -> Result<ScanResult> {
    let mut mp4_files: HashMap<String, PathBuf> = HashMap::new();
    let mut m4a_files: HashMap<String, PathBuf> = HashMap::new();
    let mut aria2_files: HashSet<String> = HashSet::new();

    collect_files(
        source_dir,
        &mut mp4_files,
        &mut m4a_files,
        &mut aria2_files,
        recursive,
    )?;

    let mut pairs = Vec::new();
    let mut stats = ScanStats::default();
    let mut processed_stems: HashSet<String> = HashSet::new();

    for stem in mp4_files.keys().chain(m4a_files.keys()) {
        if !processed_stems.insert(stem.clone()) {
            continue;
        }

        if is_downloading(stem, &aria2_files) {
            stats.skipped += 1;
            continue;
        }

        if let (Some(video), Some(audio)) = (mp4_files.get(stem), m4a_files.get(stem)) {
            pairs.push(FilePair {
                video: video.clone(),
                audio: audio.clone(),
                stem: stem.clone(),
            });
            stats.pairs += 1;
        } else {
            stats.orphaned += 1;
        }
    }

    let estimated_duration: std::time::Duration = pairs
        .iter()
        .map(|p| ffmpeg::estimate_merge_duration(&p.video))
        .sum();

    Ok(ScanResult {
        pairs,
        stats,
        estimated_duration,
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

    #[test]
    fn test_is_downloading_with_bare_stem() {
        let mut aria2_files = HashSet::new();
        aria2_files.insert("foo".to_string());
        assert!(is_downloading("foo", &aria2_files));
    }

    #[test]
    fn test_is_downloading_with_suffixed_stem() {
        // After normalization in collect_files, aria2_files stores bare stems
        let mut aria2_files = HashSet::new();
        aria2_files.insert("foo".to_string()); // was "foo.mp4" before normalization
        assert!(is_downloading("foo", &aria2_files));
    }

    #[test]
    fn test_is_downloading_no_match() {
        let mut aria2_files = HashSet::new();
        aria2_files.insert("bar".to_string());
        aria2_files.insert("baz.mp4".to_string());
        assert!(!is_downloading("foo", &aria2_files));
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
        let result = scan_directory(dir.path(), false).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.pairs, 0);
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let result = scan_directory(Path::new("/nonexistent/path/12345"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_file_not_directory() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notadir.txt");
        File::create(&file_path).unwrap();

        let result = scan_directory(&file_path, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_with_valid_pairs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
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

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_skips_aria2_files_mp4_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.mp4.aria2")).unwrap();

        let result = scan_directory(path, false).unwrap();
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

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_mp4() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_m4a() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_ignores_subdirectories_when_not_recursive() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let subdir = path.join("subdir");
        fs::create_dir(&subdir).unwrap();
        File::create(subdir.join("video.mp4")).unwrap();
        File::create(subdir.join("video.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0);
    }

    #[test]
    fn test_scan_finds_pairs_in_subdirectories_when_recursive() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let subdir = path.join("subdir");
        fs::create_dir(&subdir).unwrap();
        File::create(subdir.join("video.mp4")).unwrap();
        File::create(subdir.join("video.m4a")).unwrap();

        let result = scan_directory(path, true).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.stats.pairs, 1);
    }

    #[test]
    fn test_scan_recursive_nested_subdirectories() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let subdir1 = path.join("season1");
        let subdir2 = path.join("season2");
        fs::create_dir(&subdir1).unwrap();
        fs::create_dir(&subdir2).unwrap();

        File::create(subdir1.join("ep01.mp4")).unwrap();
        File::create(subdir1.join("ep01.m4a")).unwrap();
        File::create(subdir2.join("ep02.mp4")).unwrap();
        File::create(subdir2.join("ep02.m4a")).unwrap();

        let result = scan_directory(path, true).unwrap();
        assert_eq!(result.pairs.len(), 2);
    }

    #[test]
    fn test_scan_directory_matches_mp4_with_m4a() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("my_video.mp4")).unwrap();
        File::create(path.join("my_video.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 1, "should find one matching pair");
        assert_eq!(result.pairs[0].stem, "my_video");
        assert!(result.pairs[0].video.ends_with("my_video.mp4"));
        assert!(result.pairs[0].audio.ends_with("my_video.m4a"));
        assert_eq!(result.stats.pairs, 1);
        assert_eq!(result.stats.orphaned, 0);
    }

    #[test]
    fn test_scan_directory_orphaned_video_only() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("lonely_video.mp4")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0, "no pairs without matching audio");
        assert_eq!(
            result.stats.orphaned, 1,
            "video-only should count as orphaned"
        );
    }

    #[test]
    fn test_scan_directory_skips_aria2_downloading() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // This pair has an active .aria2 download — should be skipped
        File::create(path.join("downloading.mp4")).unwrap();
        File::create(path.join("downloading.m4a")).unwrap();
        File::create(path.join("downloading.aria2")).unwrap();

        // This pair is complete — should be found
        File::create(path.join("complete.mp4")).unwrap();
        File::create(path.join("complete.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(
            result.pairs.len(),
            1,
            "only non-downloading pair should be found"
        );
        assert_eq!(result.pairs[0].stem, "complete");
        assert_eq!(
            result.stats.skipped, 1,
            "downloading pair should be skipped"
        );
        assert_eq!(result.stats.pairs, 1);
    }

    #[test]
    fn test_scan_directory_recursive_finds_subdirs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Top-level pair
        File::create(path.join("top.mp4")).unwrap();
        File::create(path.join("top.m4a")).unwrap();

        // Nested pair
        let sub = path.join("nested");
        fs::create_dir(&sub).unwrap();
        File::create(sub.join("deep.mp4")).unwrap();
        File::create(sub.join("deep.m4a")).unwrap();

        // Non-recursive: only top-level pair
        let result_flat = scan_directory(path, false).unwrap();
        assert_eq!(
            result_flat.pairs.len(),
            1,
            "non-recursive should find only top-level"
        );

        // Recursive: both pairs
        let result_recursive = scan_directory(path, true).unwrap();
        assert_eq!(
            result_recursive.pairs.len(),
            2,
            "recursive should find pairs in subdirs"
        );
        let stems: Vec<&str> = result_recursive
            .pairs
            .iter()
            .map(|p| p.stem.as_str())
            .collect();
        assert!(stems.contains(&"top"));
        assert!(stems.contains(&"deep"));
    }

    #[test]
    fn test_scan_includes_estimated_duration() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        // Empty files → 0 duration
        assert_eq!(result.estimated_duration.as_secs(), 0);
    }

    #[test]
    fn test_scan_estimated_duration_with_data() {
        use std::io::Write;

        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create ~10 MB video file
        let mut video1 = File::create(path.join("video1.mp4")).unwrap();
        video1.write_all(&vec![0u8; 10 * 1024 * 1024]).unwrap();
        File::create(path.join("video1.m4a")).unwrap();

        // Create ~20 MB video file
        let mut video2 = File::create(path.join("video2.mp4")).unwrap();
        video2.write_all(&vec![0u8; 20 * 1024 * 1024]).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        // 10 MB / 10 MB/s = 1s, 20 MB / 10 MB/s = 2s, total = 3s
        assert_eq!(result.estimated_duration.as_secs(), 3);
    }
}
