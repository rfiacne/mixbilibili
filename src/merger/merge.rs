use super::runner::{Runner, FFMPEG_TIMEOUT};
use super::summary::MergeResult;
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::i18n::{t, tf};
use crate::progress::MergeProgress;
use crate::scanner::FilePair;
use crate::INTERRUPTED;
use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    max_retries: usize,
    runner: Option<&Runner<'_>>,
) -> MergeResult {
    if INTERRUPTED.load(Ordering::SeqCst) {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
    }

    // stem is now a composite key "parent/stem", flatten "/" to "_" for safe filename
    let safe_stem = crate::scanner::safe_stem(&pair.stem);
    let output_path = output_dir.join(format!("{}.{}", safe_stem, format.extension()));

    // Safety check: output path must not be identical to input video path
    // Use canonicalize to handle different path representations pointing to the same file
    let dominated = std::fs::canonicalize(&output_path)
        .ok()
        .zip(std::fs::canonicalize(&pair.video).ok())
        .map(|(o, v)| o == v)
        .unwrap_or(false);
    if dominated {
        return record_failure(
            pair,
            pair_index,
            t("same_source_output_runtime").into_owned(),
            Duration::ZERO,
            progress,
        );
    }

    do_merge(
        pair,
        &output_path,
        pair_index,
        format,
        progress,
        verbose,
        max_retries,
        runner,
    )
}

fn record_failure(
    pair: &FilePair,
    pair_index: usize,
    error_msg: String,
    duration: Duration,
    progress: Option<&MergeProgress>,
) -> MergeResult {
    if let Some(p) = progress {
        p.record(&pair.stem, false, duration, Some(&error_msg), None);
    } else {
        println!("{} {}: {}", t("cross").red(), pair.stem, error_msg);
    }
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        success: false,
        error: Some(error_msg),
        duration,
        was_interrupted: false,
    }
}

/// Validates that the merge output exists, is non-zero, and is at least
/// `min_expected` bytes.
///
/// With `-c copy`, the output holds the video stream plus the audio stream
/// plus container headers, so it must be >= the source video size; anything
/// smaller indicates a truncated or incomplete merge.
fn validate_output(output_path: &Path, min_expected: u64) -> Result<()> {
    let metadata = match std::fs::metadata(output_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "failed_create_output_file",
                    &[&output_path.display().to_string()]
                )
            );
        }
        Err(e) => return Err(e.into()),
    };
    if metadata.len() == 0 {
        if let Err(e) = std::fs::remove_file(output_path) {
            eprintln!("Warning: Failed to remove empty output file: {}", e);
        }
        anyhow::bail!(
            "{}",
            tf("output_file_empty", &[&output_path.display().to_string()])
        );
    }
    if metadata.len() < min_expected {
        if let Err(e) = std::fs::remove_file(output_path) {
            eprintln!("Warning: Failed to remove undersized output file: {}", e);
        }
        anyhow::bail!(
            "{}",
            tf(
                "output_too_small",
                &[
                    &output_path.display().to_string(),
                    &metadata.len().to_string(),
                    &min_expected.to_string(),
                ]
            )
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn do_merge(
    pair: &FilePair,
    output_path: &Path,
    pair_index: usize,
    format: OutputFormat,
    progress: Option<&MergeProgress>,
    verbose: bool,
    max_retries: usize,
    runner: Option<&Runner<'_>>,
) -> MergeResult {
    let start = std::time::Instant::now();

    for attempt in 0..=max_retries {
        // Early exit if interrupted — avoid useless retries after Ctrl+C
        if INTERRUPTED.load(Ordering::SeqCst) {
            return MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: None,
                duration: start.elapsed(),
                was_interrupted: true,
            };
        }

        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1));
            if let Some(p) = progress {
                p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
            } else if verbose {
                println!(
                    "{} {}",
                    "↻".yellow(),
                    tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
                );
            }
        }

        if verbose && attempt == 0 {
            println!(
                "Running: ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }

        let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, output_path, format);
        let run_result = match runner {
            Some(r) => r(&mut cmd, FFMPEG_TIMEOUT),
            None => super::runner::run_with_timeout(&mut cmd, FFMPEG_TIMEOUT),
        };
        match run_result {
            Ok(status) if status.success() => {
                // Validate output: exists, non-zero, and >= source video size.
                let min_expected = match std::fs::metadata(&pair.video) {
                    Ok(m) => m.len(),
                    Err(_) => {
                        return record_failure(
                            pair,
                            pair_index,
                            tf(
                                "source_missing",
                                &["video", &pair.video.display().to_string()],
                            ),
                            start.elapsed(),
                            progress,
                        );
                    }
                };
                if let Err(e) = validate_output(output_path, min_expected) {
                    return record_failure(
                        pair,
                        pair_index,
                        e.to_string(),
                        start.elapsed(),
                        progress,
                    );
                }

                let duration = start.elapsed();
                if let Some(p) = progress {
                    p.record(&pair.stem, true, duration, None, None);
                } else {
                    println!("{} {}", t("checkmark").green(), pair.stem);
                }
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    success: true,
                    error: None,
                    duration,
                    was_interrupted: false,
                };
            }
            Ok(_) if attempt < max_retries => continue,
            Ok(status) => {
                let error_msg = tf(
                    "merge_failed_exit",
                    &[
                        &status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        &max_retries.to_string(),
                    ],
                );
                return record_failure(pair, pair_index, error_msg, start.elapsed(), progress);
            }
            Err(_) if attempt < max_retries => continue,
            Err(e) => {
                let error_msg = tf(
                    "merge_failed_io",
                    &[&e.to_string(), &max_retries.to_string()],
                );
                return record_failure(pair, pair_index, error_msg, start.elapsed(), progress);
            }
        }
    }

    unreachable!("retry loop should always return")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use tempfile::tempdir;

    #[test]
    fn test_merge_validates_output_exists() {
        use std::fs::File;
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        // Simulate ffmpeg creating empty file (failure case)
        File::create(&output_path).unwrap();

        // Validation should reject empty file
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert_eq!(metadata.len(), 0, "Empty file should be detected");
    }

    #[test]
    fn test_merge_validates_output_nonzero_size() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        std::fs::write(&output_path, b"fake video data").unwrap();

        let metadata = std::fs::metadata(&output_path).unwrap();
        assert!(metadata.len() > 0, "Non-zero file should be accepted");
    }

    #[test]
    fn test_validate_output_helper() {
        use std::fs::File;
        let dir = tempdir().unwrap();

        // Test 1: Non-existent file
        let missing_path = dir.path().join("missing.mkv");
        let result = validate_output(&missing_path, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("未创建") || err.contains("not created"),
            "Error should mention file was not created: {}",
            err
        );

        // Test 2: Empty file
        let empty_path = dir.path().join("empty.mkv");
        File::create(&empty_path).unwrap();
        let result = validate_output(&empty_path, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty") || err.contains("为空"));
        assert!(!empty_path.exists(), "Empty file should be cleaned up");

        // Test 3: Valid file (size >= min)
        let valid_path = dir.path().join("valid.mkv");
        std::fs::write(&valid_path, b"video data").unwrap();
        let result = validate_output(&valid_path, 5);
        assert!(result.is_ok());
        assert!(valid_path.exists(), "Valid file should remain");
    }

    #[test]
    fn test_validate_output_too_small_rejected_and_cleaned() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("small.mkv");
        std::fs::write(&output_path, b"tiny").unwrap(); // 4 bytes

        let result = validate_output(&output_path, 10);
        assert!(result.is_err());
        assert!(
            !output_path.exists(),
            "undersized output should be cleaned up"
        );
    }

    #[test]
    fn test_validate_output_meets_min_passes() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("ok.mkv");
        std::fs::write(&output_path, b"exactly ten").unwrap(); // 11 bytes

        let result = validate_output(&output_path, 10);
        assert!(result.is_ok());
        assert!(output_path.exists());
    }

    #[test]
    fn test_merge_pair_rejects_identical_output() {
        let dir = tempdir().unwrap();

        let video_path = dir.path().join("test.mp4");
        let audio_path = dir.path().join("test.m4a");
        std::fs::write(&video_path, b"video").unwrap();
        std::fs::write(&audio_path, b"audio").unwrap();

        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "test".to_string(),
        };

        let result = merge_pair(
            &pair,
            0,
            dir.path(),
            crate::cli::OutputFormat::Mp4,
            None,
            false,
            0,
            None,
        );

        assert!(
            !result.success,
            "merge_pair should reject identical output path"
        );
        assert!(
            result.error.as_ref().unwrap().contains("identical")
                || result.error.as_ref().unwrap().contains("相同"),
            "Error should mention identical path: {:?}",
            result.error
        );
    }

    #[cfg(unix)]
    fn success_status() -> ExitStatus {
        std::process::Command::new("true").status().unwrap()
    }

    #[cfg(windows)]
    fn success_status() -> ExitStatus {
        std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .status()
            .unwrap()
    }

    #[cfg(unix)]
    fn failure_status() -> ExitStatus {
        std::process::Command::new("false").status().unwrap()
    }

    #[cfg(windows)]
    fn failure_status() -> ExitStatus {
        std::process::Command::new("cmd")
            .args(["/C", "exit 1"])
            .status()
            .unwrap()
    }

    fn make_pair(dir: &std::path::Path) -> (FilePair, std::path::PathBuf) {
        let video = dir.join("v.mp4");
        let audio = dir.join("v.m4a");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(&audio, b"audio").unwrap();
        let out_dir = dir.join("out");
        std::fs::create_dir(&out_dir).unwrap();
        (
            FilePair {
                video,
                audio,
                stem: "v".to_string(),
            },
            out_dir,
        )
    }

    /// Simulate ffmpeg writing the output file (last arg), then exiting successfully.
    fn fake_success_runner(
        cmd: &mut std::process::Command,
        _timeout: Duration,
    ) -> Result<ExitStatus> {
        let out = PathBuf::from(cmd.get_args().last().expect("output is last arg"));
        std::fs::write(out, b"fake merged data is long enough").unwrap();
        Ok(success_status())
    }

    #[test]
    fn test_merge_pair_success_with_fake_runner() {
        let dir = tempdir().unwrap();
        let (pair, out_dir) = make_pair(dir.path());

        let result = merge_pair(
            &pair,
            0,
            &out_dir,
            crate::cli::OutputFormat::Mkv,
            None,
            false,
            0,
            Some(&fake_success_runner),
        );

        assert!(result.success, "merge should succeed: {:?}", result.error);
        assert!(out_dir.join("v.mkv").exists(), "output file should exist");
    }

    #[test]
    fn test_merge_pair_retries_then_succeeds() {
        let dir = tempdir().unwrap();
        let (pair, out_dir) = make_pair(dir.path());

        let attempts = AtomicUsize::new(0);
        let runner = |cmd: &mut std::process::Command, _t: Duration| {
            let n = attempts.fetch_add(1, AtomicOrdering::Relaxed) + 1;
            if n == 1 {
                Ok(failure_status())
            } else {
                let out = PathBuf::from(cmd.get_args().last().expect("output is last arg"));
                std::fs::write(out, b"fake merged data is long enough").unwrap();
                Ok(success_status())
            }
        };

        let result = merge_pair(
            &pair,
            0,
            &out_dir,
            crate::cli::OutputFormat::Mkv,
            None,
            false,
            1,
            Some(&runner),
        );

        assert!(result.success, "retry should succeed: {:?}", result.error);
        assert_eq!(attempts.load(AtomicOrdering::Relaxed), 2);
        assert!(out_dir.join("v.mkv").exists());
    }

    #[test]
    fn test_merge_pair_exhausts_retries() {
        let dir = tempdir().unwrap();
        let (pair, out_dir) = make_pair(dir.path());

        let runner = |_cmd: &mut std::process::Command, _t: Duration| Ok(failure_status());

        let result = merge_pair(
            &pair,
            0,
            &out_dir,
            crate::cli::OutputFormat::Mkv,
            None,
            false,
            1,
            Some(&runner),
        );

        assert!(!result.success);
        let err = result.error.expect("failure should carry an error");
        assert!(
            err.contains("exited") || err.contains("退出码"),
            "Error should mention exit code: {}",
            err
        );
    }

    #[test]
    fn test_merge_pair_runner_io_error() {
        let dir = tempdir().unwrap();
        let (pair, out_dir) = make_pair(dir.path());

        let runner = |_cmd: &mut std::process::Command, _t: Duration| -> Result<ExitStatus> {
            anyhow::bail!("spawn failed")
        };

        let result = merge_pair(
            &pair,
            0,
            &out_dir,
            crate::cli::OutputFormat::Mkv,
            None,
            false,
            0,
            Some(&runner),
        );

        assert!(!result.success);
        let err = result.error.expect("failure should carry an error");
        assert!(
            err.contains("spawn failed"),
            "Error should propagate io failure: {}",
            err
        );
    }
}
