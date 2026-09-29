use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn get_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("mixbilibili");
    path
}

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn create_test_media(dir: &std::path::Path, name: &str) {
    let video_path = dir.join(format!("{name}.mp4"));
    let audio_path = dir.join(format!("{name}.m4a"));

    let video_str = video_path.display().to_string();
    let audio_str = audio_path.display().to_string();

    Command::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=1",
            "-y",
            &video_str,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("Failed to create test video");

    Command::new("ffmpeg")
        .args(["-f", "lavfi", "-i", "sine=f=440:d=1", "-y", &audio_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("Failed to create test audio");
}

/// Waits for a child, killing it and failing the test if it outlives `timeout`,
/// so a binary that ignores Ctrl+C cannot hang the whole test suite.
#[cfg(unix)]
fn wait_bounded(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait failed") {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process did not exit within {timeout:?} after SIGINT");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn test_help_flag() {
    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--source"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--format"));
    assert!(stdout.contains("--jobs"));
}

#[test]
fn test_invalid_format() {
    let output = Command::new(get_binary_path())
        .arg("-f")
        .arg("avi")
        .output()
        .expect("Failed to execute binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value") && stderr.contains("avi"),
        "Expected error message not found in: {}",
        stderr
    );
}

#[test]
fn test_nonexistent_source() {
    let output = Command::new(get_binary_path())
        .arg("-s")
        .arg("/nonexistent/path/12345")
        .output()
        .expect("Failed to execute binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not exist") || stderr.contains("Error") || stderr.contains("错误"),
        "Expected error message not found in: {}",
        stderr
    );
}

#[test]
fn test_empty_directory() {
    let dir = tempdir().unwrap();

    let output = Command::new(get_binary_path())
        .arg("-s")
        .arg(dir.path())
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No file pairs to merge") || stdout.contains("没有找到可合并的文件对"),
        "Expected output not found in: {}",
        stdout
    );
}

#[test]
fn test_basic_merge() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "test_video");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
        ])
        .output()
        .expect("Failed to execute binary");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "Command failed: {}", stderr);

    // Non-recursive mode: output uses bare stem
    let output_file = output.join("test_video.mkv");
    assert!(
        output_file.exists(),
        "Output file should exist: {:?}",
        output_file
    );

    assert!(
        source.join("test_video.mp4").exists(),
        "Source video should be kept"
    );
    assert!(
        source.join("test_video.m4a").exists(),
        "Source audio should be kept"
    );
}

#[test]
fn test_dry_run_no_output() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "dry_video");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--dry-run",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());

    let output_file = output.join("dry_video.mkv");
    assert!(
        !output_file.exists(),
        "Dry-run should not create output file"
    );
}

#[test]
fn test_merge_with_delete_source() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "del_video");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());

    // Non-recursive mode: output uses bare stem
    let output_file = output.join("del_video.mkv");
    assert!(output_file.exists(), "Output file should exist");

    assert!(
        !source.join("del_video.mp4").exists(),
        "Source video should be deleted"
    );
    assert!(
        !source.join("del_video.m4a").exists(),
        "Source audio should be deleted"
    );
}

#[test]
fn test_recursive_flag() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let subdir = source.join("season1");
    fs::create_dir_all(&subdir).unwrap();

    fs::File::create(subdir.join("ep01.mp4")).unwrap();
    fs::File::create(subdir.join("ep01.m4a")).unwrap();

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "--dry-run",
            "--recursive",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("ep01"),
        "Should find ep01 in subdirectory: {}",
        stdout
    );
}

#[test]
fn test_recursive_not_default() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let subdir = source.join("season1");
    fs::create_dir_all(&subdir).unwrap();

    fs::File::create(subdir.join("ep01.mp4")).unwrap();
    fs::File::create(subdir.join("ep01.m4a")).unwrap();

    let result = Command::new(get_binary_path())
        .args(["-s", &source.display().to_string(), "--dry-run"])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        !stdout.contains("ep01"),
        "Without --recursive should not find ep01: {}",
        stdout
    );
}

#[test]
fn test_once_mode_merge() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "once_video");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
            "--once",
            "once_video",
        ])
        .output()
        .expect("Failed to execute binary");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "Command failed: {}", stderr);

    let output_file = output.join("once_video.mkv");
    assert!(
        output_file.exists(),
        "Output file should exist: {:?}",
        output_file
    );

    // Verify output is non-empty
    let metadata = fs::metadata(&output_file).unwrap();
    assert!(metadata.len() > 0, "Output file should not be empty");

    // Source files should be kept (--sdel false)
    assert!(
        source.join("once_video.mp4").exists(),
        "Source video should be kept"
    );
    assert!(
        source.join("once_video.m4a").exists(),
        "Source audio should be kept"
    );
}

/// `--dry-run` promises "no files created or deleted". `--once` used to return
/// before the dry-run branch, so `-n` merged for real and — with the default
/// `--sdel` — deleted both sources.
#[test]
fn test_once_dry_run_does_not_touch_files() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "once_video");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--once",
            "once_video",
            "-n",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());

    assert!(
        source.join("once_video.mp4").exists() && source.join("once_video.m4a").exists(),
        "a dry run must not delete sources"
    );
    assert_eq!(
        fs::read_dir(&output).unwrap().count(),
        0,
        "a dry run must not create output"
    );
}

#[test]
fn test_resume_after_partial() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    // Create two pairs
    create_test_media(&source, "video1");
    create_test_media(&source, "video2");

    // First run: merge both
    let result1 = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
        ])
        .output()
        .expect("Failed to execute first run");

    assert!(result1.status.success());
    // Non-recursive mode: output uses bare stem
    assert!(output.join("video1.mkv").exists());
    assert!(output.join("video2.mkv").exists());

    // Delete output files to simulate partial state
    fs::remove_file(output.join("video1.mkv")).unwrap();
    fs::remove_file(output.join("video2.mkv")).unwrap();

    // Create a state file marking video1 as completed (bare stem in non-recursive)
    let state_content = format!(
        r#"{{
  "version": 1,
  "source_dir": "{}",
  "output_dir": "{}",
  "completed": ["video1"],
  "failed": [],
  "pending": []
}}"#,
        source.display(),
        output.display()
    );
    fs::write(source.join(".mixbilibili_state.json"), state_content).unwrap();

    // Second run with --resume: should only merge video2
    let result2 = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
            "--resume",
        ])
        .output()
        .expect("Failed to execute resume run");

    assert!(result2.status.success());

    // video1 should NOT be recreated (marked as completed)
    assert!(
        !output.join("video1.mkv").exists(),
        "video1 should not be re-merged"
    );

    // video2 should be merged
    assert!(
        output.join("video2.mkv").exists(),
        "video2 should be merged on resume"
    );

    // State file should be cleared after successful completion
    assert!(
        !source.join(".mixbilibili_state.json").exists(),
        "State file should be cleared after success"
    );
}

/// Ctrl+C during a batch must stop cleanly (exit 0), keep every source file, and
/// keep the resume checkpoint — otherwise the `--resume` hint printed right after
/// the interrupt is a lie and the next run redoes work that was already finished.
///
/// Regression: `finalize` dropped the state file whenever no merge had *failed*.
/// Interrupted pairs are not failures, so cancelling wiped the checkpoint.
#[cfg(unix)]
#[test]
fn test_interrupt_keeps_checkpoint_and_sources() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    // One real pair, copied many times. A `-c copy` merge takes ~45 ms, so this
    // batch cannot finish while the signal is on its way.
    const PAIRS: usize = 200;
    create_test_media(&source, "template");
    let video = fs::read(source.join("template.mp4")).unwrap();
    let audio = fs::read(source.join("template.m4a")).unwrap();
    fs::remove_file(source.join("template.mp4")).unwrap();
    fs::remove_file(source.join("template.m4a")).unwrap();
    for i in 0..PAIRS {
        let stem = format!("video{i:03}");
        fs::write(source.join(format!("{stem}.mp4")), &video).unwrap();
        fs::write(source.join(format!("{stem}.m4a")), &audio).unwrap();
    }

    let mut child = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
            "-j",
            "1",
            "-q",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn binary");

    // Interrupt once the batch is demonstrably under way instead of after a fixed
    // delay: with `-j 1` only one merge runs at a time, so seeing 10 outputs means
    // at least 9 pairs finished. Waiting on progress keeps this independent of how
    // fast the machine is, while still guaranteeing work is left over to interrupt.
    let deadline = Instant::now() + Duration::from_secs(60);
    while fs::read_dir(&output).unwrap().count() < 10 {
        assert!(
            Instant::now() < deadline,
            "no merge progress within 60s; ffmpeg may be hanging"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let signalled = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("Failed to run kill");
    assert!(signalled.success(), "SIGINT should be delivered");

    let status = wait_bounded(&mut child, Duration::from_secs(30));
    assert!(
        status.success(),
        "Ctrl+C is a clean stop, not a merge failure"
    );

    // Sources must all survive: `ffmpeg -c copy` never consumes them, and the
    // pending pairs still need them for `--resume`.
    assert!(source.join("video000.mp4").exists());
    assert!(source.join("video199.m4a").exists());

    // Interrupted merges leave no partial output behind.
    let outputs = fs::read_dir(&output).unwrap().count();
    assert!(outputs < PAIRS, "unfinished outputs left behind: {outputs}");

    let state_path = source.join(".mixbilibili_state.json");
    assert!(
        state_path.exists(),
        "the interrupt must keep the resume checkpoint at {}",
        state_path.display()
    );

    let json = fs::read_to_string(&state_path).unwrap();
    assert!(
        !json.contains("\"completed\": []"),
        "pairs finished before the interrupt must be recorded: {json}"
    );
    assert!(
        !json.contains("\"pending\": []"),
        "pairs skipped by the interrupt must stay pending: {json}"
    );
}

/// Ctrl+C during `--once` must exit cleanly too. There is no checkpoint to keep
/// in single-pair mode, but an interrupt is still the user's doing rather than a
/// merge failure, so it must not be reported (or counted) as one.
#[cfg(unix)]
#[test]
fn test_interrupt_during_once_exits_cleanly() {
    use std::process::Stdio;
    use std::time::Duration;

    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    // Named pipes instead of real media: ffmpeg blocks on them forever, so the
    // interrupt is guaranteed to land while a merge is in flight rather than
    // racing a copy that finishes in milliseconds. (With `--once` the pair is
    // addressed directly, so the scanner's symlink/skip rules never apply.)
    for name in ["video.mp4", "video.m4a"] {
        let created = Command::new("mkfifo")
            .arg(source.join(name))
            .status()
            .expect("Failed to run mkfifo");
        assert!(created.success(), "mkfifo should succeed");
    }

    let mut child = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--once",
            "video",
            "--sdel",
            "false",
            "-q",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn binary");

    // Either the merge is already blocked on the fifo, or Ctrl+C arrives first
    // and the pair is skipped — both must end in the same clean exit.
    std::thread::sleep(Duration::from_millis(500));

    let signalled = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("Failed to run kill");
    assert!(signalled.success(), "SIGINT should be delivered");

    let status = wait_bounded(&mut child, Duration::from_secs(30));
    assert!(
        status.success(),
        "Ctrl+C during --once is not a merge failure"
    );

    assert!(
        source.join("video.mp4").exists(),
        "the interrupted source must survive"
    );
    assert_eq!(
        fs::read_dir(&output).unwrap().count(),
        0,
        "no output should be left behind"
    );
}
