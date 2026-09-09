use crate::cli::OutputFormat;
use crate::i18n::{t, tf};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;

pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// First available (name, install_command) package manager candidate for the
/// current OS, per `std::env::consts::OS`.
fn first_package_manager() -> Option<(&'static str, &'static str)> {
    let candidates: &[(&str, &str)] = match std::env::consts::OS {
        "windows" => &[
            ("winget", "winget install ffmpeg"),
            ("choco", "choco install ffmpeg -y"),
        ],
        "macos" => &[("brew", "brew install ffmpeg")],
        "linux" => &[
            ("apt", "sudo apt update && sudo apt install -y ffmpeg"),
            ("dnf", "sudo dnf install -y ffmpeg"),
            ("yum", "sudo yum install -y ffmpeg"),
            ("pacman", "sudo pacman -S --noconfirm ffmpeg"),
            ("zypper", "sudo zypper install -y ffmpeg"),
        ],
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .find(|(name, _)| which::which(name).is_ok())
}

pub fn get_manual_instructions(os: &str) -> std::borrow::Cow<'static, str> {
    let key = match os {
        "windows" => "manual_instructions_windows",
        "macos" => "manual_instructions_macos",
        "linux" => "manual_instructions_linux",
        _ => "manual_instructions",
    };
    t(key)
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
                println!("{}", t("install_success"));
                return true;
            }
            println!("{}", t("install_failed_notfound"));
        }
        Ok(status) => {
            println!(
                "{}",
                tf(
                    "install_failed_exit",
                    &[&status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string())]
                )
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

    let Some((pm_name, cmd)) = first_package_manager() else {
        println!("{}", get_manual_instructions(std::env::consts::OS));
        return Ok(false);
    };

    print!("{}", tf("install_ffmpeg_prompt", &[pm_name]));
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_ok() {
        let input = input.trim().to_lowercase();
        if input == "y" || input == "yes" {
            return Ok(run_install(cmd));
        }
    }

    println!("{}", get_manual_instructions(std::env::consts::OS));
    Ok(false)
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
mod pm_tests {
    use super::*;

    #[test]
    fn test_get_manual_instructions_windows() {
        let instructions = get_manual_instructions("windows");
        assert!(instructions.contains("winget") || instructions.contains("choco"));
    }

    #[test]
    fn test_get_manual_instructions_macos() {
        let instructions = get_manual_instructions("macos");
        assert!(instructions.contains("brew") || instructions.contains("MacPorts"));
    }

    #[test]
    fn test_get_manual_instructions_linux() {
        let instructions = get_manual_instructions("linux");
        assert!(instructions.contains("apt") || instructions.contains("snap"));
    }

    #[test]
    fn test_get_manual_instructions_unknown() {
        let instructions = get_manual_instructions("haiku");
        assert!(instructions.contains("ffmpeg.org"));
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
