use crate::i18n::{t, tf};
use thiserror::Error;

pub mod exit_codes {
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}

/// Application-level errors with typed variants for exit code mapping.
///
/// Using thiserror eliminates the fragile string-based error classification
/// that was previously done via error_category().
#[derive(Debug, Error)]
pub enum AppError {
    #[error("ffmpeg not found")]
    FfmpegNotFound,

    #[error("{count} merge(s) failed")]
    MergeFailed { count: usize },

    #[error("source directory is not readable: {path}")]
    UnreadableSource { path: String },
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::FfmpegNotFound => exit_codes::FFMPEG_NOT_FOUND,
            AppError::MergeFailed { .. } => exit_codes::MERGE_FAILED,
            AppError::UnreadableSource { .. } => exit_codes::GENERAL_ERROR,
        }
    }
}

/// Extract the exit code from an anyhow error.
/// Tries downcasting to AppError first, then returns GENERAL_ERROR.
pub fn get_exit_code(e: &anyhow::Error) -> i32 {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        return app_err.exit_code();
    }

    exit_codes::GENERAL_ERROR
}

/// Translate AppError messages for user-facing output.
/// Returns the original error string for non-AppError types.
pub fn translate_error(e: &anyhow::Error) -> String {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        match app_err {
            AppError::FfmpegNotFound => t("ffmpeg_not_found").to_string(),
            AppError::MergeFailed { count } => tf("merge_failed", &[&count.to_string()]),
            AppError::UnreadableSource { path } => tf("unreadable_source", &[path]),
        }
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes_constants() {
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::FFMPEG_NOT_FOUND, 2);
        assert_eq!(exit_codes::MERGE_FAILED, 3);
    }

    #[test]
    fn test_app_error_display() {
        assert_eq!(AppError::FfmpegNotFound.to_string(), "ffmpeg not found");
        assert_eq!(
            AppError::MergeFailed { count: 3 }.to_string(),
            "3 merge(s) failed"
        );
        assert_eq!(
            AppError::UnreadableSource {
                path: "/tmp/test".to_string()
            }
            .to_string(),
            "source directory is not readable: /tmp/test"
        );
    }

    #[test]
    fn test_app_error_exit_codes() {
        assert_eq!(
            AppError::FfmpegNotFound.exit_code(),
            exit_codes::FFMPEG_NOT_FOUND
        );
        assert_eq!(
            AppError::MergeFailed { count: 1 }.exit_code(),
            exit_codes::MERGE_FAILED
        );
        assert_eq!(
            AppError::UnreadableSource {
                path: "test".to_string()
            }
            .exit_code(),
            exit_codes::GENERAL_ERROR
        );
    }

    #[test]
    fn test_app_error_into_anyhow() {
        let app_err = AppError::FfmpegNotFound;
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(anyhow_err.to_string(), "ffmpeg not found");
    }

    #[test]
    fn test_get_exit_code_typed_app_error() {
        let app_err = AppError::FfmpegNotFound;
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(get_exit_code(&anyhow_err), exit_codes::FFMPEG_NOT_FOUND);
    }

    #[test]
    fn test_get_exit_code_merge_failed() {
        let app_err = AppError::MergeFailed { count: 3 };
        let anyhow_err: anyhow::Error = app_err.into();
        assert_eq!(get_exit_code(&anyhow_err), exit_codes::MERGE_FAILED);
    }

    #[test]
    fn test_get_exit_code_non_app_error() {
        let err = anyhow::anyhow!("Something went wrong");
        assert_eq!(get_exit_code(&err), exit_codes::GENERAL_ERROR);
    }
}
