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

/// Result of an installation attempt
#[derive(Debug)]
pub struct InstallResult {
    pub success: bool,
    pub output: String,
}

/// Get install command for the current OS
pub fn get_install_command(os: Os) -> Option<(String, String)> {
    match os {
        Os::Windows => {
            // Try winget first
            if which::which("winget").is_ok() {
                Some(("winget".to_string(), "winget install ffmpeg".to_string()))
            } else if which::which("choco").is_ok() {
                Some(("choco".to_string(), "choco install ffmpeg -y".to_string()))
            } else {
                None
            }
        }
        Os::MacOS => {
            if which::which("brew").is_ok() {
                Some(("brew".to_string(), "brew install ffmpeg".to_string()))
            } else {
                None
            }
        }
        Os::Linux => {
            if which::which("apt").is_ok() {
                Some(("apt".to_string(), "sudo apt update && sudo apt install -y ffmpeg".to_string()))
            } else {
                None
            }
        }
        Os::Unknown => None,
    }
}

/// Get manual install instructions for the current OS
pub fn get_manual_instructions(os: Os) -> &'static str {
    match os {
        Os::Windows => {
            "To install ffmpeg manually:\n\
             1. Using winget: winget install ffmpeg\n\
             2. Using Chocolatey: choco install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html\n\
                Download the Windows build, extract, and add to PATH."
        }
        Os::MacOS => {
            "To install ffmpeg manually:\n\
             1. Using Homebrew: brew install ffmpeg\n\
             2. Using MacPorts: sudo port install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html"
        }
        Os::Linux => {
            "To install ffmpeg manually:\n\
             1. Using apt: sudo apt update && sudo apt install ffmpeg\n\
             2. Using snap: sudo snap install ffmpeg\n\
             3. Manual build: https://trac.ffmpeg.org/wiki/CompilationGuide"
        }
        Os::Unknown => {
            "Please install ffmpeg from https://ffmpeg.org/download.html"
        }
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

#[cfg(test)]
mod pm_tests {
    use super::*;

    #[test]
    fn test_get_manual_instructions_windows() {
        let instructions = get_manual_instructions(Os::Windows);
        assert!(instructions.contains("winget"));
        assert!(instructions.contains("choco"));
    }

    #[test]
    fn test_get_manual_instructions_macos() {
        let instructions = get_manual_instructions(Os::MacOS);
        assert!(instructions.contains("brew"));
        assert!(instructions.contains("MacPorts"));
    }

    #[test]
    fn test_get_manual_instructions_linux() {
        let instructions = get_manual_instructions(Os::Linux);
        assert!(instructions.contains("apt"));
        assert!(instructions.contains("snap"));
    }

    #[test]
    fn test_get_manual_instructions_unknown() {
        let instructions = get_manual_instructions(Os::Unknown);
        assert!(instructions.contains("ffmpeg.org"));
    }

    #[test]
    fn test_get_install_command_unknown_returns_none() {
        let result = get_install_command(Os::Unknown);
        assert!(result.is_none());
    }
}