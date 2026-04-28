# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.1] - 2026-04-28

### Added
- `install.sh` script for local build & install to `/usr/local/bin/mix`

### Fixed
- Progress bar triple-rendering issue with long filenames
- Removed per-item `set_message` calls that caused excessive redraws
- Simplified progress bar template (no filename, avoids terminal overflow)

## [0.4.0] - 2026-04-28

### Added
- Process timeout protection (5 min) with automatic kill for stalled ffmpeg
- Incremental state saving (every 5 merges) for interruption recovery
- Detailed timing report (total/avg/min/max/throughput) in merge summary

### Changed
- Refactored `merge_pair` into `do_dry_run` / `do_merge` for clarity
- Simplified `delete_source_files` — replaces 4-way match with error collection
- Simplified scanner pairing logic with `if let` patterns
- Simplified `finalize` — early return instead of nested `if !args.dry_run`
- Extracted `accumulate_summary` / `update_state_from_batch` helpers
- Extracted `package_managers()` to eliminate cross-function duplication in ffmpeg module
- Replaced `.contains(&stem.to_string())` with `.iter().any(\|s\| s == stem)` in state.rs
- Extracted `make_args()` test helper in cli.rs
- Removed redundant file header comments (`// src/xxx.rs`)

### Fixed
- Retry loop rebuilt ffmpeg command on each attempt (previously reused stale command)
- Deletion failure error messages now include both file paths on dual failure

## [0.3.0] - 2026-04-27

### Added
- Progress bar for batch operations (--progress, default enabled)
- Dry-run mode for preview without executing (--dry-run)
- Verbose output mode showing ffmpeg commands (--verbose)
- Resume capability for interrupted batches (--resume)
- Retry logic for transient ffmpeg failures (--retry N)
- State file tracking for resume (.mixbilibili_state.json)

### Changed
- Extended CLI with new user experience flags
- Improved output feedback during batch processing

## [0.2.0] - 2026-03-22

### Added
- Comprehensive error handling with `anyhow` crate
- GitHub Actions CI/CD pipeline for automated testing and releases
- Core functionality tests for merger module (63 tests total)
- Structured exit codes (0=success, 1=error, 2=ffmpeg not found, 3=merge failed)
- Input validation for source and output directories
- CHANGELOG.md and CONTRIBUTING.md documentation

### Changed
- Improved error messages with context information
- Enhanced input validation for source and output directories
- Replaced `num_cpus` dependency with standard library
- Pinned all dependencies to specific versions

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