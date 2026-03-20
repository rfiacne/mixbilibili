# mixbilibili CLI - Design Specification

**Date:** 2026-03-20
**Status:** Under Review
**Language:** Rust

## 1. Overview

`mixbilibili` is a cross-platform CLI tool for batch merging Bilibili downloaded video (`.mp4`) and audio (`.m4a`) files using ffmpeg.

## 2. CLI Arguments

| Short | Long | Type | Default | Description |
|-------|------|------|---------|-------------|
| `-s` | `--source` | PathBuf | `.` | Source directory containing mp4/m4a files |
| `-o` | `--output` | PathBuf | `.` | Output directory for merged files (auto-created) |
| `-d` | `--sdel` | bool | `true` | Delete source files after successful merge |
| `-f` | `--format` | String | `mkv` | Output format (mkv, mp4, mov) - **Enhancement** |
| `-j` | `--jobs` | usize | CPU cores | Max concurrent ffmpeg processes - **Enhancement** |

### Input Validation

| Argument | Validation | On Invalid |
|----------|------------|------------|
| `-f/--format` | Must be one of: `mkv`, `mp4`, `mov` (case-insensitive) | Exit 1 with error: "Invalid format '{value}'. Supported: mkv, mp4, mov" |
| `-j/--jobs` | Must be >= 1 and <= 32 | Clamp to valid range (1-32), log warning |

> **Design Note:** `-f/--format` and `-j/--jobs` are intentional enhancements beyond original requirements. Rationale: Configurable output format is a common user request for compatibility with different players; parallel processing with job control is industry standard for batch media tools.

## 3. Architecture

```
src/
├── main.rs      # CLI parsing, workflow orchestration
├── ffmpeg.rs    # ffmpeg check, installation, command builder
├── scanner.rs   # Directory scan, file pairing, aria2 filter
└── merger.rs    # Parallel merge execution, cleanup
```

> **Cross-Platform Path Handling:** All file paths use `std::path::PathBuf` throughout the codebase to ensure correct handling of path separators across Windows, macOS, and Linux.

## 4. Workflow

### 4.1 Phase 1: Environment Check
1. Check if `ffmpeg` exists in system PATH (using `which` crate)
2. If not found:
   - Detect OS (Windows/macOS/Linux)
   - Prompt user: "ffmpeg not found. Install via {package-manager}? [y/N]"
   - If yes: Run appropriate package manager command
     - **Windows**: Try `winget` first; if not available, fall back to `choco`
   - If no or failure: Print manual install instructions (see Section 7), exit with code 1

> **Design Note:** Original requirements specified automatic installation without prompting. Changed to prompted installation to give users control over system modifications.

### 4.2 Phase 2: File Scan & Pairing
1. Validate source directory exists; exit with error if not found
2. Validate output directory is writable (create if needed, or check permissions); exit with error if not writable
3. Scan source directory (non-recursive) for `.mp4` and `.m4a` files
4. Group by stem (filename without extension)
5. Filter out pairs with `.aria2` control files (checked in source directory only):
   - **Skip pair if ANY of the following aria2 files exist:**
     - `{stem}.aria2`
     - `{stem}.mp4.aria2`
     - `{stem}.m4a.aria2`
6. If no valid pairs found: Print message "No file pairs to merge", exit with code 0

> **Note:** Directory scanning is non-recursive. Only files directly in the source directory are processed.

### 4.3 Phase 3: Parallel Merge
1. Create output directory if it doesn't exist (handle permission errors)
2. Use `rayon` for parallel iteration over valid pairs
3. Control concurrency with `-j` parameter
4. Execute ffmpeg command based on output format:

**For MKV:**
```bash
ffmpeg -hide_banner -loglevel error \
  -i "{video}.mp4" -i "{audio}.m4a" \
  -c:v copy -c:a copy \
  -y "{output}/{stem}.mkv"
```

**For MP4/MOV:**
```bash
ffmpeg -hide_banner -loglevel error \
  -i "{video}.mp4" -i "{audio}.m4a" \
  -c:v copy -c:a copy -movflags +faststart \
  -y "{output}/{stem}.{format}"
```

> **Note:** `-movflags +faststart` is only valid for MP4/MOV containers, not MKV.
>
> **Correction from Requirements:** The original requirements specified `-movflags +faststart` for MKV output, which is technically invalid (ffmpeg ignores it for MKV). This spec corrects the command to only apply the flag for MP4/MOV formats where it has effect.

### 4.4 Phase 4: Cleanup & Report
1. On merge success + `--sdel=true`: Delete source `.mp4` and `.m4a`
   - Handle permission errors: Log warning, continue
2. On merge failure: Keep source files, log error
3. Print summary report with success/fail/skip counts
4. Set ffmpeg process timeout (default: 5 minutes per file) to prevent hanging

## 5. Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
which = "6"
rayon = "1"
num_cpus = "1"
colored = "2"  # Optional: for colored output
```

> **Note:** `walkdir` removed since scanning is non-recursive; using `std::fs::read_dir` instead.

## 6. Error Handling

### Exit Codes
| Code | Meaning |
|------|---------|
| 0 | Success (all merges completed successfully, or no pairs to merge) |
| 1 | ffmpeg not available, OR at least one merge failed |

> **Note:** Exit code 1 covers two scenarios: (1) ffmpeg not found and user declined installation; (2) ffmpeg available but one or more merges failed. These are mutually exclusive since merge attempts only proceed if ffmpeg is available. |

### Error Categories
| Error | Handling |
|-------|----------|
| ffmpeg not found | Prompt to install, or show manual instructions |
| Source directory not found | Exit with error message |
| Output directory creation failed | Exit with permission error |
| No valid file pairs | Exit 0 with message |
| Individual merge failure | Log error, continue, report at end |
| File deletion permission error | Log warning, continue |
| ffmpeg process timeout | Kill process, log error, continue |

### Summary Report Format
```
================================
Merge complete
Success: 15
Failed:  2
Skipped: 3 (aria2 files present)
Orphaned: 4 (missing pair)
================================

Failed files:
  - video1: ffmpeg exited with code 1
  - video2: Permission denied
```

> **Count Definitions:**
> - **Success**: Merged successfully
> - **Failed**: Merge attempted but failed
> - **Skipped**: Valid pair found but aria2 file exists (download in progress)
> - **Orphaned**: mp4 or m4a file without matching pair (not processed) - **Enhancement**

## 7. Platform-Specific Behavior

### Package Managers

| Platform | Package Manager | Install Command |
|----------|-----------------|-----------------|
| Windows | winget | `winget install ffmpeg` |
| Windows | choco (fallback) | `choco install ffmpeg` |
| macOS | brew | `brew install ffmpeg` |
| Linux (Debian/Ubuntu) | apt | `sudo apt update && sudo apt install ffmpeg` |

### Manual Installation Instructions

**Windows:**
```
To install ffmpeg manually:
1. Using winget: winget install ffmpeg
2. Using Chocolatey: choco install ffmpeg
3. Manual download: https://ffmpeg.org/download.html
   Download the Windows build, extract, and add to PATH.
```

**macOS:**
```
To install ffmpeg manually:
1. Using Homebrew: brew install ffmpeg
2. Using MacPorts: sudo port install ffmpeg
3. Manual download: https://ffmpeg.org/download.html
```

**Linux (Debian/Ubuntu):**
```
To install ffmpeg manually:
1. Using apt: sudo apt update && sudo apt install ffmpeg
2. Using snap: sudo snap install ffmpeg
3. Manual build: https://trac.ffmpeg.org/wiki/CompilationGuide
```

## 8. Output Format Support

| Format | Container | `-movflags +faststart` | Use Case |
|--------|-----------|------------------------|----------|
| mkv | Matroska | No (invalid) | Default, best for streaming |
| mp4 | MP4 | Yes | Widely compatible, web streaming |
| mov | QuickTime | Yes | Apple ecosystem |

> **Note:** All formats use stream copy (`-c:v copy -c:a copy`), no re-encoding.

## 9. Edge Cases

| Scenario | Behavior |
|----------|----------|
| Source == Output directory | Merge proceeds; `-y` overwrites existing output files |
| Source file already exists as output | `-y` flag causes overwrite |
| Permission denied on source files | Log error, skip pair, continue |
| Permission denied on output directory | Exit early with error |
| Corrupted mp4/m4a file | ffmpeg fails, log error, continue |
| Empty source directory | Exit 0 with "No file pairs to merge" |
| Only mp4 without matching m4a | Count as "Orphaned", not processed |
| Only m4a without matching mp4 | Count as "Orphaned", not processed |
| Subdirectories in source directory | Ignored (non-recursive scan) |