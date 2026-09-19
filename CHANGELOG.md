# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- `main.rs` split (1137 → 28 lines): 4-phase orchestration moved to new `workflow` module, `AppError`/exit-code mapping to new `error` module; exit codes 1/2/3 unchanged
- State schema upgraded to v2 with a per-version migration chain (`migrations()`); v1 files (with or without `version`) migrate forward transparently on load
- Output validation upgraded from "non-zero size" to size consistency: output must be >= source video size (invalid undersized outputs are cleaned up)
- `merger.rs` split into `merger/` submodules (merge/runner/summary/delete) with no public API change
- Stem normalization unified into `scanner::make_key` / `scanner::safe_stem`; `--once` and directory scan no longer maintain separate composite-key rules
- ffmpeg commands now use the path resolved by `which`, so a minimal PATH (e.g. aria2 hook) invokes the same binary that detection validated
- Child-process waiting now polls with exponential backoff (1 ms → 50 ms) instead of a fixed 100 ms interval, removing up to one full interval of dead time from every merge

### Performance
- Batch merge throughput improved ~39% on a 200-pair `-c copy` benchmark (200 merges: 2.64s → 1.61s on 8 threads), now within ~9% of the raw `xargs -P8 ffmpeg` ceiling. The fixed 100 ms poll interval dominated merges that finish in ~30 ms.
- Scan/pairing phase roughly 2x faster on large trees (20000 pairs: preview 210 ms → 120 ms, quiet scan 130 ms → 60 ms). Three causes, all per-entry work in the hot loop:
  - `entry.context(t("...").into_owned())` built the error message — a hash lookup plus a `String` — for *every* directory entry, on the happy path. Now lazy via `with_context`.
  - The relative-parent key was re-derived from each file's path (`strip_prefix` + allocation per file). It is a property of the directory, so it is now carried down the recursion.
  - `path.is_dir()` issued a second `stat()` per entry even though `read_dir` already reported the type; the entry's `file_type()` is now used once for both the symlink and directory checks.
- Preview rendering writes through one buffered, locked stdout: `println!` per row meant a `write()` syscall per row (std's stdout is line-buffered), which dominated the scan phase on large lists. Column padding is also written straight to the buffer instead of allocating a `String` per column per row.

### Added
- Per-stem output size anchors recorded on merge success; `--resume` warns (non-blocking) when a completed output's size differs from its recorded anchor
- `--once` stem validation (`validate_stem`) rejecting path separators, null bytes, empty input, and `.`/`..`
- Unit tests for the poll backoff schedule (doubling, `POLL_MAX` clamp, zero-delay guard) plus a fast-child detection regression test
- Tests pinning the recursive composite-key format for multi-level trees (nested keys decide output filenames) and the preview's exact two-column CJK alignment

### Fixed
- Translation-completeness test only scanned top-level `src/*.rs`, so the `src/merger/` submodules added in the module split were silently never validated. The scan is now recursive and asserts it visited those files, so it cannot go blind again.
- State migration driver stamped `CURRENT_SCHEMA_VERSION` from inside `migrate_v1_to_v2`; on the next schema bump, a v1 file would have been marked current while every later migration was skipped, loading state that claims a version whose fields it does not have. The driver now owns the version bump, and a new test fails if `CURRENT_SCHEMA_VERSION` is raised without a matching migration entry.

### Fixed
- Silent data loss: distinct composite keys that flatten to the same output filename (`sub/a` vs `sub_a`) are now rejected upfront instead of overwriting

## [0.8.0] - 2026-07-19

### Added
- `--ascii` flag to force ASCII symbols (disable Unicode output)
- UI foundation module: terminal symbols (Unicode/ASCII dual-mode) and semantic colors
- Box-drawing preview list with two-column video/audio alignment and CJK-aware width calculation
- UI component translation keys (report header, preview list, interactive confirmation)

### Changed
- Redesigned report header, progress bar, and interactive confirmation with highlighted defaults
- Interactive confirmation now default-rejects when `--sdel` is enabled (destructive mode)
- Error messages refactored to use Symbols and Colors for consistent styling
- Consolidated ffmpeg translation keys and removed stale dead_code allows
- Documentation structure reorganized

### Fixed
- Hardened recursive scan: deterministic pair ordering, composite key safety
- Source-overwrite safety: reject source == output when format would overwrite video
- CJK display width corrected via unicode-width in preview alignment
- Reset SIGCHLD to SIG_DFL before spawning ffmpeg (aria2 hook sets SIG_IGN, causing waitpid ECHILD)
- Prevent source file deletion on interrupt
- Resolved TOCTOU races, i18n regression, and cleanup bugs

## [0.7.0] - 2026-06-24

### Added
- `--once <stem>` flag to merge a single file pair without scanning the directory
- aria2 integration hook script (`scripts/aria2-hook.sh`) for automatic merge on download completion
- i18n keys for `--once` mode error messages

### Changed
- README updated with aria2 integration guide and `--once` usage

## [0.6.2] - 2026-05-16

### Fixed
- Progress bar counter now correctly displays completion count when processing multiple chunks
  - Previously showed incorrect values like 10/9, 11/9 when `STATE_SAVE_INTERVAL` split batches
  - `finish()` is now called once at the end of all processing, not after each chunk

## [0.6.1] - 2026-05-03

### Changed
- Language detection now uses OS-level system preferences instead of environment variables:
  - macOS: `defaults read -g AppleLanguages` (system preferences)
  - Windows: `GetUserDefaultUILanguage` (Win32 API)
  - Linux: `LANG` / `LC_ALL` environment variables (standard)

### Fixed
- All tests made locale-agnostic — pass regardless of system language setting
- Integration tests accept both Chinese and English output strings

## [0.6.0] - 2026-05-02

### Added
- Automatic language detection: outputs Simplified Chinese when system `LANG` starts with `zh`
- Full translation of all user-facing strings including `--help` text
- Zero external dependencies — lightweight `i18n` module with match-based translation table

## [0.5.0] - 2026-05-01

### Added
- `--quiet/-q` flag to suppress progress output, show compact summary
- Enhanced `--dry-run` output: lists pairs and source files that would be modified
- Structured merge report with timing stats, throughput, and failure details
- PR CI check workflow (fmt + clippy + test gate)
- Binary size report in release build output
- Automatic changelog extraction for GitHub release notes

### Changed
- Extracted `ChildExt::wait_with_timeout` trait from inline polling logic
- Structured final report with quiet mode support

### Fixed
- Thread pool build error now returns `Result` instead of panicking (`expect()`)
- Removed fragile string-matching exit code fallback — pure type-driven now
- Fixed clippy warnings for test profile

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