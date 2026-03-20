// src/ffmpeg.rs

/// Check if ffmpeg is available in PATH
pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Get ffmpeg path if available
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    which::which("ffmpeg").ok()
}

/// Supported operating systems
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOS,
    Linux,
    Unknown,
}

/// Detect current operating system
pub fn detect_os() -> Os {
    #[cfg(target_os = "windows")]
    {
        Os::Windows
    }
    #[cfg(target_os = "macos")]
    {
        Os::MacOS
    }
    #[cfg(target_os = "linux")]
    {
        Os::Linux
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Os::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ffmpeg_available_does_not_panic() {
        // This test depends on system state
        // It should not panic either way
        let _ = is_ffmpeg_available();
    }

    #[test]
    fn test_ffmpeg_path_returns_some_if_available() {
        // If ffmpeg is installed, should return Some
        if is_ffmpeg_available() {
            assert!(ffmpeg_path().is_some());
        }
    }
}

#[cfg(test)]
mod os_tests {
    use super::*;

    #[test]
    fn test_detect_os_returns_valid_value() {
        let os = detect_os();
        assert!(matches!(os, Os::Windows | Os::MacOS | Os::Linux | Os::Unknown));
    }

    #[test]
    fn test_os_debug_format() {
        let os = detect_os();
        let debug_str = format!("{:?}", os);
        assert!(!debug_str.is_empty());
    }
}