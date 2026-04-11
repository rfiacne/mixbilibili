// tests/integration_test.rs
use std::process::Command;
use tempfile::tempdir;

fn get_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("mixbilibili");
    path
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
    assert!(stderr.contains("does not exist") || stderr.contains("Error"));
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
    assert!(stdout.contains("No file pairs to merge"));
}
