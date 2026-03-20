# mixbilibili CLI - Design Specification

**Date:** 2026-03-20
**Status:** Draft
**Language:** Rust

## 1. Overview

`mixbilibili` is a cross-platform CLI tool for batch merging Bilibili downloaded video (`.mp4`) and audio (`.m4a`) files using ffmpeg.

## 2. CLI Arguments

| Short | Long | Type | Default | Description |
|-------|------|------|---------|-------------|
| `-s` | `--source` | PathBuf | `.` | Source directory containing mp4/m4a files |
| `-o` | `--output` | PathBuf | `.` | Output directory for merged files (auto-created) |
| `-d` | `--sdel` | bool | `true` | Delete source files after successful merge |
| `-f` | `--format` | String | `mkv` | Output format (mkv, mp4, mov) |
| `-j` | `--jobs` | usize | CPU cores | Max concurrent ffmpeg processes |

## 3. Architecture

```
src/
├── main.rs      # CLI parsing, workflow orchestration
├── ffmpeg.rs    # ffmpeg check, installation, command builder
├── scanner.rs   # Directory scan, file pairing, aria2 filter
└── merger.rs    # Parallel merge execution, cleanup
```

## 4. Workflow

### 4.1 Phase 1: Environment Check
1. Check if `ffmpeg` exists in system PATH (using `which` crate)
2. If not found:
   - Detect OS (Windows/macOS/Linux)
   - Prompt user: "ffmpeg not found. Install via {package-manager}? [y/N]"
   - If yes: Run appropriate package manager command
   - If no or failure: Print manual install instructions, exit with code 1

### 4.2 Phase 2: File Scan & Pairing
1. Scan source directory for `.mp4` and `.m4a` files
2. Group by stem (filename without extension)
3. Filter out pairs with `.aria2` control files:
   - Skip if `{stem}.aria2` exists
   - Skip if `{stem}.mp4.aria2` exists
   - Skip if `{stem}.m4a.aria2` exists

### 4.3 Phase 3: Parallel Merge
1. Use `rayon` for parallel iteration over valid pairs
2. Control concurrency with `--jobs` parameter
3. Execute ffmpeg command:
   ```bash
   ffmpeg -hide_banner -loglevel error \
     -i "{video}.mp4" -i "{audio}.m4a" \
     -c:v copy -c:a copy -movflags +faststart \
     -y "{output}/{stem}.{format}"
   ```

### 4.4 Phase 4: Cleanup & Report
1. On merge success + `--sdel=true`: Delete source `.mp4` and `.m4a`
2. On merge failure: Keep source files, log error
3. Print summary report with success/fail/skip counts

## 5. Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
which = "6"
walkdir = "2"
rayon = "1"
num_cpus = "1"
colored = "2"  # Optional: for colored output
```

## 6. Error Handling

- **Continue on failure**: Individual merge failures don't stop batch processing
- **Summary report**: List all failed files with reasons at the end
- **Exit codes**:
  - `0`: All merges successful
  - `1`: Some merges failed or ffmpeg not available

## 7. Platform-Specific Behavior

| Platform | Package Manager | Install Command |
|----------|-----------------|-----------------|
| Windows | winget | `winget install ffmpeg` |
| Windows | choco (fallback) | `choco install ffmpeg` |
| macOS | brew | `brew install ffmpeg` |
| Linux (Debian/Ubuntu) | apt | `sudo apt update && sudo apt install ffmpeg` |

## 8. Output Format Support

- `mkv`: Default, best container for streaming
- `mp4`: Widely compatible
- `mov`: For Apple ecosystem

Note: All formats use stream copy (`-c:v copy -c:a copy`), no re-encoding.