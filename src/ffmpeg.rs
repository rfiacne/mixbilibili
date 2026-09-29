use crate::cli::OutputFormat;
use crate::i18n::{t, tf};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, ExitStatus};

pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Human-readable exit code of a finished process, for error messages.
pub fn exit_status_text(status: &ExitStatus) -> String {
    status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// Os variants are matched in get_manual_instructions/package_managers
// but only one variant is constructed at compile time via detect_os()
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Cross-platform: only one variant constructed per target
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
            let mut pms = Vec::new();
            if which::which("apt").is_ok() {
                pms.push(("apt", "sudo apt update && sudo apt install -y ffmpeg"));
            }
            if which::which("dnf").is_ok() {
                pms.push(("dnf", "sudo dnf install -y ffmpeg"));
            }
            if which::which("yum").is_ok() {
                pms.push(("yum", "sudo yum install -y ffmpeg"));
            }
            if which::which("pacman").is_ok() {
                pms.push(("pacman", "sudo pacman -S --noconfirm ffmpeg"));
            }
            if which::which("zypper").is_ok() {
                pms.push(("zypper", "sudo zypper install -y ffmpeg"));
            }
            pms
        }
        Os::Unknown => vec![],
    }
}

pub fn get_manual_instructions(os: Os) -> std::borrow::Cow<'static, str> {
    let key = match os {
        Os::Windows => "manual_instructions_windows",
        Os::MacOS => "manual_instructions_macos",
        Os::Linux => "manual_instructions_linux",
        Os::Unknown => "manual_instructions",
    };
    t(key)
}

pub fn prompt_and_install(os: Os) -> anyhow::Result<bool> {
    // The first detected package manager wins; with none, only manual steps remain.
    let Some((pm_name, install_cmd)) = package_managers(os).first().copied() else {
        println!("{}", get_manual_instructions(os));
        return Ok(false);
    };

    print!("{}", tf("install_ffmpeg_prompt", &[pm_name]));
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_ok() {
        let answer = input.trim().to_lowercase();
        if answer == "y" || answer == "yes" {
            return Ok(run_install(install_cmd));
        }
    }

    println!("{}", get_manual_instructions(os));
    Ok(false)
}

fn run_install(cmd: &str) -> bool {
    println!("{}", tf("running_cmd", &[cmd]));

    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", cmd]).status()
    } else {
        Command::new("sh").args(["-c", cmd]).status()
    };

    match result {
        Ok(status) if status.success() => {
            if is_ffmpeg_available() {
                println!("{}", t("ffmpeg_installed"));
                return true;
            }
            println!("{}", t("ffmpeg_not_found_path"));
        }
        Ok(status) => {
            println!(
                "{}",
                tf("install_failed_exit", &[&exit_status_text(&status)])
            );
        }
        Err(e) => {
            println!("{}", tf("install_failed_run", &[&e.to_string()]));
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
    // Resolve the absolute path once so a minimal PATH (e.g. aria2 hook) still
    // finds the same binary that is_ffmpeg_available()/ensure_ffmpeg() located.
    let mut cmd = Command::new(which::which("ffmpeg").unwrap_or_else(|_| "ffmpeg".into()));

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

/// Estimate merge time based on video file size and typical throughput.
/// Returns ZERO duration on error (file not found, permission denied, etc.).
pub fn estimate_merge_duration(video_path: &Path) -> std::time::Duration {
    let metadata = match std::fs::metadata(video_path) {
        Ok(m) => m,
        Err(_) => return std::time::Duration::ZERO,
    };

    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    // Estimate: ~10 MB/s throughput (conservative for typical hardware)
    let estimated_seconds = size_mb / 10.0;

    std::time::Duration::from_secs(estimated_seconds as u64)
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
        assert!(instructions.contains("winget") || instructions.contains("choco"));
    }

    #[test]
    fn test_get_manual_instructions_macos() {
        let instructions = get_manual_instructions(Os::MacOS);
        assert!(instructions.contains("brew") || instructions.contains("MacPorts"));
    }

    #[test]
    fn test_get_manual_instructions_linux() {
        let instructions = get_manual_instructions(Os::Linux);
        assert!(instructions.contains("apt") || instructions.contains("snap"));
    }

    #[test]
    fn test_get_manual_instructions_unknown() {
        let instructions = get_manual_instructions(Os::Unknown);
        assert!(instructions.contains("ffmpeg.org"));
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

#[cfg(test)]
mod estimate_tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[test]
    fn test_estimate_missing_file_returns_zero() {
        let path = PathBuf::from("/nonexistent/video.mp4");
        assert_eq!(estimate_merge_duration(&path), Duration::ZERO);
    }

    #[test]
    fn test_estimate_small_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        // Write ~1 MB
        tmp.write_all(&vec![0u8; 1024 * 1024]).unwrap();
        let dur = estimate_merge_duration(tmp.path());
        // 1 MB / 10 MB/s = 0.1s → rounds to 0
        assert_eq!(dur.as_secs(), 0);
    }

    #[test]
    fn test_estimate_medium_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        // Write ~50 MB
        tmp.write_all(&vec![0u8; 50 * 1024 * 1024]).unwrap();
        let dur = estimate_merge_duration(tmp.path());
        // 50 MB / 10 MB/s = 5s
        assert_eq!(dur.as_secs(), 5);
    }

    #[test]
    fn test_estimate_large_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        // Write ~200 MB
        tmp.write_all(&vec![0u8; 200 * 1024 * 1024]).unwrap();
        let dur = estimate_merge_duration(tmp.path());
        // 200 MB / 10 MB/s = 20s
        assert_eq!(dur.as_secs(), 20);
    }
}

/// Tests verifying the no-transcode constraint and full argument correctness.
#[cfg(test)]
mod no_transcode_tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: get all args from a command as Strings for easier assertion.
    fn args_vec(cmd: &std::process::Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn test_no_encoding_args_present() {
        // Ensure no transcoding codecs like libx264/aac/libvorbis sneak in
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &PathBuf::from("o.mkv"),
            OutputFormat::Mkv,
        );
        let args = args_vec(&cmd);
        let forbidden = [
            "libx264",
            "libx265",
            "aac",
            "libvorbis",
            "libopus",
            "h264",
            "hevc",
        ];
        for codec in &forbidden {
            assert!(
                !args.contains(&codec.to_string()),
                "Command should not contain encoding codec '{}', args: {:?}",
                codec,
                args
            );
        }
    }

    #[test]
    fn test_copy_codec_appears_for_both_streams() {
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &PathBuf::from("o.mkv"),
            OutputFormat::Mkv,
        );
        let args = args_vec(&cmd);

        // Find positions of -c:v and -c:a and verify their following arg is "copy"
        let cv_idx = args
            .iter()
            .position(|a| a == "-c:v")
            .expect("-c:v should exist");
        let ca_idx = args
            .iter()
            .position(|a| a == "-c:a")
            .expect("-c:a should exist");
        assert_eq!(args[cv_idx + 1], "copy", "-c:v must be followed by 'copy'");
        assert_eq!(args[ca_idx + 1], "copy", "-c:a must be followed by 'copy'");
    }

    #[test]
    fn test_input_paths_in_correct_order() {
        let video = PathBuf::from("/path/to/video.mp4");
        let audio = PathBuf::from("/path/to/audio.m4a");
        let output = PathBuf::from("/out/merged.mkv");

        let cmd = build_merge_command(&video, &audio, &output, OutputFormat::Mkv);
        let args = args_vec(&cmd);

        // First -i should be video, second -i should be audio
        let i_positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| a.as_str() == "-i")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(i_positions.len(), 2, "Should have exactly 2 -i flags");
        assert_eq!(args[i_positions[0] + 1], video.to_string_lossy());
        assert_eq!(args[i_positions[1] + 1], audio.to_string_lossy());
    }

    #[test]
    fn test_output_path_is_last_arg() {
        let output = PathBuf::from("/out/result.mkv");
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &output,
            OutputFormat::Mkv,
        );
        let args = args_vec(&cmd);
        assert_eq!(
            args.last().unwrap(),
            &output.to_string_lossy().to_string(),
            "Output path should be the last argument"
        );
    }

    #[test]
    fn test_mp4_format_adds_movflags_faststart() {
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &PathBuf::from("o.mp4"),
            OutputFormat::Mp4,
        );
        let args = args_vec(&cmd);
        let movflags_idx = args.iter().position(|a| a == "-movflags");
        assert!(movflags_idx.is_some(), "MP4 should have -movflags");
        assert_eq!(args[movflags_idx.unwrap() + 1], "+faststart");
    }

    #[test]
    fn test_mkv_format_no_movflags() {
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &PathBuf::from("o.mkv"),
            OutputFormat::Mkv,
        );
        let args = args_vec(&cmd);
        assert!(
            !args.contains(&"-movflags".to_string()),
            "MKV should not have -movflags"
        );
    }

    #[test]
    fn test_command_program_is_ffmpeg() {
        let cmd = build_merge_command(
            &PathBuf::from("v.mp4"),
            &PathBuf::from("a.m4a"),
            &PathBuf::from("o.mkv"),
            OutputFormat::Mkv,
        );
        let program = cmd.get_program().to_string_lossy().to_string();
        assert!(
            program == "ffmpeg" || program.ends_with("ffmpeg") || program.ends_with("ffmpeg.exe"),
            "program should be ffmpeg (or its resolved path): {}",
            program
        );
    }

    #[test]
    fn test_estimate_empty_file_returns_zero() {
        use std::time::Duration;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        // 0 bytes file
        let dur = estimate_merge_duration(tmp.path());
        assert_eq!(dur, Duration::ZERO);
    }
}
