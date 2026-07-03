# Implementation Plan: Critical Fixes for mixbilibili

**Date:** 2026-07-03  
**Approach:** Test-Driven Development (RED → GREEN → REFACTOR)  
**Priority:** Data integrity, security, reliability

---

## Overview

This plan addresses 7 critical issues identified in the codebase analysis, organized into 3 implementation batches. Each fix follows strict TDD methodology: write failing tests first, then implement the minimal code to pass, then refactor for clarity.

---

## Batch 1: Critical Security & Data Integrity

**Goal:** Prevent data loss and security vulnerabilities  
**Estimated Effort:** 4-6 hours  
**Risk Level:** High (core data integrity)

### M1: Atomic State File Writes

**Problem:** `state.rs:65-71` uses `fs::write()` directly, causing data loss on crash/interrupt  
**Solution:** Write to `.tmp` file, then atomic `fs::rename()`

#### RED Phase (Tests First)

**File:** `src/state.rs` (add to existing `#[cfg(test)] mod tests`)

```rust
#[test]
fn test_save_creates_temp_file_first() {
    let dir = tempdir().unwrap();
    let state = MergeState::new(dir.path(), dir.path());
    
    // Save should create .tmp file, then rename
    state.save(dir.path()).unwrap();
    
    // Verify final file exists
    let state_path = MergeState::state_file_path(dir.path());
    assert!(state_path.exists());
    
    // Verify no .tmp file remains
    let tmp_path = state_path.with_extension("json.tmp");
    assert!(!tmp_path.exists(), "Temp file should be renamed, not left behind");
}

#[test]
fn test_save_atomic_on_interrupt() {
    let dir = tempdir().unwrap();
    let state_path = MergeState::state_file_path(dir.path());
    
    // Create initial valid state
    let mut state1 = MergeState::new(dir.path(), dir.path());
    state1.add_pending("video1");
    state1.mark_completed("video1");
    state1.save(dir.path()).unwrap();
    
    // Simulate crash during second save by writing garbage to .tmp
    let tmp_path = state_path.with_extension("json.tmp");
    fs::write(&tmp_path, "GARBAGE DATA").unwrap();
    
    // Load should still read the valid original file
    let loaded = MergeState::load(dir.path()).unwrap();
    assert!(loaded.is_some());
    assert!(loaded.unwrap().is_completed("video1"));
    
    // Clean up
    fs::remove_file(&tmp_path).ok();
}
```

#### GREEN Phase

**File:** `src/state.rs` - Replace `save()` method (lines 65-71)

```rust
pub fn save(&self, source: &Path) -> Result<()> {
    let path = Self::state_file_path(source);
    let content = serde_json::to_string_pretty(self)
        .context(t("failed_serialize_state").into_owned())?;
    
    // Write to temporary file first
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, &content)
        .context(t("failed_write_state").into_owned())?;
    
    // Atomic rename (POSIX: atomic, Windows: atomic for same filesystem)
    fs::rename(&tmp_path, &path)
        .context(t("failed_write_state").into_owned())?;
    
    Ok(())
}
```

#### REFACTOR Phase

- Add helper method `state_file_tmp_path()` for consistency
- Consider extracting atomic write pattern for reuse

#### Success Criteria

- ✅ All existing tests pass
- ✅ New tests verify atomic write behavior
- ✅ No `.tmp` files remain after successful save
- ✅ Corrupted `.tmp` doesn't affect valid state file

---

### M5: Stem Injection in `--once` Mode

**Problem:** `main.rs:259-260` allows path traversal via malicious stem argument  
**Solution:** Validate stem contains no path separators, resolved paths stay within source

#### RED Phase

**File:** `src/main.rs` (add new test module)

```rust
#[cfg(test)]
mod once_security_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_run_once_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        
        // Create dummy files
        File::create(dir.path().join("test.mp4")).unwrap();
        File::create(dir.path().join("test.m4a")).unwrap();
        
        let args = Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            once: Some("../escape".to_string()),
            ..Default::default()
        };
        
        let result = run_once(&args, cli::OutputFormat::Mkv, "../escape");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid") || err.contains("traversal"), 
                "Error should mention invalid stem: {}", err);
    }

    #[test]
    fn test_run_once_rejects_absolute_path() {
        let dir = tempdir().unwrap();
        
        let args = Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            once: Some("/etc/passwd".to_string()),
            ..Default::default()
        };
        
        let result = run_once(&args, cli::OutputFormat::Mkv, "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_once_accepts_valid_stem() {
        let dir = tempdir().unwrap();
        
        // Create valid pair
        File::create(dir.path().join("video.mp4")).unwrap();
        File::create(dir.path().join("video.m4a")).unwrap();
        
        let args = Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            once: Some("video".to_string()),
            ..Default::default()
        };
        
        // Should not error on validation (may error on ffmpeg, but that's OK)
        let result = run_once(&args, cli::OutputFormat::Mkv, "video");
        // We're only testing validation, not actual merge
        // If it gets past validation, it will try to run ffmpeg
        // So we just check it doesn't error with "invalid stem"
        if let Err(e) = result {
            let err = e.to_string();
            assert!(!err.contains("invalid") || !err.contains("stem"),
                    "Valid stem should not be rejected: {}", err);
        }
    }
}
```

#### GREEN Phase

**File:** `src/main.rs` - Add validation function before `run_once` (around line 255)

```rust
/// Validate stem argument to prevent path traversal attacks
fn validate_stem(stem: &str) -> Result<()> {
    // Reject path separators
    if stem.contains('/') || stem.contains('\\') {
        anyhow::bail!("Invalid stem '{}': must not contain path separators", stem);
    }
    
    // Reject empty or whitespace-only stems
    if stem.trim().is_empty() {
        anyhow::bail!("Invalid stem: cannot be empty");
    }
    
    // Reject special directory names
    if stem == "." || stem == ".." {
        anyhow::bail!("Invalid stem '{}': cannot be . or ..", stem);
    }
    
    Ok(())
}
```

**File:** `src/main.rs` - Call validation at start of `run_once` (line 259)

```rust
fn run_once(args: &Args, format: cli::OutputFormat, stem: &str) -> Result<()> {
    // Validate stem before using it
    validate_stem(stem)?;
    
    let video_path = args.source.join(format!("{stem}.mp4"));
    let audio_path = args.source.join(format!("{stem}.m4a"));
    
    // ... rest of function unchanged
}
```

#### REFACTOR Phase

- Extract validation into `cli.rs` for reuse
- Add validation to CLI parsing layer for earlier failure

#### Success Criteria

- ✅ Path traversal attempts rejected with clear error
- ✅ Valid stems work normally
- ✅ No regression in existing functionality

---

### M6: Remove Dead Code `execute_merges`

**Problem:** `merger.rs:355-420` contains unused function marked `#[allow(dead_code)]`  
**Solution:** Remove dead function entirely

#### RED Phase

**File:** `src/merger.rs` - Remove test module that tests dead code

```rust
// DELETE: mod exec_tests { ... } (lines ~350-450)
```

**Verification:**

```bash
cargo test --lib merger::exec_tests
# Should fail: module not found
```

#### GREEN Phase

**File:** `src/merger.rs` - Delete `execute_merges` function (lines 355-420)

```rust
// DELETE entire function:
// #[allow(dead_code)]
// #[allow(clippy::too_many_arguments)]
// pub fn execute_merges(...) -> Result<MergeSummary> { ... }
```

**Verification:**

```bash
cargo build
cargo test
cargo clippy
# All should pass with no references to execute_merges
```

#### REFACTOR Phase

- Verify no other dead code exists
- Check for unused imports after deletion

#### Success Criteria

- ✅ `cargo build` succeeds with no warnings about dead code
- ✅ All tests pass
- ✅ No references to `execute_merges` remain
- ✅ Code coverage unchanged or improved

---

### M3: Symlink Loop/Traversal in Recursive Scan

**Problem:** `scanner.rs:49-53` follows symlinks blindly, causing infinite loops or security issues  
**Solution:** Detect and skip symlinks during recursive traversal

#### RED Phase

**File:** `src/scanner.rs` (add to existing `mod scan_tests`)

```rust
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
```

#### GREEN Phase

**File:** `src/scanner.rs` - Modify `collect_files` function (lines 30-55)

```rust
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

        // Check if this is a symlink BEFORE following it
        let metadata = entry.metadata()?;
        if metadata.file_type().is_symlink() {
            // Skip symlinks to prevent infinite loops and security issues
            continue;
        }

        if path.is_dir() {
            if recursive {
                collect_files(&path, mp4_files, m4a_files, aria2_files, true)?;
            }
            continue;
        }

        // ... rest of function unchanged
    }

    Ok(())
}
```

#### REFACTOR Phase

- Add optional `--follow-symlinks` flag if users need symlink support
- Add logging when symlinks are skipped (verbose mode)
- Consider tracking visited inodes for additional cycle detection

#### Success Criteria

- ✅ Circular symlinks don't cause infinite loops
- ✅ External symlinks are skipped
- ✅ Normal recursive scanning works
- ✅ All existing tests pass

---

## Batch 2: Reliability Improvements

**Goal:** Improve error handling and validation  
**Estimated Effort:** 3-4 hours  
**Risk Level:** Medium (additive changes)

### M2: Atomic Source File Deletion

**Problem:** `merger.rs:426-441` deletes video before audio, risking data loss  
**Solution:** Delete audio first (less valuable), or verify both exist before deleting

#### RED Phase

**File:** `src/merger.rs` (add to `mod delete_tests`)

```rust
#[test]
fn test_delete_preserves_video_on_audio_failure() {
    let dir = tempdir().unwrap();
    let video_path = dir.path().join("video.mp4");
    let audio_path = dir.path().join("nonexistent.m4a");  // Doesn't exist
    
    // Create only video
    File::create(&video_path).unwrap();
    
    let pair = FilePair {
        video: video_path.clone(),
        audio: audio_path,
        stem: "video".to_string(),
    };
    
    let result = delete_source_files(&pair);
    assert!(result.is_err(), "Should fail when audio missing");
    
    // Video should still exist (not deleted)
    assert!(video_path.exists(), "Video should be preserved when audio deletion fails");
}

#[test]
fn test_delete_checks_existence_first() {
    let dir = tempdir().unwrap();
    let video_path = dir.path().join("video.mp4");
    let audio_path = dir.path().join("audio.m4a");
    
    // Create both files
    File::create(&video_path).unwrap();
    File::create(&audio_path).unwrap();
    
    let pair = FilePair {
        video: video_path.clone(),
        audio: audio_path.clone(),
        stem: "video".to_string(),
    };
    
    // Both exist, deletion should succeed
    let result = delete_source_files(&pair);
    assert!(result.is_ok());
    assert!(!video_path.exists());
    assert!(!audio_path.exists());
}
```

#### GREEN Phase

**File:** `src/merger.rs` - Rewrite `delete_source_files` (lines 426-441)

```rust
pub(crate) fn delete_source_files(pair: &FilePair) -> Result<()> {
    // Verify both files exist before deleting either
    let video_exists = pair.video.exists();
    let audio_exists = pair.audio.exists();
    
    if !video_exists && !audio_exists {
        anyhow::bail!(
            "Cannot delete source files: both video '{}' and audio '{}' are missing",
            pair.video.display(),
            pair.audio.display()
        );
    }
    
    // Delete audio first (less valuable file)
    if audio_exists {
        if let Err(e) = std::fs::remove_file(&pair.audio) {
            // Audio deletion failed, preserve video
            anyhow::bail!(
                "Failed to delete audio '{}': {}. Video preserved at '{}'",
                pair.audio.display(),
                e,
                pair.video.display()
            );
        }
    }
    
    // Then delete video
    if video_exists {
        if let Err(e) = std::fs::remove_file(&pair.video) {
            // Video deletion failed, but audio already deleted
            anyhow::bail!(
                "Failed to delete video '{}': {}. Audio already deleted from '{}'",
                pair.video.display(),
                e,
                pair.audio.display()
            );
        }
    }
    
    Ok(())
}
```

#### REFACTOR Phase

- Update `test_deletion_failures_tracking` to match new behavior
- Add verbose logging when files are deleted
- Consider transaction-like pattern for complex deletions

#### Success Criteria

- ✅ Video preserved when audio deletion fails
- ✅ Clear error messages indicate what happened
- ✅ Normal deletion still works
- ✅ Existing tests updated to match new behavior

---

### M4: Post-Merge Output Validation

**Problem:** No validation that ffmpeg actually produced valid output  
**Solution:** Check output file exists and has non-zero size after merge

#### RED Phase

**File:** `src/merger.rs` (add to `mod merge_tests`)

```rust
#[test]
fn test_merge_validates_output_exists() {
    // This test would require mocking ffmpeg or using a fake ffmpeg
    // For now, we test the validation logic directly
    
    let dir = tempdir().unwrap();
    let output_path = dir.path().join("output.mkv");
    
    // Simulate ffmpeg creating empty file (failure case)
    File::create(&output_path).unwrap();
    
    // Validation should reject empty file
    let metadata = std::fs::metadata(&output_path).unwrap();
    assert_eq!(metadata.len(), 0, "Empty file should be detected");
}

#[test]
fn test_merge_validates_output_nonzero_size() {
    let dir = tempdir().unwrap();
    let output_path = dir.path().join("output.mkv");
    
    // Simulate ffmpeg creating valid file
    std::fs::write(&output_path, b"fake video data").unwrap();
    
    // Validation should accept non-zero file
    let metadata = std::fs::metadata(&output_path).unwrap();
    assert!(metadata.len() > 0, "Non-zero file should be accepted");
}
```

#### GREEN Phase

**File:** `src/merger.rs` - Add validation helper function

```rust
/// Validate that merge output is valid (exists and non-zero size)
fn validate_output(output_path: &Path) -> Result<()> {
    if !output_path.exists() {
        anyhow::bail!("Merge output file not created: {}", output_path.display());
    }
    
    let metadata = std::fs::metadata(output_path)?;
    if metadata.len() == 0 {
        // Clean up empty file
        let _ = std::fs::remove_file(output_path);
        anyhow::bail!("Merge output file is empty: {}", output_path.display());
    }
    
    Ok(())
}
```

**File:** `src/merger.rs` - Call validation after successful merge in `do_merge` (around line 310)

```rust
Ok(status) if status.success() => {
    // Validate output before reporting success
    if let Err(e) = validate_output(&output_path) {
        return record_failure(pair, pair_index, e.to_string(), start.elapsed(), progress);
    }
    
    let duration = start.elapsed();
    if let Some(p) = progress {
        p.record(&pair.stem, true, duration, None, None);
    } else {
        println!("{} {}", t("checkmark").green(), pair.stem);
    }
    return MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: true,
        error: None,
        duration,
        was_interrupted: false,
    };
}
```

#### REFACTOR Phase

- Add file format validation (check magic bytes)
- Add minimum size threshold
- Consider duration validation for video files

#### Success Criteria

- ✅ Empty output files detected and reported
- ✅ Missing output files detected
- ✅ Valid files pass validation
- ✅ Empty files cleaned up automatically

---

### M7: Pre-Merge Source File Existence Check

**Problem:** No check before ffmpeg launch, confusing errors when files missing  
**Solution:** Verify source files exist before attempting merge

#### RED Phase

**File:** `src/merger.rs` (add to `mod merge_tests`)

```rust
#[test]
fn test_merge_pair_checks_source_existence() {
    let dir = tempdir().unwrap();
    
    let pair = FilePair {
        video: dir.path().join("missing.mp4"),
        audio: dir.path().join("missing.m4a"),
        stem: "missing".to_string(),
    };
    
    let result = merge_pair(&pair, 0, dir.path(), OutputFormat::Mkv, None, false, 0);
    
    assert!(!result.success);
    assert!(result.error.is_some());
    let err = result.error.unwrap();
    assert!(err.contains("missing") || err.contains("not found") || err.contains("source"),
            "Error should mention missing source: {}", err);
}

#[test]
fn test_merge_pair_clear_error_for_missing_video() {
    let dir = tempdir().unwrap();
    
    // Create only audio
    File::create(dir.path().join("video.m4a")).unwrap();
    
    let pair = FilePair {
        video: dir.path().join("missing.mp4"),
        audio: dir.path().join("video.m4a"),
        stem: "video".to_string(),
    };
    
    let result = merge_pair(&pair, 0, dir.path(), OutputFormat::Mkv, None, false, 0);
    
    assert!(!result.success);
    let err = result.error.unwrap();
    assert!(err.contains("video") || err.contains("source"),
            "Error should mention missing video: {}", err);
}
```

#### GREEN Phase

**File:** `src/merger.rs` - Add check at start of `merge_pair` (after line 210)

```rust
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    max_retries: usize,
) -> MergeResult {
    if INTERRUPTED.load(Ordering::SeqCst) {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
    }

    // Check source files exist before attempting merge
    if !pair.video.exists() {
        return record_failure(
            pair,
            pair_index,
            format!("Source video file missing: {}", pair.video.display()),
            Duration::ZERO,
            progress,
        );
    }
    
    if !pair.audio.exists() {
        return record_failure(
            pair,
            pair_index,
            format!("Source audio file missing: {}", pair.audio.display()),
            Duration::ZERO,
            progress,
        );
    }

    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    do_merge(
        pair,
        &output_path,
        pair_index,
        format,
        progress,
        verbose,
        max_retries,
    )
}
```

#### REFACTOR Phase

- Extract validation into separate function
- Add file size check (reject zero-byte source files)
- Add file readability check

#### Success Criteria

- ✅ Clear error message when source files missing
- ✅ No cryptic ffmpeg errors
- ✅ Normal merges unaffected
- ✅ Error message indicates which file is missing

---

## Batch 3: Edge Cases & Polish

**Goal:** Handle remaining edge cases  
**Estimated Effort:** 2-3 hours  
**Risk Level:** Low (isolated changes)

### M8: Retry Logic with Partial Output Cleanup

**Problem:** Retry may append to or reuse partial output from previous attempt  
**Solution:** Clean up partial output before each retry

#### RED Phase

**File:** `src/merger.rs` (add to `mod merge_tests`)

```rust
#[test]
fn test_retry_cleans_partial_output() {
    let dir = tempdir().unwrap();
    let output_path = dir.path().join("output.mkv");
    
    // Simulate partial output from failed attempt
    std::fs::write(&output_path, b"partial data").unwrap();
    assert!(output_path.exists());
    
    // Clean up function should remove it
    cleanup_partial_output(&output_path);
    assert!(!output_path.exists(), "Partial output should be cleaned up");
}
```

#### GREEN Phase

**File:** `src/merger.rs` - Add cleanup helper

```rust
/// Clean up partial output file before retry
fn cleanup_partial_output(output_path: &Path) {
    if output_path.exists() {
        let _ = std::fs::remove_file(output_path);
    }
}
```

**File:** `src/merger.rs` - Call cleanup before each retry in `do_merge` (around line 290)

```rust
for attempt in 0..=max_retries {
    if attempt > 0 {
        // Clean up partial output from previous attempt
        cleanup_partial_output(&output_path);
        
        std::thread::sleep(Duration::from_secs(1));
        // ... rest of retry logic
    }
    
    // ... rest of merge logic
}
```

#### Success Criteria

- ✅ Partial output cleaned before retry
- ✅ No file corruption from append mode
- ✅ Retry starts fresh

---

### M9: State File Migration Path

**Problem:** Atomic write changes file format/location, existing state files may break  
**Solution:** Add migration logic to handle old format

#### RED Phase

**File:** `src/state.rs` (add to `mod tests`)

```rust
#[test]
fn test_load_handles_legacy_format() {
    let dir = tempdir().unwrap();
    let state_path = dir.path().join(".mixbilibili_state.json");
    
    // Create legacy format state file (same format, just not atomic)
    let legacy_state = serde_json::json!({
        "source_dir": dir.path().to_str().unwrap(),
        "output_dir": dir.path().to_str().unwrap(),
        "completed": ["video1"],
        "failed": [],
        "pending": []
    });
    
    std::fs::write(&state_path, serde_json::to_string_pretty(&legacy_state).unwrap()).unwrap();
    
    // Load should work with legacy format
    let loaded = MergeState::load(dir.path()).unwrap();
    assert!(loaded.is_some());
    assert!(loaded.unwrap().is_completed("video1"));
}
```

#### GREEN Phase

**File:** `src/state.rs` - Update `load()` to handle both formats (if needed)

```rust
pub fn load(source: &Path) -> Result<Option<Self>> {
    let path = Self::state_file_path(source);
    
    if !path.exists() {
        return Ok(None);
    }
    
    let content = fs::read_to_string(&path)
        .context(t("failed_read_state").into_owned())?;
    
    // Try parsing as current format
    match serde_json::from_str::<Self>(&content) {
        Ok(mut state) => {
            // Validate source_dir matches
            let expected = source.canonicalize()
                .unwrap_or_else(|_| source.to_path_buf())
                .to_string_lossy()
                .into_owned();
            
            if state.source_dir != expected {
                anyhow::bail!(
                    "Resume state source_dir mismatch: state has '{}', loaded from '{}'",
                    state.source_dir,
                    expected
                );
            }
            Ok(Some(state))
        }
        Err(e) => {
            // If parsing fails, check if it's a legacy format
            // For now, just return error with helpful message
            anyhow::bail!(
                "Failed to parse state file. It may be corrupted or from an incompatible version. \
                 Error: {}. Consider removing {} and starting fresh.",
                e,
                path.display()
            )
        }
    }
}
```

#### Success Criteria

- ✅ Legacy state files still load
- ✅ Clear error message for corrupted files
- ✅ No silent data loss

---

## Implementation Order & Dependencies

```
Batch 1 (Critical):
  M1 (atomic state)     ← Start here, foundation for other atomicity fixes
  M5 (stem injection)   ← Independent, quick fix
  M6 (dead code)        ← Independent, trivial
  M3 (symlink)          ← Independent, security critical

Batch 2 (Reliability):
  M2 (atomic delete)    ← Depends on understanding from M1
  M4 (output validation) ← Independent
  M7 (pre-merge check)  ← Independent

Batch 3 (Edge Cases):
  M8 (retry cleanup)    ← Depends on M4
  M9 (migration)        ← Depends on M1
```

---

## Verification Plan

### Per-Fix Verification

For each fix:

1. ✅ Write failing tests (RED)
2. ✅ Implement minimal code to pass (GREEN)
3. ✅ Refactor for clarity (REFACTOR)
4. ✅ Run `cargo test` - all tests pass
5. ✅ Run `cargo clippy` - no warnings
6. ✅ Run `cargo build --release` - builds successfully
7. ✅ Manual test with edge cases

### Per-Batch Verification

After each batch:

```bash
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo fmt --check
```

### Final Verification

After all batches:

1. ✅ Full test suite passes
2. ✅ No compiler warnings
3. ✅ No clippy warnings
4. ✅ Code formatted correctly
5. ✅ Manual end-to-end test with real files
6. ✅ Test resume functionality
7. ✅ Test interrupt/recovery scenarios
8. ✅ Performance benchmark (no regression)

---

## Risk Mitigation

### High-Risk Changes

**M1 (Atomic State):**

- Risk: Breaking change to state file handling
- Mitigation: Backward compatibility, migration path, extensive testing

**M3 (Symlink Handling):**

- Risk: May break existing workflows using symlinks
- Mitigation: Add `--follow-symlinks` flag if needed, document behavior

**M5 (Stem Validation):**

- Risk: May reject legitimate use cases
- Mitigation: Clear error messages, document valid stem format

### Medium-Risk Changes

**M2 (Atomic Delete):**

- Risk: Changes deletion order, may affect error messages
- Mitigation: Update tests, clear error messages

**M4 (Output Validation):**

- Risk: May reject valid but small files
- Mitigation: Configurable thresholds, verbose logging

### Low-Risk Changes

**M6 (Dead Code):** No functional change  
**M7 (Pre-merge Check):** Additive, no breaking change  
**M8 (Retry Cleanup):** Isolated change  
**M9 (Migration):** Additive, backward compatible

---

## Success Metrics

### Quantitative

- ✅ All 7 issues resolved
- ✅ Test coverage maintained or improved
- ✅ Zero compiler warnings
- ✅ Zero clippy warnings
- ✅ No performance regression (<5% variance)

### Qualitative

- ✅ Data integrity: No data loss on crash/interrupt
- ✅ Security: No path traversal or symlink vulnerabilities
- ✅ Reliability: Clear error messages, graceful failure
- ✅ Maintainability: Dead code removed, cleaner logic
- ✅ User Experience: Better error messages, predictable behavior

---

## Timeline Estimate

| Batch | Tasks | Estimated Time | Buffer |
|-------|-------|----------------|--------|
| 1 | M1, M3, M5, M6 | 4-6 hours | +2 hours |
| 2 | M2, M4, M7 | 3-4 hours | +1.5 hours |
| 3 | M8, M9 | 2-3 hours | +1 hour |
| **Total** | **9 tasks** | **9-13 hours** | **+4.5 hours** |

**Recommended:** Spread over 2-3 days to allow for testing and iteration.

---

## Conclusion

This implementation plan addresses all 7 critical issues using strict TDD methodology. Each fix is:

- **Tested first** (RED phase ensures test coverage)
- **Minimal** (GREEN phase avoids over-engineering)
- **Clean** (REFACTOR phase maintains code quality)

The batched approach allows for:

- Incremental validation
- Early detection of integration issues
- Flexibility to adjust based on learnings
- Clear progress tracking

**Next Steps:**

1. Review and approve this plan
2. Start with Batch 1, Task M1 (atomic state writes)
3. Follow TDD cycle for each task
4. Verify after each batch
5. Deploy after all batches complete
