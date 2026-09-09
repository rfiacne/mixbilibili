use crate::cli::OutputFormat;
use crate::i18n::{t, tf};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A video/audio pair, identified by its stem.
///
/// This module owns the naming convention: which suffixes pair up, how the
/// aria2 control file is spelled, and where merged output belongs. Everything
/// else asks for paths instead of formatting them.
#[derive(Debug, Clone)]
pub struct FilePair {
    pub video: PathBuf,
    pub audio: PathBuf,
    /// Bare stem in a flat scan; a composite "subdir/stem" when recursive, so
    /// same-named files in different directories stay distinct.
    pub stem: String,
}

impl FilePair {
    pub const VIDEO_SUFFIX: &'static str = ".mp4";
    pub const AUDIO_SUFFIX: &'static str = ".m4a";
    /// aria2 leaves `<name>.<ext>.aria2` next to an in-flight download.
    pub const ARIA2_SUFFIX: &'static str = ".aria2";

    pub fn video_path(dir: &Path, stem: &str) -> PathBuf {
        dir.join(format!("{stem}{}", Self::VIDEO_SUFFIX))
    }

    pub fn audio_path(dir: &Path, stem: &str) -> PathBuf {
        dir.join(format!("{stem}{}", Self::AUDIO_SUFFIX))
    }

    /// The pair for `stem` in `dir`, or an error naming the source missing.
    pub fn sources_in(dir: &Path, stem: &str) -> Result<Self> {
        let video = Self::video_path(dir, stem);
        let audio = Self::audio_path(dir, stem);
        if !video.is_file() {
            anyhow::bail!("{}", tf("missing_video", &[&video.display().to_string()]));
        }
        if !audio.is_file() {
            anyhow::bail!("{}", tf("missing_audio", &[&audio.display().to_string()]));
        }
        Ok(Self {
            video,
            audio,
            stem: stem.to_string(),
        })
    }

    /// A composite key is flattened into one file name; the directory
    /// structure is not reproduced under the output directory.
    pub fn output_name(&self, format: OutputFormat) -> String {
        format!("{}.{}", self.stem.replace('/', "_"), format.extension())
    }

    pub fn output_path(&self, dir: &Path, format: OutputFormat) -> PathBuf {
        dir.join(self.output_name(format))
    }
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
}

fn collect_files(
    dir: &Path,
    scan_root: &Path,
    mp4_files: &mut HashMap<String, PathBuf>,
    m4a_files: &mut HashMap<String, PathBuf>,
    aria2_files: &mut HashSet<String>,
    recursive: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir).context(t("failed_read_dir").into_owned())?;

    for entry in entries {
        let entry = entry.context(t("failed_read_entry").into_owned())?;
        let path = entry.path();

        // Check if this is a symlink BEFORE following it
        // Use file_type() which does NOT follow symlinks (unlike metadata())
        if entry.file_type()?.is_symlink() {
            // Skip symlinks to prevent infinite loops and security issues
            continue;
        }

        if path.is_dir() {
            if recursive {
                collect_files(&path, scan_root, mp4_files, m4a_files, aria2_files, true)?;
            }
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        // Build key: bare stem for non-recursive, relative path prefix for recursive
        let make_key = |stem: &str| -> String {
            if recursive {
                let relative_parent = path
                    .parent()
                    .and_then(|p| p.strip_prefix(scan_root).ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if relative_parent.is_empty() {
                    stem.to_string()
                } else {
                    format!("{}/{}", relative_parent, stem)
                }
            } else {
                stem.to_string()
            }
        };

        if let Some(stem) = filename.strip_suffix(FilePair::ARIA2_SUFFIX) {
            // Normalize: strip the source suffix so one lookup covers
            // "foo.aria2", "foo.mp4.aria2" and "foo.m4a.aria2".
            let bare = stem
                .strip_suffix(FilePair::VIDEO_SUFFIX)
                .or_else(|| stem.strip_suffix(FilePair::AUDIO_SUFFIX))
                .unwrap_or(stem);
            aria2_files.insert(make_key(bare));
        } else if let Some(stem) = filename.strip_suffix(FilePair::VIDEO_SUFFIX) {
            mp4_files.insert(make_key(stem), path);
        } else if let Some(stem) = filename.strip_suffix(FilePair::AUDIO_SUFFIX) {
            m4a_files.insert(make_key(stem), path);
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
        source_dir,
        &mut mp4_files,
        &mut m4a_files,
        &mut aria2_files,
        recursive,
    )?;

    let mut pairs = Vec::new();
    let mut stats = ScanStats::default();

    // Collect and sort keys for deterministic output order
    let mut all_keys: Vec<&String> = mp4_files.keys().chain(m4a_files.keys()).collect();
    all_keys.sort();
    all_keys.dedup();

    for key in all_keys {
        if aria2_files.contains(key) {
            stats.skipped += 1;
            continue;
        }

        if let (Some(video), Some(audio)) = (mp4_files.get(key), m4a_files.get(key)) {
            pairs.push(FilePair {
                video: video.clone(),
                audio: audio.clone(),
                stem: key.clone(),
            });
            stats.pairs += 1;
        } else {
            stats.orphaned += 1;
        }
    }

    Ok(ScanResult { pairs, stats })
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
}

#[cfg(test)]
mod naming_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn source_paths_follow_the_convention() {
        let dir = Path::new("/downloads");
        assert_eq!(
            FilePair::video_path(dir, "ep01"),
            PathBuf::from("/downloads/ep01.mp4")
        );
        assert_eq!(
            FilePair::audio_path(dir, "ep01"),
            PathBuf::from("/downloads/ep01.m4a")
        );
    }

    #[test]
    fn output_path_flattens_a_composite_key() {
        let pair = FilePair {
            video: PathBuf::from("/in/season1/ep01.mp4"),
            audio: PathBuf::from("/in/season1/ep01.m4a"),
            stem: "season1/ep01".to_string(),
        };
        // One file name, not a recreated directory tree.
        assert_eq!(
            pair.output_path(Path::new("/out"), OutputFormat::Mkv),
            PathBuf::from("/out/season1_ep01.mkv")
        );
    }

    #[test]
    fn sources_in_finds_a_complete_pair() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("ep01.mp4")).unwrap();
        File::create(dir.path().join("ep01.m4a")).unwrap();

        let pair = FilePair::sources_in(dir.path(), "ep01").unwrap();
        assert_eq!(pair.stem, "ep01");
        assert_eq!(pair.video, dir.path().join("ep01.mp4"));
        assert_eq!(pair.audio, dir.path().join("ep01.m4a"));
    }

    #[test]
    fn sources_in_names_the_missing_video() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("ep01.m4a")).unwrap();

        let err = FilePair::sources_in(dir.path(), "ep01")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ep01.mp4"),
            "should name the video file: {err}"
        );
    }

    #[test]
    fn sources_in_names_the_missing_audio() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("ep01.mp4")).unwrap();

        let err = FilePair::sources_in(dir.path(), "ep01")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ep01.m4a"),
            "should name the audio file: {err}"
        );
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
        // stems are composite keys in recursive mode: "subdir/stem"
        // Top-level files have bare stem (empty relative parent)
        assert!(stems.contains(&"top"));
        assert!(stems.iter().any(|s| s.ends_with("/deep")));
    }

    /// Windows symlinks need developer mode / elevated privileges; skip
    /// rather than fail when the OS refuses (error 1314).
    #[cfg(windows)]
    fn symlink_dir_supported(from: &std::path::Path, to: &std::path::Path) -> bool {
        std::os::windows::fs::symlink_dir(from, to).is_ok()
    }

    #[test]
    fn test_scan_symlink_loop_does_not_infinite_loop() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create a valid pair
        File::create(path.join("video.mp4")).unwrap();
        File::create(path.join("video.m4a")).unwrap();

        // Create symlink pointing to parent directory (circular)
        #[cfg(unix)]
        std::os::unix::fs::symlink(path, path.join("loop_link")).unwrap();

        #[cfg(windows)]
        {
            if !symlink_dir_supported(path, &path.join("loop_link")) {
                eprintln!("Skipping: Windows symlink creation needs developer mode");
                return;
            }
        }

        // Should complete without hanging or stack overflow
        let result = scan_directory(path, true);
        assert!(result.is_ok());

        // Should find the valid pair
        assert_eq!(result.unwrap().pairs.len(), 1);
    }

    #[test]
    fn test_scan_skips_symlinks_to_external_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create external directory with files
        let external = tempdir().unwrap();
        File::create(external.path().join("external.mp4")).unwrap();
        File::create(external.path().join("external.m4a")).unwrap();

        // Create symlink to external directory
        #[cfg(unix)]
        std::os::unix::fs::symlink(external.path(), path.join("external_link")).unwrap();

        #[cfg(windows)]
        {
            if !symlink_dir_supported(external.path(), &path.join("external_link")) {
                eprintln!("Skipping: Windows symlink creation needs developer mode");
                return;
            }
        }

        // Create valid pair in main directory
        File::create(path.join("video.mp4")).unwrap();
        File::create(path.join("video.m4a")).unwrap();

        let result = scan_directory(path, true).unwrap();

        // Should only find the local pair, not external files
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.pairs[0].stem, "video");
    }

    #[test]
    fn test_scan_pairs_correct_matching() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();

        // Verify each video is paired with the correct audio by stem
        for pair in &result.pairs {
            let video_stem = pair.video.file_stem().unwrap().to_str().unwrap();
            let audio_stem = pair.audio.file_stem().unwrap().to_str().unwrap();
            assert_eq!(
                video_stem,
                audio_stem,
                "Video and audio stems must match: video={}, audio={}",
                pair.video.display(),
                pair.audio.display()
            );
            // In non-recursive mode, pair.stem is the bare stem
            assert_eq!(
                pair.stem, video_stem,
                "Pair stem must equal video stem in non-recursive mode"
            );
        }
    }

    #[test]
    fn test_scan_recursive_same_stem_different_subdirs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create same-named files in different subdirectories
        let season1 = path.join("season1");
        let season2 = path.join("season2");
        fs::create_dir(&season1).unwrap();
        fs::create_dir(&season2).unwrap();

        File::create(season1.join("ep01.mp4")).unwrap();
        File::create(season1.join("ep01.m4a")).unwrap();
        File::create(season2.join("ep01.mp4")).unwrap();
        File::create(season2.join("ep01.m4a")).unwrap();

        let result = scan_directory(path, true).unwrap();
        assert_eq!(
            result.pairs.len(),
            2,
            "should find both pairs with same stem in different subdirs"
        );

        // Verify each pair has video and audio from the same subdirectory
        for pair in &result.pairs {
            let video_parent = pair.video.parent().unwrap();
            let audio_parent = pair.audio.parent().unwrap();
            assert_eq!(
                video_parent,
                audio_parent,
                "Video and audio must be from same directory: video={}, audio={}",
                pair.video.display(),
                pair.audio.display()
            );
            // stem is now a composite key: "season1/ep01" or "season2/ep01"
            assert!(
                pair.stem == "season1/ep01" || pair.stem == "season2/ep01",
                "Pair stem should be composite key: {}",
                pair.stem
            );
        }
    }

    #[test]
    fn test_scan_recursive_same_stem_with_aria2() {
        // Test that aria2 detection works correctly with composite keys in recursive mode
        // season1/ep01 has .aria2 -> should be skipped
        // season2/ep01 has no .aria2 -> should be paired
        let dir = tempdir().unwrap();
        let path = dir.path();

        let season1 = path.join("season1");
        let season2 = path.join("season2");
        fs::create_dir(&season1).unwrap();
        fs::create_dir(&season2).unwrap();

        // season1/ep01 is downloading
        File::create(season1.join("ep01.mp4")).unwrap();
        File::create(season1.join("ep01.m4a")).unwrap();
        File::create(season1.join("ep01.mp4.aria2")).unwrap();

        // season2/ep01 is complete
        File::create(season2.join("ep01.mp4")).unwrap();
        File::create(season2.join("ep01.m4a")).unwrap();

        let result = scan_directory(path, true).unwrap();
        assert_eq!(result.pairs.len(), 1, "only season2/ep01 should be paired");
        assert_eq!(result.stats.skipped, 1, "season1/ep01 should be skipped");
        assert_eq!(result.pairs[0].stem, "season2/ep01");
    }
}

#[cfg(test)]
mod collect_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_collect_ignores_unrelated_extensions() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Non-media files should be silently ignored
        File::create(path.join("readme.txt")).unwrap();
        File::create(path.join("data.json")).unwrap();
        File::create(path.join("script.sh")).unwrap();
        File::create(path.join("video.mkv")).unwrap(); // Not .mp4

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 0);
        assert_eq!(result.stats.skipped, 0);
    }

    #[test]
    fn test_collect_case_sensitive_extensions() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Uppercase extensions should NOT match (Unix is case-sensitive)
        File::create(path.join("video.MP4")).unwrap();
        File::create(path.join("video.M4A")).unwrap();

        let result = scan_directory(path, false).unwrap();
        // These should not be recognized as valid media pairs
        assert_eq!(result.stats.pairs, 0);
    }

    #[test]
    fn test_collect_stem_with_dots() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Filenames with dots in stem: "ep.01.final.mp4"
        File::create(path.join("ep.01.final.mp4")).unwrap();
        File::create(path.join("ep.01.final.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.pairs[0].stem, "ep.01.final");
    }

    #[test]
    fn test_collect_stem_with_spaces_and_unicode() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("第1集 正片.mp4")).unwrap();
        File::create(path.join("第1集 正片.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.pairs[0].stem, "第1集 正片");
    }

    #[test]
    fn test_collect_aria2_with_mp4_suffix_blocks_pair() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // "video.mp4.aria2" means video.mp4 is still downloading
        File::create(path.join("video.mp4")).unwrap();
        File::create(path.join("video.m4a")).unwrap();
        File::create(path.join("video.mp4.aria2")).unwrap();

        let mut mp4 = HashMap::new();
        let mut m4a = HashMap::new();
        let mut aria2 = HashSet::new();
        collect_files(path, path, &mut mp4, &mut m4a, &mut aria2, false).unwrap();

        // In non-recursive mode, aria2 set uses bare stem
        assert!(
            aria2.contains("video"),
            "aria2 should contain bare key: video"
        );
    }

    #[test]
    fn test_collect_aria2_with_m4a_suffix_blocks_pair() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("audio.mp4")).unwrap();
        File::create(path.join("audio.m4a")).unwrap();
        File::create(path.join("audio.m4a.aria2")).unwrap();

        let mut mp4 = HashMap::new();
        let mut m4a = HashMap::new();
        let mut aria2 = HashSet::new();
        collect_files(path, path, &mut mp4, &mut m4a, &mut aria2, false).unwrap();

        // In non-recursive mode, aria2 set uses bare stem
        assert!(
            aria2.contains("audio"),
            "aria2 should contain bare key: audio"
        );
    }

    #[test]
    fn test_collect_multiple_pairs_deterministic_order() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create pairs in reverse alphabetical order
        for name in ["zebra", "alpha", "middle"] {
            File::create(path.join(format!("{}.mp4", name))).unwrap();
            File::create(path.join(format!("{}.m4a", name))).unwrap();
        }

        let result = scan_directory(path, false).unwrap();
        let stems: Vec<&str> = result.pairs.iter().map(|p| p.stem.as_str()).collect();

        // Non-recursive: bare stems, sorted alphabetically
        assert_eq!(stems, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn test_collect_empty_subdir_non_recursive() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        fs::create_dir(path.join("empty_sub")).unwrap();
        File::create(path.join("video.mp4")).unwrap();
        File::create(path.join("video.m4a")).unwrap();

        let result = scan_directory(path, false).unwrap();
        assert_eq!(result.pairs.len(), 1);
    }
}
