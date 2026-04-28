use crate::cli::OutputFormat;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;

pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

#[cfg(test)]
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    which::which("ffmpeg").ok()
}

// Os variants are matched in get_install_command/get_manual_instructions
// but only one variant is constructed at compile time via detect_os()
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Os {
    Windows,
    MacOS,
    Linux,
    Unknown,
}

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

/// Package manager candidates per OS: (name, install_command, availability_check).
fn package_managers(os: Os) -> Vec<(&'static str, &'static str)> {
    match os {
        Os::Windows => {
            let mut pms = Vec::new();
            if which::which("winget").is_ok() {
                pms.push(("winget", "winget install ffmpeg"));
            }
            if which::which("choco").is_ok() {
                pms.push(("choco", "choco install ffmpeg -y"));
            }
            pms
        }
        Os::MacOS => {
            if which::which("brew").is_ok() {
                vec![("brew", "brew install ffmpeg")]
            } else {
                vec![]
            }
        }
        Os::Linux => {
            if which::which("apt").is_ok() {
                vec![("apt", "sudo apt update && sudo apt install -y ffmpeg")]
            } else {
                vec![]
            }
        }
        Os::Unknown => vec![],
    }
}

#[allow(dead_code)]
pub fn get_install_command(os: Os) -> Option<(String, String)> {
    package_managers(os)
        .first()
        .map(|(name, cmd)| (name.to_string(), cmd.to_string()))
}

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
        Os::Unknown => "Please install ffmpeg from https://ffmpeg.org/download.html",
    }
}

pub fn prompt_and_install(os: Os) -> anyhow::Result<bool> {
    let pms = package_managers(os);
    if pms.is_empty() {
        println!("{}", get_manual_instructions(os));
        return Ok(false);
    }

    let (pm_name, _) = pms[0];
    print!("ffmpeg not found. Install via {pm_name}? [y/N]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_ok() {
        let input = input.trim().to_lowercase();
        if input == "y" || input == "yes" {
            return Ok(run_install(&pms));
        }
    }

    println!("{}", get_manual_instructions(os));
    Ok(false)
}

fn run_install(pms: &[(&str, &str)]) -> bool {
    if pms.is_empty() {
        return false;
    }

    let (_, cmd) = pms[0];
    println!("Running: {cmd}");

    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", cmd]).status()
    } else {
        Command::new("sh").args(["-c", cmd]).status()
    };

    match result {
        Ok(status) if status.success() => {
            if is_ffmpeg_available() {
                println!("ffmpeg installed successfully!");
                return true;
            }
            println!("Installation completed but ffmpeg not found in PATH.");
            println!("You may need to restart your terminal.");
        }
        Ok(status) => {
            println!("Installation failed with exit code: {:?}", status.code());
        }
        Err(e) => {
            println!("Failed to run installation: {e}");
        }
    }

    false
}

pub fn ensure_ffmpeg() -> anyhow::Result<bool> {
    if is_ffmpeg_available() {
        return Ok(true);
    }

    let os = detect_os();
    prompt_and_install(os)
}

pub fn build_merge_command(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    format: OutputFormat,
) -> Command {
    let mut cmd = Command::new("ffmpeg");

    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(video_path)
        .arg("-i")
        .arg(audio_path)
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("copy");

    if format.needs_faststart() {
        cmd.arg("-movflags").arg("+faststart");
    }

    cmd.arg("-y").arg(output_path);

    cmd
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
        assert!(matches!(
            os,
            Os::Windows | Os::MacOS | Os::Linux | Os::Unknown
        ));
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

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn test_ensure_ffmpeg_returns_true_if_available() {
        // If ffmpeg is installed, ensure_ffmpeg should return Ok(true) immediately
        // We can't easily test the prompt flow without mocking stdin
        if is_ffmpeg_available() {
            assert!(ensure_ffmpeg().unwrap());
        }
    }
}

#[cfg(test)]
mod cmd_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_merge_command_mkv_no_faststart() {
        let video = PathBuf::from("video.mp4");
        let audio = PathBuf::from("video.m4a");
        let output = PathBuf::from("output/video.mkv");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mkv);

        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("-hide_banner")));
        assert!(!args.contains(&std::ffi::OsStr::new("+faststart")));
    }

    #[test]
    fn test_build_merge_command_mp4_has_faststart() {
        let video = PathBuf::from("video.mp4");
        let audio = PathBuf::from("video.m4a");
        let output = PathBuf::from("output/video.mp4");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mp4);

        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("+faststart")));
    }

    #[test]
    fn test_build_merge_command_mov_has_faststart() {
        let video = PathBuf::from("video.mp4");
        let audio = PathBuf::from("video.m4a");
        let output = PathBuf::from("output/video.mov");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mov);

        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("+faststart")));
    }

    #[test]
    fn test_build_merge_command_has_copy_codecs() {
        let video = PathBuf::from("video.mp4");
        let audio = PathBuf::from("video.m4a");
        let output = PathBuf::from("output/video.mkv");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mkv);

        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("copy")));
    }

    #[test]
    fn test_build_merge_command_has_overwrite_flag() {
        let video = PathBuf::from("video.mp4");
        let audio = PathBuf::from("video.m4a");
        let output = PathBuf::from("output/video.mkv");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mkv);

        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("-y")));
    }
}
