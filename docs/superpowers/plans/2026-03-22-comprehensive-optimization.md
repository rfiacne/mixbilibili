# mixbilibili 综合优化实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 优化 mixbilibili CLI 工具的错误处理、测试覆盖、CI/CD、性能、代码质量、依赖管理和文档。

**Architecture:** 三阶段渐进式优化：P1 高优先级（错误处理+测试+CI/CD）→ P2 中优先级（性能+代码质量+验证）→ P3 低优先级（依赖+文档+安全）。每个阶段独立可验证。

**Tech Stack:** Rust Edition 2021, clap 4.x, rayon 1.x, anyhow (新增), GitHub Actions

---

## 文件结构

| 文件 | 责任 | 变更类型 |
|------|------|----------|
| `Cargo.toml` | 依赖管理 | 修改 |
| `src/main.rs` | 程序入口，退出码 | 修改 |
| `src/cli.rs` | CLI 参数解析和验证 | 修改 |
| `src/scanner.rs` | 目录扫描，文件配对 | 修改 |
| `src/merger.rs` | 并行合并执行 | 修改 |
| `src/ffmpeg.rs` | ffmpeg 检测和命令构建 | 修改 |
| `.github/workflows/ci.yml` | CI/CD 配置 | 新增 |
| `CHANGELOG.md` | 版本变更记录 | 新增 |
| `CONTRIBUTING.md` | 贡献指南 | 新增 |

---

## Chunk 1: P1.1 错误处理重构

### Task 1: 添加 anyhow 依赖

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: 添加 anyhow 到依赖**

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
which = "6"
rayon = "1"
num_cpus = "1"
colored = "2"
anyhow = "1.0"
```

- [ ] **Step 2: 运行 cargo check 验证依赖**

Run: `cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: 提交**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add anyhow dependency for error handling"
```

---

### Task 2: 重构 scanner.rs 错误处理

**Files:**
- Modify: `src/scanner.rs:1-130`

- [ ] **Step 1: 添加 anyhow 导入**

```rust
// src/scanner.rs 顶部
use anyhow::{Result, Context};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
```

- [ ] **Step 2: 修改 scan_directory 函数签名和错误处理**

```rust
/// Scan a directory for mp4/m4a file pairs
///
/// # Arguments
/// * `source_dir` - Directory to scan
///
/// # Returns
/// A `ScanResult` containing matched pairs and statistics.
///
/// # Errors
/// Returns an error if the directory cannot be read.
pub fn scan_directory(source_dir: &Path) -> Result<ScanResult> {
    if !source_dir.exists() {
        anyhow::bail!("Source directory does not exist: {}", source_dir.display());
    }

    if !source_dir.is_dir() {
        anyhow::bail!("Source path is not a directory: {}", source_dir.display());
    }

    // Check read permission
    let entries = fs::read_dir(source_dir)
        .context("Failed to read directory")?;

    // ... 其余代码保持不变 ...

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        // ... 其余代码保持不变 ...
    }

    // ... 其余代码保持不变 ...

    Ok(ScanResult { pairs, stats, skipped_names })
}
```

- [ ] **Step 3: 运行 cargo check 验证编译**

Run: `cargo check`
Expected: 编译成功

- [ ] **Step 4: 更新现有 scanner 测试以适配新错误类型**

**注意**: 以下测试已存在于 `src/scanner.rs:171-186`，只需更新错误断言：

```rust
// 更新 test_scan_nonexistent_directory (已存在于 line 171-175)
#[test]
fn test_scan_nonexistent_directory() {
    let result = scan_directory(Path::new("/nonexistent/path/12345"));
    assert!(result.is_err());
    // 更新: 使用 .to_string() 获取 anyhow 错误消息
    assert!(result.unwrap_err().to_string().contains("does not exist"));
}

// 更新 test_scan_file_not_directory (已存在于 line 178-186)
#[test]
fn test_scan_file_not_directory() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("notadir.txt");
    File::create(&file_path).unwrap();

    let result = scan_directory(&file_path);
    assert!(result.is_err());
    // 更新: 使用 .to_string() 获取 anyhow 错误消息
    assert!(result.unwrap_err().to_string().contains("not a directory"));
}
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test scanner`
Expected: 所有 scanner 测试通过

- [ ] **Step 6: 提交**

```bash
git add src/scanner.rs
git commit -m "refactor(scanner): migrate to anyhow error handling

- Replace Result<T, String> with anyhow::Result<T>
- Add context to error chains
- Update tests for new error format"
```

---

### Task 3: 重构 merger.rs 错误处理

**Files:**
- Modify: `src/merger.rs:1-345`

- [ ] **Step 1: 添加 anyhow 导入**

```rust
// src/merger.rs 顶部
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::scanner::{FilePair, ScanResult};
use anyhow::{Result, Context};
use colored::Colorize;
use rayon::prelude::*;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;
```

- [ ] **Step 2: 修改 ChildExt trait 签名**

```rust
impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e).context("Failed to check process status"),
            }
        }
    }
}
```

- [ ] **Step 3: 修改 merge_pair 函数返回类型**

```rust
/// Merge a single file pair
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
) -> MergeResult {
    // ... 保持内部逻辑不变，只是内部调用适配 ...
}
```

- [ ] **Step 4: 修改 run_with_timeout 函数**

```rust
/// Run a command with timeout
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = cmd.spawn()
        .context("Failed to spawn ffmpeg process")?;

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("ffmpeg process timed out after 5 minutes");
        }
        Err(e) => Err(e).context("Failed to wait for ffmpeg process"),
    }
}
```

- [ ] **Step 5: 修改 delete_source_files 函数**

```rust
/// Delete source files after successful merge
fn delete_source_files(pair: &FilePair) -> Result<()> {
    let video_result = std::fs::remove_file(&pair.video);
    let audio_result = std::fs::remove_file(&pair.audio);

    match (video_result, audio_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(anyhow::anyhow!(
            "Failed to delete video '{}': {}", pair.video.display(), e
        )),
        (Ok(()), Err(e)) => Err(anyhow::anyhow!(
            "Failed to delete audio '{}': {}", pair.audio.display(), e
        )),
        (Err(ve), Err(ae)) => Err(anyhow::anyhow!(
            "Failed to delete both files: video '{}' ({}), audio '{}' ({})",
            pair.video.display(), ve, pair.audio.display(), ae
        )),
    }
}
```

- [ ] **Step 6: 运行 cargo check 验证**

Run: `cargo check`
Expected: 编译成功

- [ ] **Step 7: 运行测试验证**

Run: `cargo test merger`
Expected: 所有 merger 测试通过

- [ ] **Step 8: 提交**

```bash
git add src/merger.rs
git commit -m "refactor(merger): migrate to anyhow error handling

- Replace String errors with anyhow::Result
- Add context to error chains
- Update ChildExt trait signature"
```

---

### Task 4: 重构 cli.rs 错误处理

**Files:**
- Modify: `src/cli.rs:1-195`

- [ ] **Step 1: 添加 anyhow 导入**

```rust
// src/cli.rs 顶部
use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;
```

- [ ] **Step 2: 修改 validate 方法签名**

```rust
impl Args {
    /// Parse and validate the format string into OutputFormat
    pub fn parsed_format(&self) -> Result<OutputFormat> {
        OutputFormat::parse(&self.format)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Validate and normalize arguments
    pub fn validate(&mut self) -> Result<()> {
        // Clamp jobs to valid range
        if self.jobs < 1 {
            eprintln!("Warning: jobs must be >= 1, clamping to 1");
            self.jobs = 1;
        } else if self.jobs > 32 {
            eprintln!("Warning: jobs must be <= 32, clamping to 32");
            self.jobs = 32;
        }
        Ok(())
    }
}
```

- [ ] **Step 3: 修改 OutputFormat::parse 返回类型**

```rust
impl OutputFormat {
    /// Parse format string, returns error if invalid
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "mkv" => Ok(Self::Mkv),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            _ => bail!("Invalid format '{}'. Supported: mkv, mp4, mov", s),
        }
    }
    // ... 其他方法不变 ...
}
```

- [ ] **Step 4: 更新现有 cli 测试以适配新错误类型**

**注意**: 以下测试已存在于 `src/cli.rs`，只需更新错误断言：

```rust
// 更新 test_parse_invalid_format (已存在于 line 63-67)
#[test]
fn test_parse_invalid_format() {
    let result = OutputFormat::parse("avi");
    assert!(result.is_err());
    // 更新: 使用 .to_string() 获取 anyhow 错误消息
    assert!(result.unwrap_err().to_string().contains("Invalid format"));
}

// 更新 test_parsed_format_invalid (已存在于 line 185-194)
#[test]
fn test_parsed_format_invalid() {
    let args = Args {
        source: PathBuf::from("."),
        output: PathBuf::from("."),
        sdel: true,
        format: "invalid".to_string(),
        jobs: 4,
    };
    // 更新: 确保错误消息格式正确
    assert!(args.parsed_format().is_err());
}
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test cli`
Expected: 所有 cli 测试通过

- [ ] **Step 6: 提交**

```bash
git add src/cli.rs
git commit -m "refactor(cli): migrate to anyhow error handling

- Replace String errors with anyhow::Result
- Update validate and parse methods"
```

---

### Task 5: 重构 main.rs 并添加退出码

**Files:**
- Modify: `src/main.rs:1-86`

- [ ] **Step 1: 添加 anyhow 导入和退出码常量**

```rust
// src/main.rs
mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use colored::Colorize;

/// Exit codes
mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const FFMPEG_NOT_FOUND: i32 = 2;
    pub const MERGE_FAILED: i32 = 3;
}
```

- [ ] **Step 2: 重构 main 函数**

```rust
fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red(), e);

        // Determine exit code based on error type
        let exit_code = determine_exit_code(&e);
        std::process::exit(exit_code);
    }
}

fn run() -> Result<()> {
    // Parse and validate arguments
    let mut args = Args::parse();
    args.validate()?;

    // Parse format early
    let format = args.parsed_format()?;

    // Phase 1: Check ffmpeg
    if !ffmpeg::ensure_ffmpeg() {
        std::process::exit(exit_codes::FFMPEG_NOT_FOUND);
    }

    // Phase 2: Scan directory
    let scan_result = scanner::scan_directory(&args.source)?;

    // Check if we have anything to do
    if scan_result.pairs.is_empty() {
        println!("{}", "No file pairs to merge".yellow());
        return Ok(());
    }

    // Validate/create output directory
    if !args.output.exists() {
        std::fs::create_dir_all(&args.output)
            .context("Failed to create output directory")?;
    }

    // Check output directory is writable
    check_output_writable(&args.output)?;

    // Phase 3: Execute merges
    println!("Processing {} file pairs...", scan_result.pairs.len());
    let summary = merger::execute_merges(
        scan_result,
        &args.output,
        format,
        args.jobs,
        args.sdel,
    );

    // Phase 4: Print report
    summary.print_report();

    // Exit with appropriate code
    if summary.all_success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Some merges failed"))
    }
}

fn check_output_writable(output: &std::path::Path) -> Result<()> {
    if output.exists() {
        let test_file = output.join(".mixbilibili_write_test");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&test_file)
        {
            Ok(_) => { let _ = std::fs::remove_file(&test_file); }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = std::fs::remove_file(&test_file);
            }
            Err(_) => {
                anyhow::bail!("Output directory is not writable: {}", output.display());
            }
        }
    }
    Ok(())
}

fn determine_exit_code(error: &anyhow::Error) -> i32 {
    let err_str = error.to_string();
    if err_str.contains("ffmpeg") || err_str.contains("FFmpeg") {
        exit_codes::FFMPEG_NOT_FOUND
    } else if err_str.contains("merge") || err_str.contains("failed") {
        exit_codes::MERGE_FAILED
    } else {
        exit_codes::GENERAL_ERROR
    }
}
```

- [ ] **Step 3: 运行 cargo check 验证**

Run: `cargo check`
Expected: 编译成功

- [ ] **Step 4: 运行全部测试**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 5: 提交**

```bash
git add src/main.rs
git commit -m "refactor(main): migrate to anyhow and add exit codes

- Add structured exit codes (0=success, 1=error, 2=ffmpeg, 3=merge)
- Separate run() from main() for better error handling
- Improve output directory writable check"
```

---

## Chunk 2: P1.2 核心功能测试

### Task 6: 添加 merger.rs 测试

**Files:**
- Modify: `src/merger.rs` (测试模块)

- [ ] **Step 1: 添加 delete_source_files 测试**

```rust
#[cfg(test)]
mod delete_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_delete_source_files_success() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_video_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Only create audio, video is missing
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to delete video"));
    }

    #[test]
    fn test_delete_source_files_both_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Neither file exists
        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to delete both"));
    }
}
```

- [ ] **Step 2: 添加 wait_timeout 测试 (跨平台)**

**注意**: 使用条件编译处理跨平台差异。Windows 使用 `timeout`，Unix 使用 `sleep`。

```rust
#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[test]
    fn test_wait_timeout_normal_completion() {
        // Use cross-platform sleep command
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.1")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let result = child.wait_timeout(Duration::from_secs(5));
        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.is_some());
    }

    #[test]
    fn test_wait_timeout_exceeded() {
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("10")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let result = child.wait_timeout(Duration::from_millis(50));
        assert!(result.is_ok());
        // Should return None indicating timeout
        let status = result.unwrap();
        assert!(status.is_none());
    }

    #[test]
    fn test_wait_timeout_already_finished() {
        // Very short sleep that finishes almost immediately
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.01")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        // Wait a bit for it to finish
        std::thread::sleep(Duration::from_millis(150));

        let result = child.wait_timeout(Duration::from_secs(5));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test merger`
Expected: 所有 merger 测试通过

- [ ] **Step 4: 提交**

```bash
git add src/merger.rs
git commit -m "test(merger): add tests for delete_source_files and wait_timeout

- Test successful deletion, missing files, both missing
- Test timeout behavior using sleep command
- Test normal completion and already finished processes"
```

---

### Task 7: 添加 main.rs 集成测试

**Files:**
- Modify: `src/main.rs` (添加测试模块)

- [ ] **Step 1: 添加集成测试模块**

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;

    fn create_test_video(path: &std::path::Path) {
        // Create a minimal valid MP4 header
        let mut file = File::create(path).unwrap();
        // Minimal MP4 header (ftyp box)
        file.write_all(&[
            0x00, 0x00, 0x00, 0x14,  // size (20 bytes)
            b'f', b't', b'y', b'p',  // type 'ftyp'
            b'i', b's', b'o', b'm',  // major brand
            0x00, 0x00, 0x00, 0x00,  // minor version
            b'i', b's', b'o', b'm',  // compatible brand
        ]).unwrap();
    }

    fn create_test_audio(path: &std::path::Path) {
        // Create a minimal M4A header
        let mut file = File::create(path).unwrap();
        // Minimal M4A header (ftyp box)
        file.write_all(&[
            0x00, 0x00, 0x00, 0x18,  // size (24 bytes)
            b'f', b't', b'y', b'p',  // type 'ftyp'
            b'M', b'4', b'A', b' ',  // major brand
            0x00, 0x00, 0x00, 0x00,  // minor version
            b'i', b's', b'o', b'm',  // compatible brand
            b'M', b'4', b'A', b' ',  // compatible brand
        ]).unwrap();
    }

    #[test]
    fn test_check_output_writable_success() {
        let dir = tempdir().unwrap();
        let result = check_output_writable(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_output_writable_nonexistent_creates_test_file() {
        let dir = tempdir().unwrap();
        // Directory exists, should be writable
        let result = check_output_writable(dir.path());
        assert!(result.is_ok());
        // Test file should be cleaned up
        assert!(!dir.path().join(".mixbilibili_write_test").exists());
    }

    #[test]
    fn test_exit_codes_constants() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::FFMPEG_NOT_FOUND, 2);
        assert_eq!(exit_codes::MERGE_FAILED, 3);
    }

    #[test]
    fn test_determine_exit_code_general() {
        let err = anyhow::anyhow!("Some random error");
        assert_eq!(determine_exit_code(&err), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn test_determine_exit_code_ffmpeg() {
        let err = anyhow::anyhow!("ffmpeg not found in PATH");
        assert_eq!(determine_exit_code(&err), exit_codes::FFMPEG_NOT_FOUND);
    }

    #[test]
    fn test_determine_exit_code_merge() {
        let err = anyhow::anyhow!("Some merges failed to complete");
        assert_eq!(determine_exit_code(&err), exit_codes::MERGE_FAILED);
    }
}
```

- [ ] **Step 2: 运行测试验证**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 3: 提交**

```bash
git add src/main.rs
git commit -m "test(main): add integration tests

- Test check_output_writable function
- Test exit code determination logic
- Test exit code constants"
```

---

## Chunk 3: P1.3 GitHub Actions CI/CD

### Task 8: 创建 CI/CD 配置

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: 创建目录结构**

Run: `mkdir -p .github/workflows`
Expected: 目录创建成功

- [ ] **Step 2: 创建 CI 配置文件**

```yaml
name: CI

on:
  push:
    branches: [master]
  pull_request:
    branches: [master]
  release:
    types: [created]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: ~/.cargo/registry
          key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache cargo index
        uses: actions/cache@v4
        with:
          path: ~/.cargo/git
          key: ${{ runner.os }}-cargo-git-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache target directory
        uses: actions/cache@v4
        with:
          path: target
          key: ${{ runner.os }}-target-${{ hashFiles('**/Cargo.lock') }}

      - name: Install ffmpeg
        run: sudo apt-get install -y ffmpeg

      - name: Check formatting
        run: cargo fmt --check

      - name: Run clippy
        run: cargo clippy -- -D warnings

      - name: Run tests
        run: cargo test

  build:
    needs: test
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Install ffmpeg (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get install -y ffmpeg

      - name: Install ffmpeg (macOS)
        if: runner.os == 'macOS'
        run: brew install ffmpeg

      - name: Install ffmpeg (Windows)
        if: runner.os == 'Windows'
        run: choco install ffmpeg -y

      - name: Build release
        run: cargo build --release

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: binary-${{ matrix.os }}
          path: |
            target/release/mixbilibili
            target/release/mixbilibili.exe
          if-no-files-found: ignore

  release:
    needs: build
    if: github.event_name == 'release'
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4

      - name: Upload Release Assets
        uses: softprops/action-gh-release@v1
        with:
          files: binary-*/mixbilibili*
```

- [ ] **Step 3: 提交**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add GitHub Actions workflow

- Test job: format check, clippy, tests with ffmpeg
- Build job: multi-platform (Linux, macOS, Windows)
- Release job: upload binaries to GitHub Releases
- Add cargo caching for faster builds"
```

---

## Chunk 4: P2.1 性能优化

### Task 9: 移除 pairs 克隆

**Files:**
- Modify: `src/merger.rs:169-222`

- [ ] **Step 1: 重构 execute_merges 函数**

```rust
/// Execute parallel merges with controlled concurrency
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
) -> MergeSummary {
    let output_dir = output_dir.to_path_buf();

    // Use reference instead of clone
    let pairs = &scan_result.pairs;

    // Configure thread pool
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .unwrap();

    // Execute merges in parallel with indices
    let results: Vec<MergeResult> = pool.install(|| {
        pairs
            .par_iter()
            .enumerate()
            .map(|(idx, pair)| merge_pair(pair, idx, &output_dir, format))
            .collect()
    });

    // Build summary
    let mut summary = MergeSummary::new();
    summary.skipped_count = scan_result.stats.skipped;
    summary.orphaned_count = scan_result.stats.orphaned;

    for result in results {
        if result.success {
            summary.success_count += 1;

            // Delete source files if requested
            if delete_source {
                let pair = &pairs[result.pair_index];
                if let Err(e) = delete_source_files(pair) {
                    eprintln!("Warning: {}", e);
                    summary.deletion_failures += 1;
                }
            }
        } else {
            summary.failed_count += 1;
            if let Some(error) = result.error {
                summary.failures.push((result.pair_name, error));
            }
        }
    }

    summary
}
```

- [ ] **Step 2: 运行测试验证**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 3: 提交**

```bash
git add src/merger.rs
git commit -m "perf(merger): remove unnecessary pairs clone

- Use reference to scan_result.pairs instead of cloning
- Reduces memory allocation for large file sets"
```

---

### Task 10: 添加常量替代魔法数字

**Files:**
- Modify: `src/merger.rs:1-36`

- [ ] **Step 1: 重构现有常量**

**注意**: `FFMPEG_TIMEOUT` 常量已存在于 `src/merger.rs:12`。重构为使用新的命名常量。

```rust
// src/merger.rs 顶部 - 修改现有常量定义
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::scanner::{FilePair, ScanResult};
use anyhow::{Result, Context};
use colored::Colorize;
use rayon::prelude::*;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;

/// Default timeout for ffmpeg process (5 minutes)
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Polling interval for checking process status
const POLL_INTERVAL_MILLIS: u64 = 100;

/// Default timeout for ffmpeg process (修改现有常量)
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(DEFAULT_TIMEOUT_SECS);
```

- [ ] **Step 2: 更新 wait_timeout 使用常量**

```rust
impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(POLL_INTERVAL_MILLIS);

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(e).context("Failed to check process status"),
            }
        }
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add src/merger.rs
git commit -m "refactor(merger): replace magic numbers with constants

- Add DEFAULT_TIMEOUT_SECS (300) and POLL_INTERVAL_MILLIS (100)
- Improve code readability and maintainability"
```

---

## Chunk 5: P2.2-P2.3 代码质量和输入验证

### Task 11: 清理 ffmpeg.rs 未使用代码标注

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: 审查并移除不必要的 #[allow(dead_code)]**

```rust
// src/ffmpeg.rs
// Os enum is used by detect_os(), keep it but remove allow(dead_code)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOS,
    Linux,
    Unknown,
}

// InstallResult is used by run_install(), keep it but remove allow(dead_code)
#[derive(Debug)]
pub struct InstallResult {
    pub success: bool,
    pub output: String,
}

// ffmpeg_path is used in tests, keep it with conditional allow
#[cfg(test)]
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    which::which("ffmpeg").ok()
}

#[cfg(not(test))]
#[allow(dead_code)]
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    which::which("ffmpeg").ok()
}
```

- [ ] **Step 2: 运行 clippy 检查**

Run: `cargo clippy -- -D warnings`
Expected: 无警告

- [ ] **Step 3: 运行测试验证**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add src/ffmpeg.rs
git commit -m "refactor(ffmpeg): clean up dead_code annotations

- Remove unnecessary #[allow(dead_code)] from Os and InstallResult
- Make ffmpeg_path conditional for test usage"
```

---

### Task 12: 增强输入验证

**Files:**
- Modify: `src/cli.rs:109-127`

- [ ] **Step 1: 增强 validate 方法**

```rust
impl Args {
    /// Parse and validate the format string into OutputFormat
    pub fn parsed_format(&self) -> Result<OutputFormat> {
        OutputFormat::parse(&self.format)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Validate and normalize arguments
    pub fn validate(&mut self) -> Result<()> {
        // Clamp jobs to valid range
        if self.jobs < 1 {
            eprintln!("Warning: jobs must be >= 1, clamping to 1");
            self.jobs = 1;
        } else if self.jobs > 32 {
            eprintln!("Warning: jobs must be <= 32, clamping to 32");
            self.jobs = 32;
        }

        // Validate source directory
        if !self.source.exists() {
            bail!("Source directory does not exist: {}", self.source.display());
        }
        if !self.source.is_dir() {
            bail!("Source path is not a directory: {}", self.source.display());
        }

        // Validate output directory (if different from source)
        if self.output != self.source {
            // Output will be created if it doesn't exist
            if self.output.exists() && !self.output.is_dir() {
                bail!("Output path exists but is not a directory: {}", self.output.display());
            }
        }

        Ok(())
    }
}
```

- [ ] **Step 2: 添加验证测试**

```rust
#[cfg(test)]
mod validation_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_validate_source_not_exists() {
        let mut args = Args {
            source: PathBuf::from("/nonexistent/path/12345"),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_validate_source_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::File::create(&file).unwrap();

        let mut args = Args {
            source: file,
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_validate_output_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("output.txt");
        fs::File::create(&file).unwrap();

        let mut args = Args {
            source: dir.path().to_path_buf(),
            output: file,
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_validate_success() {
        let dir = tempdir().unwrap();

        let mut args = Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_ok());
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test cli`
Expected: 所有 cli 测试通过

- [ ] **Step 4: 提交**

```bash
git add src/cli.rs
git commit -m "feat(cli): enhance input validation

- Validate source directory exists and is a directory
- Validate output path is not a file if it exists
- Add comprehensive validation tests"
```

---

## Chunk 6: P3.1 依赖管理

### Task 13: 精确版本锁定和移除 num_cpus

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/cli.rs`

- [ ] **Step 1: 更新 Cargo.toml**

**注意**: `[profile.release]` 部分已存在，只需修改依赖部分。

```toml
[package]
name = "mixbilibili"
version = "0.1.0"
edition = "2021"
description = "A CLI tool for batch merging Bilibili video and audio files"
license = "MIT"

[dependencies]
clap = { version = "4.5.0", features = ["derive"] }
which = "6.0.0"
rayon = "1.10.0"
colored = "2.1.0"
anyhow = "1.0.82"

[dev-dependencies]
tempfile = "3.10.0"

# [profile.release] 部分已存在，无需修改
```

- [ ] **Step 2: 更新 cli.rs 使用标准库替代 num_cpus**

```rust
// src/cli.rs Args 结构体
/// A CLI tool for batch merging Bilibili video and audio files
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Source directory containing mp4/m4a files
    #[arg(short, long, default_value = ".")]
    pub source: PathBuf,

    /// Output directory for merged files (auto-created)
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    /// Delete source files after successful merge
    #[arg(short = 'd', long, default_value_t = true)]
    pub sdel: bool,

    /// Output format: mkv, mp4, mov
    #[arg(short, long, default_value = "mkv", value_name = "FORMAT")]
    pub format: String,

    /// Number of parallel ffmpeg processes
    #[arg(short = 'j', long, default_value_t = default_jobs())]
    pub jobs: usize,
}

/// Get default number of jobs based on available parallelism
fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}
```

- [ ] **Step 3: 运行 cargo check 验证**

Run: `cargo check`
Expected: 编译成功

- [ ] **Step 4: 运行测试验证**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml Cargo.lock src/cli.rs
git commit -m "chore: pin dependency versions and remove num_cpus

- Pin all dependencies to specific versions
- Replace num_cpus with std::thread::available_parallelism
- Remove external dependency, use standard library"
```

---

## Chunk 7: P3.2 文档完善

### Task 14: 创建 CHANGELOG.md

**Files:**
- Create: `CHANGELOG.md`

- [ ] **Step 1: 创建 CHANGELOG.md**

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive error handling with `anyhow` crate
- GitHub Actions CI/CD pipeline for automated testing and releases
- Core functionality tests for merger module
- Structured exit codes (0=success, 1=error, 2=ffmpeg not found, 3=merge failed)

### Changed
- Improved error messages with context information
- Enhanced input validation for source and output directories
- Replaced `num_cpus` dependency with standard library

### Fixed
- TOCTOU race condition in output directory write check

## [0.1.0] - 2024-01-01

### Added
- Initial release
- Batch merge Bilibili video (.mp4) and audio (.m4a) files
- Support for multiple output formats (MKV, MP4, MOV)
- Parallel processing with configurable concurrency
- Automatic ffmpeg detection and installation prompt
- Skip files being downloaded (aria2 detection)
- Optional source file deletion after merge
```

- [ ] **Step 2: 提交**

```bash
git add CHANGELOG.md
git commit -m "docs: add CHANGELOG.md

- Document unreleased changes
- Add initial release notes"
```

---

### Task 15: 创建 CONTRIBUTING.md

**Files:**
- Create: `CONTRIBUTING.md`

- [ ] **Step 1: 创建 CONTRIBUTING.md**

```markdown
# Contributing to mixbilibili

Thank you for your interest in contributing!

## Development Setup

1. Install Rust via [rustup](https://rustup.rs/)
2. Clone the repository
3. Run `cargo test` to verify your setup

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Add tests for new functionality

## Pull Requests

1. Create a feature branch from `master`
2. Make your changes with clear commit messages
3. Ensure CI passes (format, clippy, tests)
4. Open a pull request with a description of changes

## Testing

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test scanner
cargo test merger
cargo test cli

# Run clippy
cargo clippy -- -D warnings

# Check formatting
cargo fmt --check
```

## Release Process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create a git tag: `git tag v0.x.x`
4. Push tag: `git push --tags`
5. GitHub Actions will build and publish releases
```

- [ ] **Step 2: 提交**

```bash
git add CONTRIBUTING.md
git commit -m "docs: add CONTRIBUTING.md

- Add development setup instructions
- Document code style requirements
- Add PR and testing guidelines"
```

---

### Task 16: 添加 Rustdoc 注释

**Files:**
- Modify: `src/scanner.rs`
- Modify: `src/merger.rs`
- Modify: `src/ffmpeg.rs`
- Modify: `src/cli.rs`

- [ ] **Step 1: 添加 scanner.rs 文档注释**

```rust
/// A pair of video and audio files to merge.
///
/// Represents matched video (.mp4) and audio (.m4a) files
/// with the same filename stem.
#[derive(Debug, Clone)]
pub struct FilePair {
    /// The video file path (.mp4)
    pub video: PathBuf,
    /// The audio file path (.m4a)
    pub audio: PathBuf,
    /// The filename stem (filename without extension)
    pub stem: String,
}

/// Statistics from directory scanning.
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Number of valid file pairs found
    pub pairs: usize,
    /// Number of pairs skipped due to aria2 control files
    pub skipped: usize,
    /// Number of orphaned files (mp4 or m4a without matching pair)
    pub orphaned: usize,
}

/// Result of scanning a directory for media file pairs.
#[derive(Debug)]
pub struct ScanResult {
    /// Valid file pairs ready for merging
    pub pairs: Vec<FilePair>,
    /// Scanning statistics
    pub stats: ScanStats,
    /// Names of skipped pairs (aria2 downloads in progress)
    pub skipped_names: Vec<String>,
}

/// Scan a directory for matching mp4/m4a file pairs.
///
/// # Arguments
///
/// * `source_dir` - The directory to scan for media files
///
/// # Returns
///
/// A `ScanResult` containing matched pairs and statistics.
///
/// # Errors
///
/// Returns an error if:
/// - The source directory does not exist
/// - The source path is not a directory
/// - The directory cannot be read
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use mixbilibili::scanner::scan_directory;
///
/// let result = scan_directory(Path::new("./downloads"))?;
/// println!("Found {} pairs", result.pairs.len());
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn scan_directory(source_dir: &Path) -> Result<ScanResult> {
    // ... implementation ...
}
```

- [ ] **Step 2: 添加 merger.rs 文档注释**

```rust
/// Result of a single merge operation.
#[derive(Debug)]
pub struct MergeResult {
    /// Index of the pair in the original pairs vector
    pub pair_index: usize,
    /// The stem name of the processed pair
    pub pair_name: String,
    /// Whether the merge succeeded
    pub success: bool,
    /// Error message if the merge failed
    pub error: Option<String>,
}

/// Summary of all merge operations.
#[derive(Debug, Default)]
pub struct MergeSummary {
    /// Number of successful merges
    pub success_count: usize,
    /// Number of failed merges
    pub failed_count: usize,
    /// Number of skipped pairs (aria2 files present)
    pub skipped_count: usize,
    /// Number of orphaned files
    pub orphaned_count: usize,
    /// List of failed merges with error messages
    pub failures: Vec<(String, String)>,
    /// Number of source file deletion failures
    pub deletion_failures: usize,
}

/// Merge a single file pair using ffmpeg.
///
/// # Arguments
///
/// * `pair` - The file pair to merge
/// * `pair_index` - Index for tracking in results
/// * `output_dir` - Directory for output file
/// * `format` - Output format (MKV, MP4, MOV)
///
/// # Returns
///
/// A `MergeResult` indicating success or failure.
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
) -> MergeResult {
    // ... implementation ...
}

/// Execute parallel merges with controlled concurrency.
///
/// # Arguments
///
/// * `scan_result` - Result from directory scanning
/// * `output_dir` - Directory for output files
/// * `format` - Output format
/// * `jobs` - Number of parallel processes
/// * `delete_source` - Whether to delete source files after success
///
/// # Returns
///
/// A `MergeSummary` with results of all operations.
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
) -> MergeSummary {
    // ... implementation ...
}
```

- [ ] **Step 3: 添加 ffmpeg.rs 文档注释**

```rust
/// Check if ffmpeg is available in PATH.
///
/// # Returns
///
/// `true` if ffmpeg can be found, `false` otherwise.
pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Ensure ffmpeg is available, prompting for installation if needed.
///
/// If ffmpeg is not found, prompts the user to install it using
/// the system's package manager (winget, brew, apt, etc.).
///
/// # Returns
///
/// `true` if ffmpeg is available (was already or installed successfully).
pub fn ensure_ffmpeg() -> bool {
    // ... implementation ...
}

/// Build an ffmpeg command for merging video and audio files.
///
/// # Arguments
///
/// * `video_path` - Path to the video file
/// * `audio_path` - Path to the audio file
/// * `output_path` - Path for the output file
/// * `format` - Output format (determines codec flags)
///
/// # Returns
///
/// A configured `Command` ready to execute.
pub fn build_merge_command(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    format: OutputFormat,
) -> Command {
    // ... implementation ...
}
```

- [ ] **Step 4: 添加 cli.rs 文档注释**

```rust
/// Supported output formats for merged files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Matroska Video container (.mkv)
    Mkv,
    /// MPEG-4 Part 14 container (.mp4)
    Mp4,
    /// QuickTime File Format (.mov)
    Mov,
}

/// Command-line arguments for the mixer.
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Source directory containing mp4/m4a files
    #[arg(short, long, default_value = ".")]
    pub source: PathBuf,

    /// Output directory for merged files (auto-created)
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    /// Delete source files after successful merge
    #[arg(short = 'd', long, default_value_t = true)]
    pub sdel: bool,

    /// Output format: mkv, mp4, mov
    #[arg(short, long, default_value = "mkv", value_name = "FORMAT")]
    pub format: String,

    /// Number of parallel ffmpeg processes
    #[arg(short = 'j', long, default_value_t = default_jobs())]
    pub jobs: usize,
}
```

- [ ] **Step 5: 运行 cargo doc 验证**

Run: `cargo doc --no-deps`
Expected: 文档生成成功，无警告

- [ ] **Step 6: 提交**

```bash
git add src/scanner.rs src/merger.rs src/ffmpeg.rs src/cli.rs
git commit -m "docs: add rustdoc comments to public APIs

- Document all public structs and functions
- Add examples and error descriptions
- Document CLI arguments"
```

---

## Chunk 8: P3.3 安全细节修复

### Task 17: 修复 TOCTOU 竞态条件

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 确认 check_output_writable 已修复**

检查在 Task 5 中添加的 `check_output_writable` 函数已经使用了 `OpenOptions::create_new(true)` 原子操作。如果不是，更新为：

```rust
fn check_output_writable(output: &std::path::Path) -> Result<()> {
    if output.exists() {
        let test_file = output.join(".mixbilibili_write_test");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)  // Atomic create - fails if exists or no permission
            .open(&test_file)
        {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // File already exists, which means directory is writable
                let _ = std::fs::remove_file(&test_file);
            }
            Err(_) => {
                anyhow::bail!("Output directory is not writable: {}", output.display());
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 2: 确认已有测试覆盖**

确认在 Task 7 中已添加 `test_check_output_writable_success` 测试。

- [ ] **Step 3: 运行测试验证**

Run: `cargo test check_output_writable`
Expected: 测试通过

- [ ] **Step 4: 如果有更改，提交**

```bash
git add src/main.rs
git commit -m "fix(security): use atomic file creation for write check

- Use OpenOptions::create_new(true) to prevent TOCTOU race
- Handle AlreadyExists case correctly"
```

---

## 完成检查

### Task 18: 最终验证

- [ ] **Step 1: 运行完整测试套件**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 2: 运行 clippy 检查**

Run: `cargo clippy -- -D warnings`
Expected: 无警告

- [ ] **Step 3: 检查格式**

Run: `cargo fmt --check`
Expected: 格式正确

- [ ] **Step 4: 构建发布版本**

Run: `cargo build --release`
Expected: 构建成功

- [ ] **Step 5: 最终提交**

```bash
git add -A
git commit -m "chore: complete comprehensive optimization

P1 (High Priority):
- Error handling with anyhow
- Core functionality tests
- GitHub Actions CI/CD

P2 (Medium Priority):
- Performance optimizations
- Code quality improvements
- Enhanced input validation

P3 (Low Priority):
- Pinned dependency versions
- Removed num_cpus dependency
- Added CHANGELOG.md and CONTRIBUTING.md
- Added rustdoc comments
- Fixed TOCTOU security issue"
```

---

## 成功标准

- [ ] 所有现有功能正常工作
- [ ] 测试覆盖核心路径
- [ ] CI 在 PR 时自动运行
- [ ] 错误信息清晰有上下文
- [ ] 无 clippy 警告
- [ ] 文档完整 (CHANGELOG, CONTRIBUTING, rustdoc)