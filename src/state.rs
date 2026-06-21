use crate::i18n::t;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MergeState {
    pub source_dir: String,
    pub output_dir: String,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub completed: HashSet<String>,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub failed: HashSet<String>,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub pending: HashSet<String>,
}

impl MergeState {
    pub fn new(source: &Path, output: &Path) -> Self {
        Self {
            source_dir: source.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            completed: HashSet::new(),
            failed: HashSet::new(),
            pending: HashSet::new(),
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
        let state: Self =
            serde_json::from_str(&content).context(t("failed_parse_state").into_owned())?;
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
        fs::write(&path, content).context(t("failed_write_state").into_owned())?;
        Ok(())
    }

    pub fn mark_completed(&mut self, stem: &str) {
        self.pending.remove(stem);
        self.completed.insert(stem.to_string());
    }

    pub fn mark_failed(&mut self, stem: &str) {
        self.pending.remove(stem);
        self.failed.insert(stem.to_string());
    }

    pub fn is_completed(&self, stem: &str) -> bool {
        self.completed.contains(stem)
    }

    pub fn add_pending(&mut self, stem: &str) {
        if !self.pending.contains(stem)
            && !self.completed.contains(stem)
            && !self.failed.contains(stem)
        {
            self.pending.insert(stem.to_string());
        }
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
        let dir_str = dir.path().to_string_lossy().to_string();
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
}
