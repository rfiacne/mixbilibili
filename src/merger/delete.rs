use crate::i18n::tf;
use crate::scanner::FilePair;
use anyhow::Result;

pub fn delete_source_files(pair: &FilePair) -> Result<()> {
    // Delete audio first (less valuable file)
    // Attempt deletion directly and interpret errors to avoid TOCTOU race
    match std::fs::remove_file(&pair.audio) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "source_missing",
                    &["audio", &pair.audio.display().to_string()]
                )
            );
        }
        Err(e) => {
            // Audio deletion failed, preserve video
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_audio",
                    &[
                        &pair.audio.display().to_string(),
                        &e.to_string(),
                        &pair.video.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    // Then delete video
    match std::fs::remove_file(&pair.video) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Audio already deleted, video missing
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video_audio_gone",
                    &[
                        &pair.video.display().to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Err(e) => {
            // Video deletion failed, but audio already deleted
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video",
                    &[
                        &pair.video.display().to_string(),
                        &e.to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_delete_source_files_both_succeed() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("test_v.mp4");
        let audio_path = dir.path().join("test_a.m4a");
        std::fs::write(&video_path, b"fake video").unwrap();
        std::fs::write(&audio_path, b"fake audio").unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "test".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_video_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("missing.mp4");
        let audio_path = dir.path().join("exists.m4a");
        std::fs::write(&audio_path, b"fake audio").unwrap();

        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "missing".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("failed_delete_video") || err.contains("video") || err.contains("视频"),
            "Error should mention video: {}",
            err
        );
    }

    #[test]
    fn test_delete_source_files_success() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_both_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("video") || err.contains("audio"),
            "Error should mention missing file: {}",
            err
        );
    }

    #[test]
    fn test_deletion_failures_tracking() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        File::create(&video_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Deletion should fail when audio missing");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("audio"),
            "Error should mention audio: {}",
            err_msg
        );

        assert!(
            video_path.exists(),
            "Video should be preserved when audio is missing (atomic deletion)"
        );
    }

    #[test]
    fn test_delete_preserves_video_on_audio_failure() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("nonexistent.m4a");

        File::create(&video_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Should fail when audio missing");

        assert!(
            video_path.exists(),
            "Video should be preserved when audio deletion fails"
        );
    }

    #[test]
    fn test_delete_checks_existence_first() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("audio.m4a");

        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_audio_missing_preserves_video() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("keep_me.mp4");
        let audio = dir.path().join("nonexistent.m4a");
        std::fs::write(&video, b"video data").unwrap();

        let pair = FilePair {
            video: video.clone(),
            audio,
            stem: "keep_me".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        assert!(video.exists());
    }
}
