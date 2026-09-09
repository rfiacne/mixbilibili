use crate::i18n::t;
use crate::i18n::tf;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn set_as_vec<S: Serializer>(set: &HashSet<String>, s: S) -> Result<S::Ok, S::Error> {
    let mut vec: Vec<&String> = set.iter().collect();
    vec.sort();
    vec.serialize(s)
}

fn vec_as_set<'de, D: Deserializer<'de>>(d: D) -> Result<HashSet<String>, D::Error> {
    let vec: Vec<String> = Vec::deserialize(d)?;
    Ok(vec.into_iter().collect())
}

/// Resume bookkeeping: which stems finished and which failed.
///
/// The `pending` set that older files carry is deliberately gone: its content
/// is exactly `scan result − completed`, so a resume run recomputes it by
/// rescanning. `#[serde(default)]` keeps pre-existing files loadable (old
/// files carry `pending`, which serde ignores; rolled-back binaries meet the
/// fields they know).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MergeState {
    pub source_dir: String,
    pub output_dir: String,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub completed: HashSet<String>,
    #[serde(serialize_with = "set_as_vec", deserialize_with = "vec_as_set")]
    pub failed: HashSet<String>,
}

/// Thread-safe resume-state tracker shared by the merge workers.
///
/// Owns the state and the save policy: workers call `record` after each
/// merge; the tracker flushes every `save_interval` outcomes and on `flush`.
/// Saves go through a snapshot-then-write protocol: the state lock is held
/// only for the clone, and a sequence guard keeps an older snapshot from
/// overwriting a newer one when two workers race on the file.
pub struct StateTracker {
    state: Mutex<MergeState>,
    source: PathBuf,
    save_interval: usize,
    outcomes: AtomicUsize,
    seq: AtomicUsize,
    /// Serializes snapshot writes; holds the last written sequence number.
    save_lock: Mutex<usize>,
}

impl StateTracker {
    pub fn new(state: MergeState, source: &Path, save_interval: usize) -> Self {
        Self {
            state: Mutex::new(state),
            source: source.to_path_buf(),
            save_interval: save_interval.max(1),
            outcomes: AtomicUsize::new(0),
            seq: AtomicUsize::new(0),
            save_lock: Mutex::new(0),
        }
    }

    /// Record one merge outcome; interrupted outcomes leave the state
    /// untouched so the stem stays unrecorded and a resume re-picks it.
    /// Returns an error when a cadence save failed — the batch should report
    /// it and keep merging.
    pub fn record(&self, stem: &str, success: bool, was_interrupted: bool) -> Result<()> {
        let snapshot = {
            let mut state = self.state.lock().unwrap();
            if !was_interrupted {
                if success {
                    state.mark_completed(stem);
                } else {
                    state.mark_failed(stem);
                }
            }
            let count = self.outcomes.fetch_add(1, Ordering::Relaxed) + 1;
            if count.is_multiple_of(self.save_interval) {
                Some((state.clone(), self.seq.fetch_add(1, Ordering::Relaxed) + 1))
            } else {
                None
            }
        }; // state lock dropped before file I/O so workers are never blocked
        match snapshot {
            Some((s, seq)) => self.save_snapshot(&s, seq),
            None => Ok(()),
        }
    }

    /// Flush the final state when the batch ends.
    pub fn flush(&self) -> Result<()> {
        let state = self.state.lock().unwrap();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.save_snapshot(&state, seq)
    }

    fn save_snapshot(&self, state: &MergeState, seq: usize) -> Result<()> {
        // Serialize writers and drop snapshots a newer one already superseded.
        let mut last = self.save_lock.lock().unwrap();
        if seq <= *last {
            return Ok(());
        }
        state.save(&self.source)?;
        *last = seq;
        Ok(())
    }
}

impl MergeState {
    pub fn new(source: &Path, output: &Path) -> Self {
        Self {
            source_dir: source.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            completed: HashSet::new(),
            failed: HashSet::new(),
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
                "{}",
                tf("state_source_mismatch", &[&state.source_dir, &expected])
            );
        }
        Ok(Some(state))
    }

    /// Atomic save: write a sibling temp file, then rename over the target.
    /// Readers never observe a torn file, and interrupted writes leave the
    /// previous state intact.
    pub fn save(&self, source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        let tmp = path.with_extension("json.tmp");
        let content =
            serde_json::to_string_pretty(self).context(t("failed_serialize_state").into_owned())?;
        fs::write(&tmp, content).context(t("failed_write_state").into_owned())?;
        fs::rename(&tmp, &path).context(t("failed_write_state").into_owned())?;
        Ok(())
    }

    pub fn mark_completed(&mut self, stem: &str) {
        self.failed.remove(stem);
        self.completed.insert(stem.to_string());
    }

    pub fn mark_failed(&mut self, stem: &str) {
        self.completed.remove(stem);
        self.failed.insert(stem.to_string());
    }

    pub fn is_completed(&self, stem: &str) -> bool {
        self.completed.contains(stem)
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
    }

    #[test]
    fn test_state_save_load() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path());
        state.mark_completed("test");

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();

        assert!(loaded.completed.contains("test"));
    }

    #[test]
    fn test_state_load_old_vec_format() {
        // Pre-refactor files carry "format" and "pending" keys: serde must
        // ignore the unknown ones and still load the sets.
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
    }

    #[test]
    fn test_save_is_atomic_no_temp_leftover() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path());
        state.save(dir.path()).unwrap();
        // The tmp sibling must be renamed away, not left behind.
        let tmp = MergeState::state_file_path(dir.path()).with_extension("json.tmp");
        assert!(!tmp.exists(), "temp file should be renamed over the target");
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
        state.mark_failed("video1");
        assert!(state.failed.contains("video1"));
        assert!(!state.completed.contains("video1"));
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
        // Locale-independent: the message quotes both directories in any language.
        assert!(
            err_msg.contains(&state.source_dir),
            "Error should quote the recorded source dir, got: {err_msg}"
        );
    }

    #[test]
    fn test_mark_completed_clears_prior_failure() {
        // A retried merge that now succeeds must not stay listed as failed.
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.mark_failed("video1");
        state.mark_completed("video1");
        assert!(state.completed.contains("video1"));
        assert!(!state.failed.contains("video1"));
    }

    #[test]
    fn test_mark_failed_clears_prior_completion() {
        let mut state = MergeState::new(Path::new("."), Path::new("."));
        state.mark_completed("video1");
        state.mark_failed("video1");
        assert!(state.failed.contains("video1"));
        assert!(!state.completed.contains("video1"));
    }

    #[test]
    fn test_merge_state_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let output_dir = dir.path().join("output");
        let mut state = MergeState::new(dir.path(), &output_dir);
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
    }
}

#[cfg(test)]
mod tracker_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn tracker_records_at_interval_and_flushes() {
        let dir = tempdir().unwrap();
        let tracker = StateTracker::new(MergeState::new(dir.path(), dir.path()), dir.path(), 2);

        // First outcome: below interval, nothing on disk yet.
        tracker.record("a", true, false).unwrap();
        assert!(MergeState::load(dir.path()).unwrap().is_none());

        // Second outcome hits the cadence and flushes both.
        tracker.record("b", false, false).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(loaded.is_completed("a"));
        assert!(loaded.failed.contains("b"));

        // Interrupted outcomes leave no trace; flush persists what exists.
        tracker.record("c", false, true).unwrap();
        tracker.flush().unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        assert!(!loaded.is_completed("c"));
        assert!(!loaded.failed.contains("c"));
    }

    #[test]
    fn tracker_is_shared_across_threads() {
        let dir = tempdir().unwrap();
        let tracker = std::sync::Arc::new(StateTracker::new(
            MergeState::new(dir.path(), dir.path()),
            dir.path(),
            1,
        ));

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let tracker = tracker.clone();
                std::thread::spawn(move || {
                    tracker.record(&format!("v{i}"), true, false).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        tracker.flush().unwrap();

        let loaded = MergeState::load(dir.path()).unwrap().unwrap();
        for i in 0..8 {
            assert!(loaded.is_completed(&format!("v{i}")));
        }
    }
}
