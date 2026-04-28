use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// State for tracking merge progress to enable resume capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MergeState {
    /// Source directory path
    pub source_dir: String,
    /// Output directory path
    pub output_dir: String,
    /// Output format
    pub format: String,
    /// Completed file stems
    pub completed: Vec<String>,
    /// Failed file stems
    pub failed: Vec<String>,
    /// Pending file stems
    pub pending: Vec<String>,
}

impl MergeState {
    /// Create a new merge state
    pub fn new(source: &Path, output: &Path, format: &str) -> Self {
        Self {
            source_dir: source.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            format: format.to_string(),
            completed: Vec::new(),
            failed: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Get the state file path for a given source directory
    pub fn state_file_path(source: &Path) -> PathBuf {
        source.join(".mixbilibili_state.json")
    }

    /// Load state from a source directory
    pub fn load(source: &Path) -> Result<Option<Self>> {
        let path = Self::state_file_path(source);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).context("Failed to read state file")?;
        let state: Self = serde_json::from_str(&content).context("Failed to parse state file")?;
        Ok(Some(state))
    }

    /// Save state to a source directory
    pub fn save(&self, source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        let content = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
        fs::write(&path, content).context("Failed to write state file")?;
        Ok(())
    }

    /// Mark a file stem as completed
    pub fn mark_completed(&mut self, stem: &str) {
        self.pending.retain(|s| s != stem);
        if !self.completed.iter().any(|s| s == stem) {
            self.completed.push(stem.to_string());
        }
    }

    /// Mark a file stem as failed
    pub fn mark_failed(&mut self, stem: &str) {
        self.pending.retain(|s| s != stem);
        if !self.failed.iter().any(|s| s == stem) {
            self.failed.push(stem.to_string());
        }
    }

    /// Check if a file stem is already completed
    pub fn is_completed(&self, stem: &str) -> bool {
        self.completed.iter().any(|s| s == stem)
    }

    /// Add a pending file stem
    pub fn add_pending(&mut self, stem: &str) {
        if !self.pending.iter().any(|s| s == stem)
            && !self.completed.iter().any(|s| s == stem)
            && !self.failed.iter().any(|s| s == stem)
        {
            self.pending.push(stem.to_string());
        }
    }

    /// Clear the state file
    pub fn clear(source: &Path) -> Result<()> {
        let path = Self::state_file_path(source);
        if path.exists() {
            fs::remove_file(&path).context("Failed to remove state file")?;
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
        let state = MergeState::new(Path::new("/source"), Path::new("/output"), "mkv");
        assert_eq!(state.format, "mkv");
        assert!(state.completed.is_empty());
        assert!(state.failed.is_empty());
        assert!(state.pending.is_empty());
    }

    #[test]
    fn test_state_save_load() {
        let dir = tempdir().unwrap();
        let mut state = MergeState::new(dir.path(), dir.path(), "mkv");
        state.add_pending("test");
        state.mark_completed("test");

        state.save(dir.path()).unwrap();
        let loaded = MergeState::load(dir.path()).unwrap().unwrap();

        assert_eq!(loaded.completed, vec!["test"]);
        assert!(loaded.pending.is_empty());
    }

    #[test]
    fn test_is_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."), "mkv");
        state.completed.push("video1".to_string());
        assert!(state.is_completed("video1"));
        assert!(!state.is_completed("video2"));
    }

    #[test]
    fn test_mark_failed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."), "mkv");
        state.add_pending("video1");
        state.mark_failed("video1");
        assert!(state.failed.contains(&"video1".to_string()));
        assert!(!state.pending.contains(&"video1".to_string()));
    }

    #[test]
    fn test_add_pending_no_duplicates() {
        let mut state = MergeState::new(Path::new("."), Path::new("."), "mkv");
        state.add_pending("video1");
        state.add_pending("video1");
        assert_eq!(state.pending.len(), 1);
    }

    #[test]
    fn test_add_pending_not_in_completed() {
        let mut state = MergeState::new(Path::new("."), Path::new("."), "mkv");
        state.completed.push("video1".to_string());
        state.add_pending("video1");
        assert!(!state.pending.contains(&"video1".to_string()));
    }

    #[test]
    fn test_clear_state() {
        let dir = tempdir().unwrap();
        let state = MergeState::new(dir.path(), dir.path(), "mkv");
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
}
