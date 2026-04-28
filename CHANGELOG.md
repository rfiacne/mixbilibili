# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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