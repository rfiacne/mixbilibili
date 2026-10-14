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
fn test_dry_run_once_previews_without_touching_files() {
    // Regression: --once used to bypass --dry-run entirely and really merge
    // (and with default --sdel, really delete) the pair.
    if !ffmpeg_available() {
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
            "--dry-run",
            "--once",
            "once_video",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());
    assert!(
        source.join("once_video.mp4").exists() && source.join("once_video.m4a").exists(),
        "--dry-run --once must not delete sources"
    );
    assert!(
        !output.join("once_video.mkv").exists(),
        "--dry-run --once must not produce output"
    );
}

#[test]
fn test_once_merges_single_pair() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();
    create_test_media(&source, "one");
    create_test_media(&source, "two");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
            "--once",
            "one",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());
    assert!(output.join("one.mkv").exists(), "--once merges its stem");
    assert!(
        !output.join("two.mkv").exists(),
        "--once must not touch other pairs"
    );
}

#[test]
fn test_once_rejects_path_traversal_stem() {
    let dir = tempdir().unwrap();
    let result = Command::new(get_binary_path())
        .args(["-s", &dir.path().display().to_string(), "--once", "../evil"])
        .output()
        .expect("Failed to execute binary");

    assert!(!result.status.success(), "traversal stem must fail");
    assert!(
        !result.stderr.is_empty(),
        "traversal rejection should explain itself"
    );
}

// Same stem in two subdirs must produce two distinct outputs, never one
// silently overwriting the other. Naming follows the documented contract:
// the composite key flattens (season1/ep01 -> season1_ep01.mkv).
#[test]
fn test_recursive_same_stem_pairs_both_survive() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir_all(source.join("season1")).unwrap();
    fs::create_dir_all(source.join("season2")).unwrap();
    create_test_media(&source.join("season1"), "ep01");
    create_test_media(&source.join("season2"), "ep01");

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "--sdel",
            "false",
            "--recursive",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(result.status.success());
    assert!(
        output.join("season1_ep01.mkv").exists(),
        "season1/ep01 must merge to its own output"
    );
    assert!(
        output.join("season2_ep01.mkv").exists(),
        "season2/ep01 must merge to its own output"
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

    // Create a state file marking video1 as completed (bare stem in non-recursive).
    // Escape backslashes so Windows temp paths embed validly in the JSON string.
    let esc = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let state_content = format!(
        r#"{{
  "version": 1,
  "source_dir": "{}",
  "output_dir": "{}",
  "completed": ["video1"],
  "failed": [],
  "pending": []
}}"#,
        esc(&source),
        esc(&output)
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

/// `scripts/aria2-hook.sh` shells out to exactly this command once both
/// downloads have landed. The hook is bash and so is untested by this suite,
/// so the contract it depends on is pinned here: merge in place, remove both
/// sources, stay silent, and leave nothing to resume.
#[test]
fn test_once_quiet_contract_used_by_the_aria2_hook() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    create_test_media(dir.path(), "hook_video");

    let dir_str = dir.path().display().to_string();
    let output = Command::new(get_binary_path())
        .args([
            "--once",
            "hook_video",
            "-s",
            &dir_str,
            "-o",
            &dir_str,
            "--sdel",
            "true",
            "--quiet",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success(), "hook call must succeed");
    assert!(
        output.stdout.is_empty(),
        "--quiet must stay silent for the hook: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        dir.path().join("hook_video.mkv").exists(),
        "merged in place"
    );
    assert!(
        !dir.path().join("hook_video.mp4").exists() && !dir.path().join("hook_video.m4a").exists(),
        "--sdel true removes both sources"
    );
    assert!(
        !dir.path().join(".mixbilibili_state.json").exists(),
        "--once leaves no resume state behind"
    );
}

/// `-q` promises summary-only. ffmpeg's stderr is captured rather than
/// inherited, so its diagnostics cannot leak past the promise — and cannot
/// scramble the progress bar, which draws on the same stream.
#[test]
fn test_quiet_mode_leaks_no_ffmpeg_diagnostics() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();

    create_test_media(&source, "good_video");
    // Not a video: ffmpeg will refuse it, which is exactly what used to leak.
    fs::write(source.join("bad_video.mp4"), b"not a video").unwrap();
    fs::write(source.join("bad_video.m4a"), b"not audio").unwrap();

    let result = Command::new(get_binary_path())
        .args([
            "-s",
            &source.display().to_string(),
            "-o",
            &output.display().to_string(),
            "-q",
            "--sdel",
            "false",
        ])
        .output()
        .expect("Failed to execute binary");

    assert!(!result.status.success(), "the bad pair should fail the run");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    for leak in ["moov atom", "Error opening", "Invalid data"] {
        assert!(
            !combined.contains(leak),
            "-q leaked ffmpeg's own output ({leak}): {combined}"
        );
    }
    assert!(
        !combined.contains("✓ good_video"),
        "-q should not list pairs: {combined}"
    );
}
