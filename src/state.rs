use crate::i18n::t;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn set_as_vec<S: Serializer>(set: &HashSet<String>, s: S) -> Result<S::Ok, S::Error> {
    let mut vec: Vec<&String> = set.iter().collect();
    vec.sort();
    vec.serialize(s)
}

fn vec_as_set<'de, D: Deserializer<'de>>(d: D) -> Result<HashSet<String>, D::Error> {
    let vec: Vec<String> = Vec::deserialize(d)?;
    Ok(vec.into_iter().collect())
}

/// Schema version of state files written before versioning existed.
/// Files without a `version` field deserialize to this value via
/// `default_schema_version`, so they migrate forward like any v1 file.
pub const LEGACY_SCHEMA_VERSION: u32 = 1;

/// Current schema version. Bump together with a migration entry in
/// `migrations()`; `load()` still rejects any file with a newer version.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

fn default_schema_version() -> u32 {
    LEGACY_SCHEMA_VERSION
}

/// A schema migration: `(from_version, migrate_fn)`. `migrate_fn` only reshapes
/// data; [`apply_migrations`] owns the version bump to `from_version + 1`.
type Migration = fn(&mut MergeState);

/// Ordered migration chain. Index `v - LEGACY_SCHEMA_VERSION` upgrades v → v+1.
fn migrations() -> &'static [(u32, Migration)] {
    &[(LEGACY_SCHEMA_VERSION, migrate_v1_to_v2)]
}

/// v1 → v2: introduce per-stem output size anchors. The `anchors` field is
/// populated by serde's `#[serde(default)]`, so there is no data to reshape and
/// this migration is intentionally a no-op.
fn migrate_v1_to_v2(_state: &mut MergeState) {}

/// Advances `state` to `target_version` by applying every migration in `chain`
/// in order, stopping early if a step is missing.
///
/// The driver — not each migration — bumps `state.version`. Otherwise a migration
/// that wrote `CURRENT_SCHEMA_VERSION` directly would, on the next schema bump,
/// stamp an old file as current while the loop skipped every later migration,
/// leaving state that claims a version whose fields it does not have.
fn apply_migrations(state: &mut MergeState, chain: &[(u32, Migration)], target_version: u32) {
    for (from_version, migrate) in chain {
        if state.version == *from_version {
            migrate(state);
            state.version = from_version + 1;
        }
        if state.version >= target_version {
            break;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MergeState {
    #[serde(default = "default_schema_version")]
    pub version: u32,
    pub source_dir: String,
    pub output_dir: String,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub completed: HashSet<String>,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub failed: HashSet<String>,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub pending: HashSet<String>,
    /// stem → merged output byte size. Recorded on success; used on resume to
    /// detect outputs modified externally. Absent from v1 files.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub anchors: HashMap<String, u64>,
}

impl MergeState {
    pub fn new(source: &Path, output: &Path) -> Self {
        Self {
            version: CURRENT_SCHEMA_VERSION,
            source_dir: source.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            completed: HashSet::new(),
            failed: HashSet::new(),
            pending: HashSet::new(),
            anchors: HashMap::new(),
        }
    }

    pub fn state_file_path(source: &Path) -> PathBuf {
        source.join(".mixbilibili_state.json")
    }

    pub fn load(source: &Path) -> Result<Option<Self>> {
        let path = Self::state_file_path(source);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).context(t("failed_read_state").into_owned())?;

        // Try to parse with detailed error message
        let mut state: Self = match serde_json::from_str(&content) {
            Ok(state) => state,
            Err(e) => {
                anyhow::bail!("{}: {}. {}", t("failed_parse_state"), path.display(), e);
            }
        };

        // Reject files written by a newer mixbilibili (forward compatibility).
        if state.version > CURRENT_SCHEMA_VERSION {
            anyhow::bail!(
                "State file schema version {} is not supported (max: {}). Please upgrade mixbilibili.",
                state.version,
                CURRENT_SCHEMA_VERSION
            );
        }

        // Apply any pending migrations in order (old version → current).
        // A file already at the current version runs no migration.
        apply_migrations(&mut state, migrations(), CURRENT_SCHEMA_VERSION);

        let expected = source.to_string_lossy().to_string();
        if state.source_dir != expected {
            anyhow::bail!(
                "Resume state source_dir mismatch: state has '{}', loaded from '{}'",
                state.source_dir,
                expected
            );
        }
        Ok(Some(state))
    }

    pub fn save(&self, source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        let content =
            serde_json::to_string_pretty(self).context(t("failed_serialize_state").into_owned())?;

        // Write to temporary file first for atomic operation
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, &content).context(t("failed_write_state").into_owned())?;

        // Atomic rename (on POSIX systems, this is atomic)
        fs::rename(&tmp_path, &path)
            .context(t("failed_write_state").into_owned())
            .inspect_err(|_e| {
                let _ = fs::remove_file(&tmp_path);
            })?;

        Ok(())
    }

    pub fn mark_completed(&mut self, stem: &str) {
        self.pending.remove(stem);
        self.failed.remove(stem); // Clean failed set on successful completion
        self.completed.insert(stem.to_string());
    }

    pub fn mark_failed(&mut self, stem: &str) {
        self.pending.remove(stem);
        self.failed.insert(stem.to_string());
    }

    pub fn is_failed(&self, stem: &str) -> bool {
        self.failed.contains(stem)
    }

    /// Still to be merged — including pairs an interrupt left untouched.
    pub fn is_pending(&self, stem: &str) -> bool {
        self.pending.contains(stem)
    }

    pub fn is_completed(&self, stem: &str) -> bool {
        self.completed.contains(stem)
    }

    pub fn add_pending(&mut self, stem: &str) {
        // Guard pending.contains to avoid allocation when stem already exists
        if !self.pending.contains(stem)
            && !self.completed.contains(stem)
            && !self.failed.contains(stem)
        {
            self.pending.insert(stem.to_string());
        }
    }

    /// Record the merged output byte size for a stem (size anchor).
    pub fn record_anchor(&mut self, stem: &str, size: u64) {
        self.anchors.insert(stem.to_string(), size);
    }

    /// Return the recorded output size for a stem, if any.
    pub fn anchor_for(&self, stem: &str) -> Option<u64> {
        self.anchors.get(stem).copied()
    }

    pub fn clear(source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        if path.exists() {
            fs::remove_file(&path).context(t("failed_remove_state").into_owned())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_state_creation() {
        let state = MergeState::new(Path::new("/source"), Path::new("/output"));
        assert!(state.completed.is_empty());
        assert!(state.failed.is_empty());
        assert!(state.pending.is_empty());
    }

    #[test]
    fn test_state_save_load() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path());
        state.add_pending("test");
        state.mark_completed("test");

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();

        assert!(loaded.completed.contains("test"));
        assert!(loaded.pending.is_empty());
    }

    #[test]
    fn test_state_load_old_vec_format() {
        let dir = tempdir().unwrap();
        // Escape backslashes so Windows temp paths embed validly in the JSON string.
        let dir_str = dir.path().to_string_lossy().replace('\\', "\\\\");
        let json = format!(
            r#"{{
  "source_dir": "{dir_str}",
  "output_dir": "{dir_str}",
  "format": "mkv",
  "completed": ["video1", "video2"],
  "failed": [],
  "pending": ["video3"]
}}"#
        );
        let path = MergeState::state_file_path(dir.path());
        fs::write(&path, json).unwrap();

        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(loaded.completed.contains("video1"));
        assert!(loaded.completed.contains("video2"));
        assert!(loaded.pending.contains("video3"));
    }

    #[test]
    fn test_state_load_corrupted_json() {
        let dir = tempdir().unwrap();
        let path = MergeState::state_file_path(dir.path());

        // Write invalid JSON
        fs::write(&path, "{ invalid json }}}").unwrap();

        let result = MergeState::load(dir.path());
        assert!(result.is_err(), "Should fail on corrupted JSON");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("解析") || err_msg.contains("parse") || err_msg.contains("corrupted"),
            "Error should mention parsing issue: {}",
            err_msg
        );
    }

    #[test]
    fn test_state_load_missing_required_fields() {
        let dir = tempdir().unwrap();
        // Escape backslashes so Windows temp paths embed validly in the JSON string.
        let dir_str = dir.path().to_string_lossy().replace('\\', "\\\\");
        let path = MergeState::state_file_path(dir.path());

        // Write JSON missing required fields
        let json = format!(
            r#"{{
  "source_dir": "{dir_str}"
}}"#
        );
        fs::write(&path, json).unwrap();

        let result = MergeState::load(dir.path());
        assert!(result.is_err(), "Should fail on missing required fields");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("缺少")
                || err_msg.contains("missing field")
                || err_msg.contains("parse"),
            "Error should mention missing fields or parsing issue: {}",
            err_msg
        );
    }

    #[test]
    fn test_state_load_wrong_field_types() {
        let dir = tempdir().unwrap();
        // Escape backslashes so Windows temp paths embed validly in the JSON string.
        let dir_str = dir.path().to_string_lossy().replace('\\', "\\\\");
        let path = MergeState::state_file_path(dir.path());

        // Write JSON with wrong types (completed as string instead of array)
        let json = format!(
            r#"{{
  "source_dir": "{dir_str}",
  "output_dir": "{dir_str}",
  "completed": "not_an_array",
  "failed": [],
  "pending": []
}}"#
        );
        fs::write(&path, json).unwrap();

        let result = MergeState::load(dir.path());
        assert!(result.is_err(), "Should fail on wrong field types");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("类型") || err_msg.contains("type") || err_msg.contains("parse"),
            "Error should mention type mismatch or parsing issue: {}",
            err_msg
        );
    }

    #[test]
    fn test_is_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.completed.insert("video1".to_string());
        assert!(state.is_completed("video1"));
        assert!(!state.is_completed("video2"));
    }

    #[test]
    fn test_mark_failed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.add_pending("video1");
        state.mark_failed("video1");
        assert!(state.failed.contains("video1"));
        assert!(!state.pending.contains("video1"));
    }

    #[test]
    fn test_add_pending_no_duplicates() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.add_pending("video1");
        state.add_pending("video1");
        assert_eq!(state.pending.len(), 1);
    }

    #[test]
    fn test_add_pending_not_in_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.completed.insert("video1".to_string());
        state.add_pending("video1");
        assert!(!state.pending.contains("video1"));
    }

    #[test]
    fn test_clear_state() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        state.save(dir.path()).unwrap();

        assert!(MergeState::state_file_path(dir.path()).exists());

        MergeState::clear(dir.path()).unwrap();
        assert!(!MergeState::state_file_path(dir.path()).exists());
    }

    #[test]
    fn test_load_nonexistent() {
        let dir = tempdir().unwrap();
        let loaded = MergeState::load(dir.path()).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_merge_state_clear_removes_file() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        state.save(dir.path()).unwrap();
        let path = MergeState::state_file_path(dir.path());
        assert!(path.exists(), "state file should exist after save");
        MergeState::clear(dir.path()).unwrap();
        assert!(!path.exists(), "state file should be gone after clear");
    }

    #[test]
    fn test_merge_state_clear_no_error_when_no_file() {
        let dir = tempdir().unwrap();
        let path = MergeState::state_file_path(dir.path());
        assert!(!path.exists());
        let result = MergeState::clear(dir.path());
        assert!(result.is_ok(), "clear should not error when no file exists");
    }

    #[test]
    fn test_merge_state_is_completed_false_for_unknown() {
        let state = MergeState::new(Path::new("."), Path::new("."));
        assert!(!state.is_completed("nonexistent_video"));
        assert!(!state.is_completed(""));
        assert!(!state.is_completed("anything"));
    }

    #[test]
    fn test_merge_state_resume_validates_source_dir() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();

        // Create state with source_dir pointing to dir_a
        let state = MergeState::new(dir_a.path(), dir_a.path());
        state.save(dir_a.path()).unwrap();

        // Copy state file to dir_b so load() finds it there
        let state_content = fs::read_to_string(MergeState::state_file_path(dir_a.path())).unwrap();
        fs::write(MergeState::state_file_path(dir_b.path()), &state_content).unwrap();

        // Loading from dir_b should fail because source_dir in state points to dir_a
        let result = MergeState::load(dir_b.path());
        assert!(
            result.is_err(),
            "Loading state with mismatched source_dir should error"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("source") || err_msg.contains("dir") || err_msg.contains("mismatch"),
            "Error should mention source directory mismatch, got: {err_msg}"
        );
    }

    #[test]
    fn test_merge_state_mark_completed_moves_from_pending() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.add_pending("video1");
        state.add_pending("video2");
        assert!(state.pending.contains("video1"));

        state.mark_completed("video1");

        assert!(
            !state.pending.contains("video1"),
            "video1 should be removed from pending"
        );
        assert!(
            state.completed.contains("video1"),
            "video1 should be in completed"
        );
        assert!(
            state.pending.contains("video2"),
            "video2 should still be pending"
        );
    }

    #[test]
    fn test_merge_state_mark_failed_moves_from_pending() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.add_pending("video1");
        state.add_pending("video2");
        assert!(state.pending.contains("video1"));

        state.mark_failed("video1");

        assert!(
            !state.pending.contains("video1"),
            "video1 should be removed from pending"
        );
        assert!(
            state.failed.contains("video1"),
            "video1 should be in failed"
        );
        assert!(
            state.pending.contains("video2"),
            "video2 should still be pending"
        );
    }

    #[test]
    fn test_merge_state_add_pending_skips_already_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.completed.insert("video1".to_string());

        state.add_pending("video1");

        assert!(
            !state.pending.contains("video1"),
            "completed items should not be added to pending"
        );
    }

    #[test]
    fn test_merge_state_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let output_dir = dir.path().join("output");
        let mut state = MergeState::new(dir.path(), &output_dir);
        state.add_pending("video1");
        state.add_pending("video2");
        state.mark_completed("video1");
        state.mark_failed("video2");

        state.save(dir.path()).unwrap();

        let loaded = MergeState::load(dir.path())
            .unwrap()
            .expect("state should load");

        assert_eq!(loaded.source_dir, dir.path().to_string_lossy());
        assert_eq!(
            loaded.output_dir,
            dir.path().join("output").to_string_lossy()
        );
        assert!(loaded.completed.contains("video1"));
        assert!(loaded.failed.contains("video2"));
        assert!(loaded.pending.is_empty());
    }

    #[test]
    fn test_save_creates_temp_file_first() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        let state_path = MergeState::state_file_path(dir.path());

        // Save should succeed
        state.save(dir.path()).unwrap();

        // Verify final file exists
        assert!(state_path.exists());

        // Verify no .tmp file remains after successful save
        let tmp_path = state_path.with_extension("json.tmp");
        assert!(
            !tmp_path.exists(),
            "Temp file should be renamed, not left behind"
        );
    }

    #[test]
    fn test_save_uses_atomic_write_pattern() {
        // This test verifies the implementation uses temp file + rename
        let dir = tempdir().unwrap();
        let state_path = MergeState::state_file_path(dir.path());
        let tmp_path = state_path.with_extension("json.tmp");

        // Create initial state
        let mut state1 = MergeState::new(dir.path(), dir.path());
        state1.add_pending("video1");
        state1.save(dir.path()).unwrap();

        // Verify initial state is saved correctly
        let loaded1 = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(loaded1.pending.contains("video1"));

        // Now save again with different data
        let mut state2 = MergeState::new(dir.path(), dir.path());
        state2.add_pending("video2");
        state2.save(dir.path()).unwrap();

        // Verify new state is saved correctly
        let loaded2 = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(loaded2.pending.contains("video2"));
        assert!(!loaded2.pending.contains("video1"));

        // Verify no temp file remains
        assert!(!tmp_path.exists(), "Temp file should not remain after save");
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
}

/// Additional tests for state transitions, serialization order, and edge cases.
#[cfg(test)]
mod state_logic_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_add_pending_skips_already_failed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.failed.insert("bad_video".to_string());

        state.add_pending("bad_video");

        assert!(
            !state.pending.contains("bad_video"),
            "Failed items should not be re-added to pending"
        );
    }

    #[test]
    fn test_mark_completed_idempotent() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.mark_completed("vid");
        state.mark_completed("vid"); // second call should not panic
        assert!(state.completed.contains("vid"));
        assert_eq!(state.completed.len(), 1);
    }

    #[test]
    fn test_mark_failed_idempotent() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.mark_failed("vid");
        state.mark_failed("vid");
        assert!(state.failed.contains("vid"));
        assert_eq!(state.failed.len(), 1);
    }

    #[test]
    fn test_serialized_sets_are_sorted() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path());
        // Insert in non-sorted order
        state.completed.insert("zebra".to_string());
        state.completed.insert("alpha".to_string());
        state.completed.insert("middle".to_string());

        state.save(dir.path()).unwrap();

        let content = fs::read_to_string(MergeState::state_file_path(dir.path())).unwrap();
        // In JSON, the array should be sorted
        let alpha_pos = content.find("alpha").unwrap();
        let middle_pos = content.find("middle").unwrap();
        let zebra_pos = content.find("zebra").unwrap();
        assert!(
            alpha_pos < middle_pos && middle_pos < zebra_pos,
            "Serialized completed set should be sorted alphabetically"
        );
    }

    #[test]
    fn test_state_file_path_format() {
        let path = MergeState::state_file_path(Path::new("/some/dir"));
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            ".mixbilibili_state.json"
        );
        assert_eq!(path.parent().unwrap(), Path::new("/some/dir"));
    }

    #[test]
    fn test_state_new_stores_paths_correctly() {
        let state = MergeState::new(Path::new("/src"), Path::new("/out"));
        assert_eq!(state.source_dir, "/src");
        assert_eq!(state.output_dir, "/out");
    }

    #[test]
    fn test_mark_completed_also_removes_from_failed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.pending.insert("a".to_string());
        state.failed.insert("a".to_string()); // edge case: in both sets

        state.mark_completed("a");

        assert!(!state.pending.contains("a"));
        assert!(state.completed.contains("a"));
        // mark_completed removes from both pending and failed
        assert!(!state.failed.contains("a"));
    }

    #[test]
    fn test_large_state_roundtrip() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path());

        // Add many items
        for i in 0..100 {
            state.add_pending(&format!("video_{:04}", i));
        }
        for i in 0..50 {
            state.mark_completed(&format!("video_{:04}", i));
        }
        for i in 50..75 {
            state.mark_failed(&format!("video_{:04}", i));
        }

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();

        assert_eq!(loaded.completed.len(), 50);
        assert_eq!(loaded.failed.len(), 25);
        assert_eq!(loaded.pending.len(), 25);
    }

    #[test]
    fn test_state_default_has_empty_fields() {
        let state = MergeState::default();
        assert!(state.source_dir.is_empty());
        assert!(state.output_dir.is_empty());
        assert!(state.completed.is_empty());
        assert!(state.failed.is_empty());
        assert!(state.pending.is_empty());
    }

    #[test]
    fn test_load_old_format_gets_default_version() {
        let dir = tempdir().unwrap();
        // Escape backslashes so Windows temp paths embed validly in the JSON string.
        let dir_str = dir.path().to_string_lossy().replace('\\', "\\\\");
        // Old format JSON without "version" field
        let json = format!(
            r#"{{
  "source_dir": "{dir_str}",
  "output_dir": "{dir_str}",
  "completed": [],
  "failed": [],
  "pending": []
}}"#
        );
        let path = MergeState::state_file_path(dir.path());
        fs::write(&path, json).unwrap();

        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        // Old format defaults to v1, then migrates forward to the current version.
        assert_eq!(
            loaded.version, CURRENT_SCHEMA_VERSION,
            "old format should migrate to the current version"
        );
        assert!(
            loaded.anchors.is_empty(),
            "migrated anchors should be empty"
        );
    }

    #[test]
    fn test_load_v1_migrates_to_v2() {
        let dir = tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();
        // Explicit version 1 state file
        let json = format!(
            r#"{{
  "version": 1,
  "source_dir": "{dir_str}",
  "output_dir": "{dir_str}",
  "completed": ["video1"],
  "failed": [],
  "pending": []
}}"#
        );
        let path = MergeState::state_file_path(dir.path());
        fs::write(&path, json).unwrap();

        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.version, CURRENT_SCHEMA_VERSION);
        assert!(loaded.completed.contains("video1"));
        assert!(loaded.anchors.is_empty());
    }

    #[test]
    fn test_migrate_v1_to_v2_leaves_data_intact() {
        // The v1 → v2 migration is a pure version bump; serde fills `anchors`.
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = LEGACY_SCHEMA_VERSION;
        state.completed.insert("video1".to_string());

        migrate_v1_to_v2(&mut state);
        apply_migrations(&mut state, migrations(), CURRENT_SCHEMA_VERSION);

        assert_eq!(state.version, CURRENT_SCHEMA_VERSION);
        assert!(state.completed.contains("video1"));
    }

    /// Synthetic chain used to prove the driver walks a *future*, longer chain.
    /// Each step appends a marker so we can assert every step ran, in order.
    fn step_to_v2(state: &mut MergeState) {
        state.pending.insert("ran-v1-v2".to_string());
    }

    fn step_to_v3(state: &mut MergeState) {
        state.pending.insert("ran-v2-v3".to_string());
    }

    #[test]
    fn test_apply_migrations_runs_every_step_in_order() {
        let chain: &[(u32, Migration)] = &[(1, step_to_v2), (2, step_to_v3)];
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = 1;

        apply_migrations(&mut state, chain, 3);

        assert_eq!(state.version, 3, "should reach the target version");
        assert!(
            state.pending.contains("ran-v1-v2"),
            "v1 → v2 migration was skipped"
        );
        assert!(
            state.pending.contains("ran-v2-v3"),
            "v2 → v3 migration was skipped"
        );
    }

    #[test]
    fn test_apply_migrations_stops_at_current_version() {
        // A v2 file on a v3 chain must only run the v2 → v3 step.
        let chain: &[(u32, Migration)] = &[(1, step_to_v2), (2, step_to_v3)];
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = 2;

        apply_migrations(&mut state, chain, 3);

        assert_eq!(state.version, 3);
        assert!(
            !state.pending.contains("ran-v1-v2"),
            "must not re-run an already-applied migration"
        );
        assert!(state.pending.contains("ran-v2-v3"));
    }

    #[test]
    fn test_apply_migrations_is_noop_when_already_current() {
        let chain: &[(u32, Migration)] = &[(1, step_to_v2), (2, step_to_v3)];
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = 3;

        apply_migrations(&mut state, chain, 3);

        assert_eq!(state.version, 3);
        assert!(state.pending.is_empty(), "no migration should have run");
    }

    #[test]
    fn test_apply_migrations_tolerates_missing_step() {
        // If a migration is missing, the state keeps whatever version it reached
        // rather than being stamped as current with un-migrated fields.
        let chain: &[(u32, Migration)] = &[(1, step_to_v2)];
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = 1;

        apply_migrations(&mut state, chain, 3);

        assert_eq!(
            state.version, 2,
            "should not claim v3 without running v2→v3"
        );
    }

    #[test]
    fn test_migration_chain_is_contiguous_and_complete() {
        // Bumping CURRENT_SCHEMA_VERSION without adding the matching migration
        // would leave old files unable to reach the current version, so fail
        // loudly here instead of silently loading un-migrated state.
        let chain = migrations();
        assert_eq!(
            chain.len() as u32,
            CURRENT_SCHEMA_VERSION - LEGACY_SCHEMA_VERSION,
            "each version bump from LEGACY to CURRENT needs exactly one migration entry"
        );
        for (offset, (from_version, _)) in chain.iter().enumerate() {
            assert_eq!(
                *from_version,
                LEGACY_SCHEMA_VERSION + offset as u32,
                "migration chain must be ordered and contiguous"
            );
        }
    }

    #[test]
    fn test_load_v1_reaches_current_through_real_chain() {
        // End-to-end proof that a legacy file lands on the current version and
        // that every step in the real chain actually ran.
        let mut state = MergeState::new(Path::new("/s"), Path::new("/o"));
        state.version = LEGACY_SCHEMA_VERSION;

        apply_migrations(&mut state, migrations(), CURRENT_SCHEMA_VERSION);

        assert_eq!(state.version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_record_anchor_and_query() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        assert_eq!(state.anchor_for("video1"), None);
        state.record_anchor("video1", 12345);
        assert_eq!(state.anchor_for("video1"), Some(12345));
        assert_eq!(state.anchor_for("video2"), None);
    }

    #[test]
    fn test_anchors_roundtrip() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path());
        state.add_pending("video1");
        state.mark_completed("video1");
        state.record_anchor("video1", 999);

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.anchor_for("video1"), Some(999));
    }

    #[test]
    fn test_anchors_empty_not_serialized() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        state.save(dir.path()).unwrap();

        let content = fs::read_to_string(MergeState::state_file_path(dir.path())).unwrap();
        assert!(
            !content.contains("anchors"),
            "empty anchors map should be skipped: {content}"
        );
    }

    #[test]
    fn test_save_includes_version() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        state.save(dir.path()).unwrap();

        let content = fs::read_to_string(MergeState::state_file_path(dir.path())).unwrap();
        assert!(
            content.contains("\"version\""),
            "serialized state should include version field"
        );
    }

    #[test]
    fn test_reject_future_version() {
        let dir = tempdir().unwrap();
        // Escape backslashes so Windows temp paths embed validly in the JSON string.
        let dir_str = dir.path().to_string_lossy().replace('\\', "\\\\");
        // Future format with version > 1
        let json = format!(
            r#"{{
  "version": 99,
  "source_dir": "{dir_str}",
  "output_dir": "{dir_str}",
  "completed": [],
  "failed": [],
  "pending": []
}}"#
        );
        let path = MergeState::state_file_path(dir.path());
        fs::write(&path, json).unwrap();

        let result = MergeState::load(dir.path());
        assert!(result.is_err(), "should reject future schema version");
        assert!(result.unwrap_err().to_string().contains("not supported"));
    }
}
