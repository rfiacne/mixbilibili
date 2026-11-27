# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.1] - 2026-11-27

Documentation only: no behaviour change.

### Changed
- The README now matches the CLI. It lists all fifteen flags (`--interactive`,
  `-R`, `--ascii` and `-p` were missing), documents the exit codes, the output
  naming rule including how `-R` flattens a composite stem into one file name,
  the size check that catches truncated merges, and the fact that `-f mp4` into
  the source directory is refused rather than overwriting the source video. It
  no longer promises macOS binaries, which are not built
- `CONTRIBUTING.md` describes the checks CI actually runs, including
  `deslop-please`, and the tag-only release split

### Fixed
- `AGENTS.md` stated the MSRV as "Rust 2021 edition", which names an edition
  rather than a version. The real floor is 1.88, set by the dependency graph
- Two archived `openspec` proposals listed fully checked tasks that the
  `52ad585` reconciliation had since undone (`ffmpeg::Os`, `package_managers`,
  `get_install_command` are present again). They now open with what no longer
  holds, so an archived checklist cannot be mistaken for the current tree
- The 2026-08-13 roadmap is marked as a v0.8.0 snapshot

## [0.10.0] - 2026-11-27

Reconciled two diverged development lines (local v0.8.1 and upstream v0.9.1)
on the upstream architecture. Upstream's `MergeRunner` seam, library interface
and pipeline are the base; the local line's independent improvements were
carried over on top of it. A review pass over that result follows; its fixes
are listed under Changed and Fixed below.

### Changed
- Child-process waiting now polls with exponential backoff (1 ms → 50 ms)
  instead of a fixed 100 ms interval, removing up to one full interval of dead
  time from every merge. The schedule is extracted into `next_poll_delay()` so
  it is unit-testable without timing assertions
- Output validation upgraded from "non-zero size" to size consistency: the
  output must be >= the source video size, since `-c copy` output carries the
  video stream plus the audio stream plus container headers. Truncated merges
  that ffmpeg reported as successful are now caught and cleaned up
- Scanning no longer builds error context eagerly per entry (`with_context`
  runs the closure only on failure), derives the relative-parent key once per
  *directory* instead of once per file, and takes a single `file_type()` per
  entry instead of a second `stat()` via `Path::is_dir()`
- The declared minimum Rust version is 1.88, matching what the dependency graph
  actually requires (`which` → `home 0.5.12`), and a CI job now builds on that
  version with `--locked` so the claim is verified rather than asserted

### Fixed
- An explicitly set, non-UTF-8 locale (`LANG=C`, `LC_ALL=POSIX`) now selects
  ASCII symbols on every platform. Previously the macOS fallback overrode it,
  so a macOS terminal with `LANG=C` still got Unicode output
- The preview box is closed on all four sides. Its width calculation omitted
  the output-name column and the right border was never drawn, so the top and
  bottom borders were far shorter than the rows
- The merge report box is closed on all four sides. Its title row was unpadded,
  the mid separator was drawn with the *top* corners (a second `┌───┐` line),
  and the width was pinned at 40 columns so wider rows overflowed. Its failure
  connectors also hardcoded Unicode box characters even under `--ascii`
- `--dry-run` no longer creates the output directory and no longer requires
  ffmpeg to be installed; a declined `--interactive` prompt no longer leaves an
  output directory behind either
- `--once` behaves as the batch of one it is documented to be: it prints the
  report (with `-q`, one summary line and nothing else), honours
  `--interactive`, and reports an interrupt. It still writes no resume state,
  so it does not print the `--resume` hint
- A failed merge no longer deletes an output file it never touched. An earlier
  run's finished merge at that path used to be removed when the new run failed
  before ffmpeg opened it
- A run that cannot merge no longer rewrites `.mixbilibili_state.json`. With
  ffmpeg missing, the state was saved (and, without `--resume`, rebuilt) before
  the failure was detected, discarding earlier progress
- Cancelling an `--interactive` run no longer deletes a resume state it did not
  create
- The ffmpeg install prompt is skipped when stdin is not a terminal, so a
  download hook or CI run no longer blocks on it or consumes the stdin its
  parent handed it. The exit code for a missing ffmpeg is still 2
- Two translation keys that were called but no longer present in the table
  (`starting_merges`, `invalid_stem`) are restored; in debug builds they
  panicked, in release builds the key name reached the user. Every remaining
  hardcoded user-visible string now goes through the translation table

### Performance
- Preview, deletion-preview and report rendering are buffered. `println!` per
  row cost one `write()` syscall per row (std's stdout flushes on every
  newline), so a 20000-row preview spent ~70ms in syscalls alone
- Row rendering writes padding straight to the buffer instead of allocating a
  `String` per column per row, and the row index goes through a stack buffer

## [0.9.1] - 2026-09-09

### Fixed
- `--dry-run --once <stem>` no longer merges and deletes sources; it previews the
  pair like the batch path does (regression introduced when `--once` became a
  batch of one)
- Windows: hand-written state-file tests escape path backslashes, symlink
  scanner tests skip instead of failing without developer-mode privileges, and
  the progress-record test accepts the ASCII symbol fallback

## [0.9.0] - 2026-08-30

### Fixed
- Ctrl+C no longer deletes the resume state file: an interrupted batch keeps its
  pending pairs and its sources, so the `--resume` hint it prints is real
- A failed merge now says what ffmpeg said ("...after 0 retries: Error opening
  input files: Invalid data found when processing input") instead of an exit code
- A failed merge no longer leaves a partial output file behind
- `C.utf8` (no dash) now counts as a UTF-8 locale for symbol output

### Changed
- `-q/--quiet` really suppresses the per-pair lines — the documented promise —
  and ffmpeg's stderr is captured rather than inherited, so it cannot leak past
  `-q` or scramble the progress bar
- The crate is a library plus a thin `main.rs` adapter: tests drive
  `mixbilibili::run()` and the module interfaces directly
- The ffmpeg invocation sits behind an injected `MergeRunner`; retry, timeout,
  output validation, partial cleanup, source deletion and resume state are
  tested without ffmpeg (`tests/merge_policy.rs`)
- A cancelled merge is distinct from a timed-out one: the pair stays pending
  instead of being recorded as a timeout failure
- `--once` is a batch of one pair through the same pipeline as a scan; it does
  not start a thread pool
- The global `INTERRUPTED` flag is replaced by an injected `Interrupt` token,
  which also reaches the poll loop that kills a running ffmpeg
- `MergeSummary` and `MergeState` fields are private; reports are built by
  `from_results`, state is read through `is_completed`/`is_failed`/`is_pending`
- `FilePair` owns the naming convention, so the preview can no longer promise a
  file name the run does not write
- The CLI is declared once via clap derive; a bad `--format` is rejected by clap
  instead of silently becoming `mkv`
- Unknown i18n keys panic in debug builds; every key must exist in both
  languages; 15 unreferenced keys removed

## [0.8.0] - 2026-07-19

### Added
- `src/ui` module: `Symbols` and `Colors` for box-drawing, Unicode/ASCII-aware
  output, and a `--ascii` flag (`MIXBILIBILI_ASCII`) that forces ASCII symbols
- Redesigned preview list, interactive confirmation and merge report
- Merge state file carries a schema version, checked on load

### Fixed
- SIGCHLD is reset to `SIG_DFL` before any thread starts, so `waitpid` works
  when the tool is invoked from aria2's on-download-complete hook
- Source files are no longer deleted for a pair whose merge was interrupted
- Recursive scans key pairs by `subdir/stem`, so same-named files in different
  directories no longer collide; scan order is deterministic
- Refusing a source==output run when the output format would overwrite the
  source video, checked at start-up and again per merge

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