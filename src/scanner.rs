use crate::ffmpeg;
use crate::i18n::t;
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct FilePair {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub stem: String,
}

#[derive(Debug, Default)]
pub struct ScanStats {
    pub skipped: usize,
    pub orphaned: usize,
}

#[derive(Debug)]
pub struct ScanResult {
    pub pairs: Vec<FilePair>,
    pub stats: ScanStats,
    pub estimated_duration: Duration,
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

        if aria2_files.contains(stem) {
            stats.skipped += 1;
            continue;
        }

        if let (Some(video), Some(audio)) = (mp4_files.get(stem), m4a_files.get(stem)) {
            pairs.push(FilePair {
                video: video.clone(),
                audio: audio.clone(),
                stem: stem.clone(),
            });
        } else {
            stats.orphaned += 1;
        }
    }

    let estimated_duration: std::time::Duration = pairs
        .par_iter()
        .map(|p| ffmpeg::estimate_merge_duration(&p.video))
        .sum();

    Ok(ScanResult {
        pairs,
        stats,
        estimated_duration,
    })
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

    fn pair_paths(path: &Path, stem: &str) {
        File::create(path.join(format!("{stem}.mp4"))).unwrap();
        File::create(path.join(format!("{stem}.m4a"))).unwrap();
    }

    #[test]
    fn test_scan_matches_pairs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        pair_paths(path, "my_video");
        pair_paths(path, "second");

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 2, "should find both matching pairs");
        assert_eq!(result.stats.skipped, 0);
        assert_eq!(result.stats.orphaned, 0);
        let first = result.pairs.iter().find(|p| p.stem == "my_video").unwrap();
        assert!(first.video.ends_with("my_video.mp4"));
        assert!(first.audio.ends_with("my_video.m4a"));
    }

    // Any .aria2 sentinel (bare stem or file-suffix) marks the pair as busy.
    #[test]
    fn test_scan_skips_aria2_downloading_variants() {
        for sentinel in ["busy", "busy.mp4", "busy.m4a"] {
            let dir = tempdir().unwrap();
            let path = dir.path();
            pair_paths(path, "busy");
            pair_paths(path, "complete");
            File::create(path.join(format!("{sentinel}.aria2"))).unwrap();

            let result = scan_directory(path, false).unwrap();
            assert_eq!(result.pairs.len(), 1, "only the busy pair is blocked");
            assert_eq!(result.pairs[0].stem, "complete");
            assert_eq!(
                result.stats.skipped, 1,
                "{sentinel}.aria2 must count skipped"
            );
        }
    }

    #[test]
    fn test_scan_counts_orphaned_either_ext() {
        for ext in ["mp4", "m4a"] {
            let dir = tempdir().unwrap();
            File::create(dir.path().join(format!("lonely.{ext}"))).unwrap();

            let result = scan_directory(dir.path(), false).unwrap();
            assert_eq!(result.pairs.len(), 0, "half pair has no match");
            assert_eq!(result.stats.orphaned, 1, "lonely.{ext} counts as orphaned");
        }
    }

    #[test]
    fn test_scan_recursive_scans_subdirs_only_when_asked() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        pair_paths(path, "top");
        let sub = path.join("nested");
        fs::create_dir(&sub).unwrap();
        pair_paths(&sub, "deep");

        let flat = scan_directory(path, false).unwrap();
        assert_eq!(flat.pairs.len(), 1, "non-recursive sees only top level");
        assert_eq!(flat.stats.orphaned, 0, "subdir files must be invisible");

        let recursive = scan_directory(path, true).unwrap();
        assert_eq!(recursive.pairs.len(), 2, "recursive finds subdir pairs");
        let stems: Vec<&str> = recursive.pairs.iter().map(|p| p.stem.as_str()).collect();
        assert!(stems.contains(&"top"));
        assert!(stems.contains(&"deep"));
    }

    // Estimated duration scales with file size at a fixed 10 MB/s heuristic.
    #[test]
    fn test_scan_estimated_duration_scales_with_size() {
        use std::io::Write;

        let dir = tempdir().unwrap();
        let path = dir.path();

        // Empty pair: zero duration.
        pair_paths(path, "video1");

        // ~10 MB video → 1s, plus a second pair at ~20 MB → 2s. Total 3s.
        File::create(path.join("video2.m4a")).unwrap();
        for (name, mb) in [("video2.mp4", 10), ("video3.mp4", 20)] {
            let mut f = File::create(path.join(name)).unwrap();
            f.write_all(&vec![0u8; mb * 1024 * 1024]).unwrap();
        }
        File::create(path.join("video3.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 3);
        // video1 contributes 0 (empty files), video2 1s, video3 2s.
        assert_eq!(result.estimated_duration.as_secs(), 3);
    }
}
