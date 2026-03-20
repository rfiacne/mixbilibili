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