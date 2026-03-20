# mixbilibili CLI Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a cross-platform CLI tool for batch merging Bilibili video (.mp4) and audio (.m4a) files using ffmpeg.

**Architecture:** 4-module Rust binary with clap CLI, parallel processing via rayon, and graceful error handling. Each module has a single responsibility: CLI parsing, ffmpeg management, file scanning/pairing, and merge execution.

**Tech Stack:** Rust 1.70+, clap 4 (derive), which 6, rayon 1, num_cpus 1, colored 2

---

## Chunk 1: Project Setup & CLI Module

### Task 1: Initialize Rust Binary Project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `.gitignore`

- [ ] **Step 1: Initialize cargo project**

Run: `cargo init --name mixbilibili`
Expected: Creates Cargo.toml and src/main.rs

- [ ] **Step 2: Initialize git repository**

Run: `git init`
Expected: Initialized empty Git repository

- [ ] **Step 3: Create .gitignore**

```gitignore
/target
/Cargo.lock
**/*.rs.bk
*.pdb
.DS_Store
```

- [ ] **Step 4: Update Cargo.toml with dependencies**

```toml
[package]
name = "mixbilibili"
version = "0.1.0"
edition = "2021"
description = "A CLI tool for batch merging Bilibili video and audio files"
license = "MIT"

[dependencies]
clap = { version = "4", features = ["derive"] }
which = "6"
rayon = "1"
num_cpus = "1"
colored = "2"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 5: Verify project compiles**

Run: `cargo check`
Expected: Compiles successfully with no errors

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs .gitignore
git commit -m "chore: initialize Rust binary project with dependencies"
```

---

### Task 2: Write Tests for OutputFormat

**Files:**
- Create: `src/cli.rs`

- [ ] **Step 1: Write failing tests for OutputFormat**

```rust
// src/cli.rs
use clap::ValueEnum;

/// Supported output formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Mkv,
    Mp4,
    Mov,
}

impl OutputFormat {
    /// Parse format string, returns error message if invalid
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "mkv" => Ok(Self::Mkv),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            _ => Err(format!("Invalid format '{}'. Supported: mkv, mp4, mov", s)),
        }
    }

    /// Get file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
        }
    }

    /// Returns true if format requires -movflags +faststart
    pub fn needs_faststart(&self) -> bool {
        matches!(self, Self::Mp4 | Self::Mov)
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_formats_lowercase() {
        assert_eq!(OutputFormat::parse("mkv").unwrap(), OutputFormat::Mkv);
        assert_eq!(OutputFormat::parse("mp4").unwrap(), OutputFormat::Mp4);
        assert_eq!(OutputFormat::parse("mov").unwrap(), OutputFormat::Mov);
    }

    #[test]
    fn test_parse_valid_formats_case_insensitive() {
        assert_eq!(OutputFormat::parse("MKV").unwrap(), OutputFormat::Mkv);
        assert_eq!(OutputFormat::parse("Mp4").unwrap(), OutputFormat::Mp4);
        assert_eq!(OutputFormat::parse("MOV").unwrap(), OutputFormat::Mov);
    }

    #[test]
    fn test_parse_invalid_format() {
        let result = OutputFormat::parse("avi");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid format 'avi'. Supported: mkv, mp4, mov");
    }

    #[test]
    fn test_extension() {
        assert_eq!(OutputFormat::Mkv.extension(), "mkv");
        assert_eq!(OutputFormat::Mp4.extension(), "mp4");
        assert_eq!(OutputFormat::Mov.extension(), "mov");
    }

    #[test]
    fn test_needs_faststart() {
        assert!(!OutputFormat::Mkv.needs_faststart());
        assert!(OutputFormat::Mp4.needs_faststart());
        assert!(OutputFormat::Mov.needs_faststart());
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test cli::tests`
Expected: All 5 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/cli.rs
git commit -m "feat: add OutputFormat enum with tests"
```

---

### Task 3: Write Tests and Implement Args Struct

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add Args struct with tests to cli.rs**

```rust
// Add to src/cli.rs after OutputFormat implementation
use clap::Parser;
use std::path::PathBuf;

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
    #[arg(short, long, default_value_t = true)]
    pub sdel: bool,

    /// Output format: mkv, mp4, mov
    #[arg(short, long, default_value = "mkv", value_name = "FORMAT")]
    pub format: String,

    /// Number of parallel ffmpeg processes
    #[arg(short = 'j', long, default_value_t = num_cpus::get())]
    pub jobs: usize,
}

impl Args {
    /// Parse and validate the format string into OutputFormat
    pub fn parsed_format(&self) -> Result<OutputFormat, String> {
        OutputFormat::parse(&self.format)
    }

    /// Validate and normalize arguments
    pub fn validate(&mut self) -> Result<(), String> {
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

#[cfg(test)]
mod args_tests {
    use super::*;

    #[test]
    fn test_validate_jobs_clamp_to_min() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 0,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 1);
    }

    #[test]
    fn test_validate_jobs_clamp_to_max() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 100,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 32);
    }

    #[test]
    fn test_validate_jobs_in_range() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 4);
    }

    #[test]
    fn test_parsed_format_valid() {
        let args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mp4".to_string(),
            jobs: 4,
        };
        assert_eq!(args.parsed_format().unwrap(), OutputFormat::Mp4);
    }

    #[test]
    fn test_parsed_format_invalid() {
        let args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "invalid".to_string(),
            jobs: 4,
        };
        assert!(args.parsed_format().is_err());
    }
}
```

- [ ] **Step 2: Update main.rs to use CLI**

```rust
// src/main.rs
mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use clap::Parser;
use cli::Args;

fn main() {
    let mut args = Args::parse();

    if let Err(e) = args.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Validate format early
    let format = match args.parsed_format() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("Source: {:?}", args.source);
    println!("Output: {:?}", args.output);
    println!("Format: {}", format);
    println!("Jobs: {}", args.jobs);
    println!("Delete source: {}", args.sdel);
}
```

- [ ] **Step 3: Add placeholder modules**

Create empty placeholder files:

```rust
// src/ffmpeg.rs
// TODO: Implement ffmpeg module

#[cfg(test)]
mod tests {}
```

```rust
// src/scanner.rs
// TODO: Implement scanner module

#[cfg(test)]
mod tests {}
```

```rust
// src/merger.rs
// TODO: Implement merger module

#[cfg(test)]
mod tests {}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Test CLI manually**

Run: `cargo run -- --help`
Expected: Shows help text with all arguments

Run: `cargo run -- -f invalid`
Expected: Prints error about invalid format

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs src/ffmpeg.rs src/scanner.rs src/merger.rs
git commit -m "feat: add CLI argument parsing with clap derive and validation"
```

---

## Chunk 2: FFmpeg Module

### Task 4: Write Tests and Implement FFmpeg Check

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Write tests and implementation for ffmpeg check**

```rust
// src/ffmpeg.rs
use std::process::Command;

/// Check if ffmpeg is available in PATH
pub fn is_ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

/// Get ffmpeg path if available
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    which::which("ffmpeg").ok()
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test ffmpeg::tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/ffmpeg.rs
git commit -m "feat: add ffmpeg availability check with tests"
```

---

### Task 5: Write Tests and Implement OS Detection

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Add OS detection with tests**

```rust
// Add to src/ffmpeg.rs

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
```

- [ ] **Step 2: Run tests**

Run: `cargo test ffmpeg::os_tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/ffmpeg.rs
git commit -m "feat: add OS detection for package manager selection"
```

---

### Task 6: Write Tests and Implement Package Manager Commands

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Add package manager functions with tests**

```rust
// Add to src/ffmpeg.rs

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
```

- [ ] **Step 2: Run tests**

Run: `cargo test ffmpeg::pm_tests`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add src/ffmpeg.rs
git commit -m "feat: add package manager commands and manual instructions with tests"
```

---

### Task 7: Write Tests and Implement Installation Prompt

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Add installation prompt functions with tests**

```rust
// Add to src/ffmpeg.rs
use std::io::{self, BufRead, Write};

/// Prompt user for ffmpeg installation
/// Returns true if user agreed and installation succeeded
pub fn prompt_and_install(os: Os) -> bool {
    if let Some((pm_name, _)) = get_install_command(os) {
        print!("ffmpeg not found. Install via {}? [y/N]: ", pm_name);
        io::stdout().flush().ok();

        let mut input = String::new();
        if io::stdin().lock().read_line(&mut input).is_ok() {
            let input = input.trim().to_lowercase();
            if input == "y" || input == "yes" {
                return run_install(os);
            }
        }
    }

    // Print manual instructions and exit
    println!("{}", get_manual_instructions(os));
    false
}

/// Run the installation command
fn run_install(os: Os) -> bool {
    if let Some((_, cmd)) = get_install_command(os) {
        println!("Running: {}", cmd);

        let result = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", &cmd]).status()
        } else {
            Command::new("sh").args(["-c", &cmd]).status()
        };

        match result {
            Ok(status) if status.success() => {
                // Verify installation
                if is_ffmpeg_available() {
                    println!("ffmpeg installed successfully!");
                    return true;
                } else {
                    println!("Installation completed but ffmpeg not found in PATH.");
                    println!("You may need to restart your terminal.");
                }
            }
            Ok(status) => {
                println!("Installation failed with exit code: {:?}", status.code());
            }
            Err(e) => {
                println!("Failed to run installation: {}", e);
            }
        }
    }

    println!("{}", get_manual_instructions(os));
    false
}

/// Ensure ffmpeg is available, prompting for installation if needed
/// Returns true if ffmpeg is available (was already or installed successfully)
pub fn ensure_ffmpeg() -> bool {
    if is_ffmpeg_available() {
        return true;
    }

    let os = detect_os();
    prompt_and_install(os)
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn test_ensure_ffmpeg_returns_true_if_available() {
        // If ffmpeg is installed, ensure_ffmpeg should return true immediately
        // We can't easily test the prompt flow without mocking stdin
        if is_ffmpeg_available() {
            assert!(ensure_ffmpeg());
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test ffmpeg::install_tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/ffmpeg.rs
git commit -m "feat: add interactive ffmpeg installation with user prompt"
```

---

### Task 8: Write Tests and Implement Merge Command Builder

**Files:**
- Modify: `src/ffmpeg.rs`

- [ ] **Step 1: Add command builder with tests**

```rust
// Add to src/ffmpeg.rs
use crate::cli::OutputFormat;
use std::path::Path;

/// Build ffmpeg merge command
pub fn build_merge_command(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    format: OutputFormat,
) -> Command {
    let mut cmd = Command::new("ffmpeg");

    cmd.arg("-hide_banner")
        .arg("-loglevel").arg("error")
        .arg("-i").arg(video_path)
        .arg("-i").arg(audio_path)
        .arg("-c:v").arg("copy")
        .arg("-c:a").arg("copy");

    if format.needs_faststart() {
        cmd.arg("-movflags").arg("+faststart");
    }

    cmd.arg("-y").arg(output_path);

    cmd
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test ffmpeg::cmd_tests`
Expected: All 5 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/ffmpeg.rs
git commit -m "feat: add ffmpeg merge command builder with tests"
```

---

## Chunk 3: Scanner Module

### Task 9: Write Tests and Implement File Pair Data Structures

**Files:**
- Modify: `src/scanner.rs`

- [ ] **Step 1: Define data structures with tests**

```rust
// src/scanner.rs
use std::path::PathBuf;

/// A pair of video and audio files to merge
#[derive(Debug, Clone)]
pub struct FilePair {
    /// The video file (.mp4)
    pub video: PathBuf,
    /// The audio file (.m4a)
    pub audio: PathBuf,
    /// The stem (filename without extension)
    pub stem: String,
}

/// Statistics from scanning
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Number of valid pairs found
    pub pairs: usize,
    /// Number of pairs skipped due to aria2 files
    pub skipped: usize,
    /// Number of orphaned files (mp4 or m4a without pair)
    pub orphaned: usize,
}

/// Result of scanning a directory
#[derive(Debug)]
pub struct ScanResult {
    /// Valid file pairs ready for merging
    pub pairs: Vec<FilePair>,
    /// Statistics
    pub stats: ScanStats,
    /// Names of skipped pairs (for aria2)
    pub skipped_names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_pair_debug() {
        let pair = FilePair {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("video.m4a"),
            stem: "video".to_string(),
        };
        let debug_str = format!("{:?}", pair);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_scan_stats_default() {
        let stats = ScanStats::default();
        assert_eq!(stats.pairs, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.orphaned, 0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test scanner::tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/scanner.rs
git commit -m "feat: add FilePair and ScanResult data structures with tests"
```

---

### Task 10: Write Tests and Implement Directory Scanner

**Files:**
- Modify: `src/scanner.rs`

- [ ] **Step 1: Add scan function with comprehensive tests**

```rust
// Add to src/scanner.rs
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Scan a directory for mp4/m4a file pairs
pub fn scan_directory(source_dir: &Path) -> Result<ScanResult, String> {
    if !source_dir.exists() {
        return Err(format!("Source directory does not exist: {}", source_dir.display()));
    }

    if !source_dir.is_dir() {
        return Err(format!("Source path is not a directory: {}", source_dir.display()));
    }

    // Check read permission
    let entries = fs::read_dir(source_dir)
        .map_err(|e| format!("Failed to read directory: {}", e))?;

    // Collect all mp4 and m4a files
    let mut mp4_files: HashMap<String, PathBuf> = HashMap::new();
    let mut m4a_files: HashMap<String, PathBuf> = HashMap::new();
    let mut aria2_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Get filename as string
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        // Check for aria2 control files
        if filename.ends_with(".aria2") {
            let base = filename.strip_suffix(".aria2").unwrap();
            aria2_files.insert(base.to_string());
            continue;
        }

        // Categorize media files
        if filename.ends_with(".mp4") {
            let stem = filename.strip_suffix(".mp4").unwrap();
            mp4_files.insert(stem.to_string(), path);
        } else if filename.ends_with(".m4a") {
            let stem = filename.strip_suffix(".m4a").unwrap();
            m4a_files.insert(stem.to_string(), path);
        }
    }

    // Find pairs and track stats
    let mut pairs = Vec::new();
    let mut stats = ScanStats::default();
    let mut skipped_names = Vec::new();

    let all_stems: std::collections::HashSet<_> = mp4_files.keys()
        .chain(m4a_files.keys())
        .collect();

    for stem in all_stems {
        let has_aria2 = aria2_files.contains(stem.as_str())
            || aria2_files.contains(&format!("{}.mp4", stem))
            || aria2_files.contains(&format!("{}.m4a", stem));

        if has_aria2 {
            stats.skipped += 1;
            skipped_names.push(stem.clone());
            continue;
        }

        match (mp4_files.get(stem), m4a_files.get(stem)) {
            (Some(video), Some(audio)) => {
                pairs.push(FilePair {
                    video: video.clone(),
                    audio: audio.clone(),
                    stem: stem.clone(),
                });
                stats.pairs += 1;
            }
            _ => {
                stats.orphaned += 1;
            }
        }
    }

    Ok(ScanResult { pairs, stats, skipped_names })
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_scan_empty_directory() {
        let dir = tempdir().unwrap();
        let result = scan_directory(dir.path()).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.pairs, 0);
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let result = scan_directory(Path::new("/nonexistent/path/12345"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn test_scan_file_not_directory() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notadir.txt");
        File::create(&file_path).unwrap();

        let result = scan_directory(&file_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn test_scan_with_valid_pairs() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 2);
        assert_eq!(result.stats.pairs, 2);
        assert_eq!(result.stats.skipped, 0);
        assert_eq!(result.stats.orphaned, 0);
    }

    #[test]
    fn test_scan_skips_aria2_files_stem() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.aria2")).unwrap();

        File::create(path.join("video2.mp4")).unwrap();
        File::create(path.join("video2.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.stats.skipped, 1);
        assert!(result.skipped_names.contains(&"video1".to_string()));
    }

    #[test]
    fn test_scan_skips_aria2_files_mp4_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.mp4.aria2")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_skips_aria2_files_m4a_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        File::create(path.join("video1.m4a")).unwrap();
        File::create(path.join("video1.m4a.aria2")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.skipped, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_mp4() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.mp4")).unwrap();
        // No matching m4a

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_counts_orphaned_m4a() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        File::create(path.join("video1.m4a")).unwrap();
        // No matching mp4

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.stats.orphaned, 1);
    }

    #[test]
    fn test_scan_ignores_subdirectories() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Create subdirectory with files
        let subdir = path.join("subdir");
        fs::create_dir(&subdir).unwrap();
        File::create(subdir.join("video.mp4")).unwrap();
        File::create(subdir.join("video.m4a")).unwrap();

        let result = scan_directory(path).unwrap();
        assert_eq!(result.pairs.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test scanner::scan_tests`
Expected: All 10 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/scanner.rs
git commit -m "feat: add directory scanner with aria2 filtering and comprehensive tests"
```

---

## Chunk 4: Merger Module

### Task 11: Write Tests and Implement Merge Result Types

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Define result types with tests**

```rust
// src/merger.rs
use std::path::PathBuf;

/// Result of a single merge operation
#[derive(Debug)]
pub struct MergeResult {
    /// Index of the pair in the original pairs vector
    pub pair_index: usize,
    /// The file pair that was processed
    pub pair_name: String,
    /// Whether the merge succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Summary of all merge operations
#[derive(Debug, Default)]
pub struct MergeSummary {
    /// Number of successful merges
    pub success_count: usize,
    /// Number of failed merges
    pub failed_count: usize,
    /// Number of skipped pairs (aria2)
    pub skipped_count: usize,
    /// Number of orphaned files
    pub orphaned_count: usize,
    /// List of failed merges with errors
    pub failures: Vec<(String, String)>,
}

impl MergeSummary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all operations succeeded
    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_summary_default() {
        let summary = MergeSummary::default();
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert!(summary.all_success());
    }

    #[test]
    fn test_merge_summary_all_success_false_with_failures() {
        let mut summary = MergeSummary::default();
        summary.failed_count = 1;
        assert!(!summary.all_success());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test merger::tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/merger.rs
git commit -m "feat: add MergeResult and MergeSummary types with tests"
```

---

### Task 12: Write Tests and Implement Single File Merge

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Add merge function with timeout and tests**

```rust
// Add to src/merger.rs
use crate::cli::OutputFormat;
use crate::ffmpeg;
use crate::scanner::FilePair;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;

/// Default timeout for ffmpeg process (5 minutes)
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);

/// Extension trait for waiting with timeout
trait ChildExt {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, std::io::Error>;
}

impl ChildExt for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, std::io::Error> {
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
                Err(e) => return Err(e),
            }
        }
    }
}

/// Merge a single file pair
pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    output_dir: &Path,
    format: OutputFormat,
) -> MergeResult {
    use colored::Colorize;

    let output_path = output_dir.join(format!("{}.{}", pair.stem, format.extension()));

    let mut cmd = ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, format);

    match run_with_timeout(&mut cmd, FFMPEG_TIMEOUT) {
        Ok(status) if status.success() => {
            println!("{} {}", "✓".green(), pair.stem);
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: true,
                error: None,
            }
        }
        Ok(status) => {
            println!("{} {}: ffmpeg exited with code {:?}", "✗".red(), pair.stem, status.code());
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(format!("ffmpeg exited with code {:?}", status.code())),
            }
        }
        Err(e) => {
            println!("{} {}: {}", "✗".red(), pair.stem, e);
            MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                success: false,
                error: Some(e),
            }
        }
    }
}

/// Run a command with timeout
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<ExitStatus, String> {
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("ffmpeg process timed out after 5 minutes".to_string())
        }
        Err(e) => Err(format!("Failed to wait for ffmpeg: {}", e)),
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    // Note: These tests require ffmpeg to be installed
    // They test the function structure, not actual ffmpeg execution

    #[test]
    fn test_merge_result_debug() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            success: true,
            error: None,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_merge_result_with_error() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            success: false,
            error: Some("test error".to_string()),
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test merger::merge_tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/merger.rs
git commit -m "feat: add single file merge with timeout support and tests"
```

---

### Task 13: Write Tests and Implement Parallel Merge Execution

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Add parallel merge function with tests**

```rust
// Add to src/merger.rs
use rayon::prelude::*;
use crate::scanner::{ScanResult, ScanStats};

/// Execute parallel merges with controlled concurrency
pub fn execute_merges(
    scan_result: ScanResult,
    output_dir: &Path,
    format: OutputFormat,
    jobs: usize,
    delete_source: bool,
) -> MergeSummary {
    let output_dir = output_dir.to_path_buf();

    // Store pairs for later reference during deletion
    let pairs = scan_result.pairs.clone();

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
                delete_source_files(pair);
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

/// Delete source files after successful merge
fn delete_source_files(pair: &FilePair) {
    if let Err(e) = std::fs::remove_file(&pair.video) {
        eprintln!("Warning: Failed to delete {}: {}", pair.video.display(), e);
    }
    if let Err(e) = std::fs::remove_file(&pair.audio) {
        eprintln!("Warning: Failed to delete {}: {}", pair.audio.display(), e);
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_execute_merges_empty_pairs() {
        let dir = tempdir().unwrap();
        let scan_result = ScanResult {
            pairs: vec![],
            stats: ScanStats::default(),
            skipped_names: vec![],
        };

        let summary = execute_merges(
            scan_result,
            dir.path(),
            OutputFormat::Mkv,
            1,
            false,
        );

        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert!(summary.all_success());
    }

    #[test]
    fn test_execute_merges_with_skipped_stats() {
        let dir = tempdir().unwrap();
        let scan_result = ScanResult {
            pairs: vec![],
            stats: ScanStats {
                pairs: 0,
                skipped: 5,
                orphaned: 3,
            },
            skipped_names: vec![],
        };

        let summary = execute_merges(
            scan_result,
            dir.path(),
            OutputFormat::Mkv,
            1,
            false,
        );

        assert_eq!(summary.skipped_count, 5);
        assert_eq!(summary.orphaned_count, 3);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test merger::exec_tests`
Expected: Tests pass

- [ ] **Step 3: Commit**

```bash
git add src/merger.rs
git commit -m "feat: add parallel merge execution with rayon and tests"
```

---

### Task 14: Implement Summary Report

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Add report formatting**

```rust
// Add to src/merger.rs
use colored::Colorize;

impl MergeSummary {
    /// Print a formatted summary report
    pub fn print_report(&self) {
        println!("{}", "================================".bright_black());
        println!("{}", "Merge complete".green().bold());
        println!("{}: {}", "Success".green(), self.success_count);
        println!("{}: {}", "Failed".red(), self.failed_count);
        println!("{}: {} (aria2 files present)", "Skipped".yellow(), self.skipped_count);
        println!("{}: {} (missing pair)", "Orphaned".bright_black(), self.orphaned_count);
        println!("{}", "================================".bright_black());

        if !self.failures.is_empty() {
            println!("\n{}", "Failed files:".red());
            for (name, error) in &self.failures {
                println!("  - {}: {}", name, error);
            }
        }
    }
}
```

- [ ] **Step 2: Compile check**

Run: `cargo check`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/merger.rs
git commit -m "feat: add colored summary report output"
```

---

## Chunk 5: Main Orchestration

### Task 15: Wire Up Main Function

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update main with full workflow**

```rust
// src/main.rs
mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use clap::Parser;
use cli::Args;
use colored::Colorize;

fn main() {
    // Parse and validate arguments
    let mut args = Args::parse();
    if let Err(e) = args.validate() {
        eprintln!("{} {}", "Error:".red(), e);
        std::process::exit(1);
    }

    // Parse format early
    let format = match args.parsed_format() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            std::process::exit(1);
        }
    };

    // Phase 1: Check ffmpeg
    if !ffmpeg::ensure_ffmpeg() {
        std::process::exit(1);
    }

    // Phase 2: Scan directory
    let scan_result = match scanner::scan_directory(&args.source) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            std::process::exit(1);
        }
    };

    // Check if we have anything to do
    if scan_result.pairs.is_empty() {
        println!("{}", "No file pairs to merge".yellow());
        std::process::exit(0);
    }

    // Validate/create output directory
    if !args.output.exists() {
        if let Err(e) = std::fs::create_dir_all(&args.output) {
            eprintln!("{} Failed to create output directory: {}", "Error:".red(), e);
            std::process::exit(1);
        }
    }

    // Check output directory is writable
    if args.output.exists() {
        // Try to write a test file
        let test_file = args.output.join(".mixbilibili_write_test");
        if std::fs::File::create(&test_file).is_err() {
            eprintln!("{} Output directory is not writable: {}", "Error:".red(), args.output.display());
            std::process::exit(1);
        }
        let _ = std::fs::remove_file(&test_file);
    }

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
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
```

- [ ] **Step 2: Build release**

Run: `cargo build --release`
Expected: Builds successfully

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire up main function with complete workflow"
```

---

## Chunk 6: Integration Tests & Documentation

### Task 16: Add Integration Test - Help

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Add help test**

```rust
// tests/integration_test.rs
use std::process::Command;

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
```

- [ ] **Step 2: Run test**

Run: `cargo test --test integration_test test_help_flag`
Expected: Test passes

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for help flag"
```

---

### Task 17: Add Integration Test - Invalid Format

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add invalid format test**

```rust
// Add to tests/integration_test.rs

#[test]
fn test_invalid_format() {
    let output = Command::new(get_binary_path())
        .arg("-f")
        .arg("avi")
        .output()
        .expect("Failed to execute binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Check for the error message (may contain ANSI color codes)
    assert!(
        stderr.contains("Invalid format") && stderr.contains("avi"),
        "Expected error message not found in: {}",
        stderr
    );
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test integration_test test_invalid_format`
Expected: Test passes

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for invalid format"
```

---

### Task 18: Add Integration Test - Nonexistent Source

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add nonexistent source test**

```rust
// Add to tests/integration_test.rs

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
```

- [ ] **Step 2: Run test**

Run: `cargo test --test integration_test test_nonexistent_source`
Expected: Test passes

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for nonexistent source directory"
```

---

### Task 19: Add Integration Test - Empty Directory

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add empty directory test**

```rust
// Add to tests/integration_test.rs
use tempfile::tempdir;

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
```

- [ ] **Step 2: Run test**

Run: `cargo test --test integration_test test_empty_directory`
Expected: Test passes

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for empty directory"
```

---

### Task 20: Add README Documentation

**Files:**
- Create: `README.md`

- [ ] **Step 1: Create README**

```markdown
# mixbilibili

A cross-platform CLI tool for batch merging Bilibili downloaded video (`.mp4`) and audio (`.m4a`) files using ffmpeg.

## Installation

### Prerequisites
- Rust 1.70+
- ffmpeg (will prompt to install if not found)

### Build from source

```bash
git clone https://github.com/yourname/mixbilibili.git
cd mixbilibili
cargo build --release
```

The binary will be at `target/release/mixbilibili`.

## Usage

```bash
# Merge all mp4/m4a pairs in current directory
mixbilibili

# Specify source and output directories
mixbilibili -s /path/to/downloads -o /path/to/output

# Use mp4 format with 4 parallel jobs
mixbilibili -f mp4 -j 4

# Keep source files after merge
mixbilibili --sdel false
```

## Options

| Flag | Description | Default |
|------|-------------|---------|
| `-s, --source` | Source directory | `.` |
| `-o, --output` | Output directory | `.` |
| `-d, --sdel` | Delete source files after merge | `true` |
| `-f, --format` | Output format (mkv/mp4/mov) | `mkv` |
| `-j, --jobs` | Parallel ffmpeg processes | CPU cores |

## Features

- **Automatic pairing**: Matches `video.mp4` with `video.m4a`
- **aria2 awareness**: Skips files currently being downloaded (detects `.aria2` control files)
- **Parallel processing**: Configurable concurrency with `-j` flag
- **Cross-platform**: Works on Windows, macOS, and Linux
- **ffmpeg auto-install**: Prompts to install ffmpeg if not found

## License

MIT
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add README with usage instructions"
```

---

### Task 21: Final Verification

**Files:**
- None (verification task)

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Build release**

Run: `cargo build --release`
Expected: Clean build

- [ ] **Step 4: Test binary manually**

Create test files and verify the tool works:

```bash
mkdir -p /tmp/mixbilibili-test
cd /tmp/mixbilibili-test
touch video1.mp4 video1.m4a
touch video2.mp4 video2.m4a

# Run the tool (requires ffmpeg for actual merging)
/path/to/target/release/mixbilibili -s . -o ./output
```

- [ ] **Step 5: Final commit**

```bash
git add -A
git status
git commit -m "chore: final build verification"
```

---

## Summary

This plan creates a complete Rust CLI tool with:
- CLI parsing via clap derive with `ValueEnum` for format validation
- Comprehensive unit tests alongside implementation code
- ffmpeg environment management with OS-specific install prompts
- Directory scanning with aria2 file filtering
- Parallel merge execution with rayon (using pair indices for efficient deletion)
- Colored output and summary reporting
- Integration tests for CLI behavior
- Each test as a separate task for proper TDD tracking