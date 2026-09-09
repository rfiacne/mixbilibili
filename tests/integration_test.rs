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

#[test]
fn test_recursive_merge_mirrors_subdirs() {
    if !ffmpeg_available() {
        eprintln!("Skipping: ffmpeg not available");
        return;
    }
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let output = dir.path().join("output");
    let season = source.join("season1");
    fs::create_dir_all(&season).unwrap();
    create_test_media(&season, "ep01");

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
        output.join("season1").join("ep01.mkv").exists(),
        "recursive merge must mirror the source subdir into the output"
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
