use crate::ffmpeg;
use crate::i18n::{t, tf};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Build the composite key for a file: bare stem for non-recursive,
/// "relative_parent/stem" for recursive scans.
pub(crate) fn make_key(recursive: bool, relative_parent: &str, stem: &str) -> String {
    if recursive && !relative_parent.is_empty() {
        format!("{}/{}", relative_parent, stem)
    } else {
        stem.to_string()
    }
}

/// Flatten a composite key "parent/stem" into a safe output filename.
/// Single source of truth for the key → filename rule, shared by merger and workflow.
pub(crate) fn safe_stem(key: &str) -> String {
    key.replace('/', "_")
}

/// Relative-parent string for a child directory of `rel_parent`.
fn child_rel_parent(rel_parent: &str, child_name: &str) -> String {
    if rel_parent.is_empty() {
        child_name.to_string()
    } else {
        let mut out = String::with_capacity(rel_parent.len() + 1 + child_name.len());
        out.push_str(rel_parent);
        out.push('/');
        out.push_str(child_name);
        out
    }
}

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
    pub estimated_duration: Duration,
}

fn is_downloading(stem: &str, aria2_files: &HashSet<String>) -> bool {
    aria2_files.contains(stem)
}

/// Walks `dir` and records candidate files into the keyed maps.
///
/// `rel_parent` is `dir`'s path relative to the scan root (`""` at the root).
/// It is carried down the recursion because it is a property of the *directory*:
/// deriving it per entry cost a `strip_prefix` walk plus an allocation for every
/// file in the tree, which dominated the scan on large directories.
fn collect_files(
    dir: &Path,
    rel_parent: &str,
    mp4_files: &mut HashMap<String, PathBuf>,
    m4a_files: &mut HashMap<String, PathBuf>,
    aria2_files: &mut HashSet<String>,
    recursive: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| t("failed_read_dir"))?;

    for entry in entries {
        // Lazy context: the closure only runs on failure. Building the message
        // eagerly allocated a String for every entry in the tree.
        let entry = entry.with_context(|| t("failed_read_entry"))?;

        // One `file_type()` for the whole entry. Unlike `metadata()` it does not
        // follow symlinks, and unlike `Path::is_dir()` it reuses the type already
        // returned by readdir instead of issuing a second stat() per entry.
        let file_type = entry.file_type()?;

        // Skip symlinks to prevent infinite loops and security issues
        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            if recursive {
                let child = child_rel_parent(rel_parent, &entry.file_name().to_string_lossy());
                collect_files(
                    &entry.path(),
                    &child,
                    mp4_files,
                    m4a_files,
                    aria2_files,
                    true,
                )?;
            }
            continue;
        }

        let name = entry.file_name();
        let Some(filename) = name.to_str() else {
            continue;
        };

        if let Some(stem) = filename.strip_suffix(".aria2") {
            // Normalize: strip .mp4/.m4a suffix so is_downloading needs only one lookup
            let bare = stem
                .strip_suffix(".mp4")
                .or_else(|| stem.strip_suffix(".m4a"))
                .unwrap_or(stem);
            aria2_files.insert(make_key(recursive, rel_parent, bare));
        } else if let Some(stem) = filename.strip_suffix(".mp4") {
            mp4_files.insert(make_key(recursive, rel_parent, stem), entry.path());
        } else if let Some(stem) = filename.strip_suffix(".m4a") {
            m4a_files.insert(make_key(recursive, rel_parent, stem), entry.path());
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
        "",
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
        if is_downloading(key, &aria2_files) {
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

    // Guard against silent overwrite: distinct composite keys (e.g. "a/b" and
    // "a_b") can flatten to the same output filename. The second pair would
    // overwrite the first's output (unrecoverable with --sdel), so reject upfront.
    let mut seen_flat: HashMap<String, String> = HashMap::new();
    for pair in &pairs {
        let flat = safe_stem(&pair.stem);
        if let Some(prev) = seen_flat.insert(flat.clone(), pair.stem.clone()) {
            anyhow::bail!(
                "{}",
                tf("output_name_collision", &[&prev, &pair.stem, &flat])
            );
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
    fn test_is_downloading_with_composite_key() {
        let mut aria2_files = HashSet::new();
        aria2_files.insert("parent/foo".to_string());
        assert!(is_downloading("parent/foo", &aria2_files));
    }

    #[test]
    fn test_is_downloading_with_suffixed_composite_key() {
        // After normalization in collect_files, aria2_files stores composite keys
        let mut aria2_files = HashSet::new();
        aria2_files.insert("parent/foo".to_string()); // was "foo.mp4" before normalization
        assert!(is_downloading("parent/foo", &aria2_files));
    }

    #[test]
    fn test_is_downloading_no_match() {
        let mut aria2_files = HashSet::new();
        aria2_files.insert("bar".to_string());
        aria2_files.insert("baz.mp4".to_string());
        assert!(!is_downloading("foo", &aria2_files));
    }

    #[test]
    fn test_make_key_non_recursive() {
        assert_eq!(make_key(false, "parent", "stem"), "stem");
    }

    #[test]
    fn test_make_key_recursive_with_parent() {
        assert_eq!(make_key(true, "parent", "stem"), "parent/stem");
    }

    #[test]
    fn test_make_key_recursive_root() {
        assert_eq!(make_key(true, "", "stem"), "stem");
    }

    #[test]
    fn test_safe_stem_flattens_composite_key() {
        assert_eq!(safe_stem("parent/stem"), "parent_stem");
        assert_eq!(safe_stem("stem"), "stem");
        assert_eq!(safe_stem("a/b/c"), "a_b_c");
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
    fn test_scan_rejects_output_name_collision() {
        let dir = tempdir().unwrap();

        // "sub/a" and "sub_a" both flatten to "sub_a" → silent overwrite risk
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        for f in ["sub/a.mp4", "sub/a.m4a", "sub_a.mp4", "sub_a.m4a"] {
            File::create(dir.path().join(f)).unwrap();
        }

        let result = scan_directory(dir.path(), true);
        assert!(result.is_err(), "should reject flattened-name collision");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("collision") || err.contains("冲突"),
            "Error should mention collision: {}",
            err
        );
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
    fn test_scan_recursive_nested_keys_use_relative_parent_path() {
        // The composite key decides the output filename, so pin the exact format
        // for multi-level nesting (including a file directly in the scan root).
        let dir = tempdir().unwrap();
        let root = dir.path();

        let season1 = root.join("season1");
        let deeper = season1.join("extra");
        fs::create_dir_all(&deeper).unwrap();

        for (parent, stem) in [
            (root.to_path_buf(), "ep03"),
            (season1.clone(), "ep01"),
            (deeper.clone(), "ep02"),
        ] {
            File::create(parent.join(format!("{stem}.mp4"))).unwrap();
            File::create(parent.join(format!("{stem}.m4a"))).unwrap();
        }

        let result = scan_directory(root, true).unwrap();
        let mut stems: Vec<&str> = result.pairs.iter().map(|p| p.stem.as_str()).collect();
        stems.sort_unstable();

        assert_eq!(
            stems,
            vec!["ep03", "season1/ep01", "season1/extra/ep02"],
            "recursive keys must be the path relative to the scan root"
        );
    }

    #[test]
    fn test_scan_recursive_keys_flatten_to_distinct_output_names() {
        // Guards the pipeline that follows scan: each distinct key must map to a
        // distinct flattened filename, or pairs would overwrite each other.
        let dir = tempdir().unwrap();
        let root = dir.path();

        let nested = root.join("a").join("b");
        let x_y = root.join("x_y");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&x_y).unwrap();
        for (parent, stem) in [
            (root.join("a"), "b"),
            (nested.clone(), "c"),
            (x_y.clone(), "c"),
        ] {
            File::create(parent.join(format!("{stem}.mp4"))).unwrap();
            File::create(parent.join(format!("{stem}.m4a"))).unwrap();
        }

        let result = scan_directory(root, true).unwrap();
        let mut flat: Vec<String> = result.pairs.iter().map(|p| safe_stem(&p.stem)).collect();
        flat.sort();
        flat.dedup();

        assert_eq!(flat.len(), result.pairs.len(), "flat names must be unique");
        assert_eq!(flat, vec!["a_b", "a_b_c", "x_y_c"]);
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
        std::os::windows::fs::symlink_dir(path, path.join("loop_link")).unwrap();

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
        std::os::windows::fs::symlink_dir(external.path(), path.join("external_link")).unwrap();

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
        collect_files(path, "", &mut mp4, &mut m4a, &mut aria2, false).unwrap();

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
        collect_files(path, "", &mut mp4, &mut m4a, &mut aria2, false).unwrap();

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
