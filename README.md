# mixbilibili

A cross-platform CLI tool for batch merging Bilibili downloaded video (`.mp4`)
and audio (`.m4a`) files using ffmpeg.

Merging is a stream copy (`-c:v copy -c:a copy`): no re-encoding, no quality
loss, and a full-length merge finishes in seconds.

## Installation

### Prebuilt binaries

Download the archive for your platform from
[Releases](https://github.com/rfiacne/mixbilibili/releases):

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `mixbilibili-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `mixbilibili-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `mixbilibili-<version>-x86_64-pc-windows-msvc.zip` |

### From source

Requires Rust 1.88+ (see `rust-version` in `Cargo.toml`) and ffmpeg.

```bash
cargo build --release
# binary: target/release/mixbilibili
```

On Linux and macOS, `./install.sh` builds and installs to `/usr/local/bin`
(override with `--prefix`), and `./install.sh uninstall` removes it:

```bash
./install.sh --prefix ~/.local
```

macOS is supported from source; there is no prebuilt macOS artifact yet.

### ffmpeg

ffmpeg is required. If it is missing, mixbilibili offers to install it through
a package manager it can find (`winget` or `choco` on Windows, `brew` on macOS,
`apt`, `dnf`, `yum`, `pacman` or `zypper` on Linux). When stdin is not a
terminal — a download hook, cron, or CI — it prints manual instructions instead
of waiting for an answer, and exits with code 2.

## Usage

```bash
# Merge every pair in the current directory, writing next to the sources
mixbilibili

# Read from one directory, write to another
mixbilibili -s ~/Downloads/bilibili -o ~/Videos

# Merge into mp4 instead of the default mkv, with 4 parallel ffmpeg processes
mixbilibili -f mp4 -j 4

# Keep the source files (they are deleted after a successful merge by default)
mixbilibili --sdel false

# See exactly what would happen, and delete nothing
mixbilibili --dry-run

# See the plan and confirm before anything is touched
mixbilibili --interactive

# Also pick up pairs inside subdirectories
mixbilibili -R

# Merge one pair by name, without scanning
mixbilibili --once ep01
```

A pair is `X.mp4` plus `X.m4a`. Pairs whose `.aria2` control file is still
present are skipped — aria2 has not finished writing them — and are reported as
*skipped* rather than failed. Files with no partner are reported as *orphan* and
never touched.

## Options

| Flag | Description | Default |
|------|-------------|---------|
| `-s, --source <DIR>` | Source directory to scan | `.` |
| `-o, --output <DIR>` | Where merged files go; created if missing | `.` |
| `-d, --sdel [<BOOL>]` | Delete the source pair after a successful merge | `true` |
| `-f, --format <FMT>` | Output format: `mkv`, `mp4`, `mov` | `mkv` |
| `-j, --jobs <N>` | Parallel ffmpeg processes, clamped to 1–32 | CPU cores |
| `-p, --progress [<BOOL>]` | Show the progress bar | `true` |
| `-n, --dry-run` | Preview the plan; create, merge and delete nothing | — |
| `-i, --interactive` | Show the plan and wait for confirmation | — |
| `-r, --resume` | Skip pairs a previous interrupted run already completed | — |
| `-R, --recursive` | Scan subdirectories instead of just the top level | — |
| `--once <STEM>` | Merge only this pair, without scanning | — |
| `--retry <N>` | Retry a failed merge N times | `0` |
| `-v, --verbose` | Print the ffmpeg command for each pair, and retry notices | — |
| `-q, --quiet` | No progress bar and no per-pair lines; only the final summary | — |
| `--ascii` | Use ASCII glyphs instead of Unicode box drawing | auto |

`-d/--sdel` and `-p/--progress` take an optional boolean: bare `-d` means
`-d true`.

### Output

The output name is the pair's stem plus the format's extension — `ep01.mp4` and
`ep01.m4a` become `ep01.mkv`. With `-R`, a pair's identity is its path relative
to the source directory, and that identity is flattened into a single file name:
`season1/ep01` becomes `season1_ep01.mkv`. Same-named files in different
directories therefore do not collide.

`mp4` and `mov` are written with `-movflags +faststart`, so the index sits at
the front of the file and playback can start before the download finishes. `mkv`
does not need it.

Because a `.mp4` source and an `mp4`/`mov` output can collide, writing either of
those formats into the source directory is rejected up front rather than
risking the source video: use a different `-o`. Merging to `mkv` in place is
fine, since `.mkv` cannot collide with a source suffix.

A merge is verified after ffmpeg exits: the output must exist and be at least as
large as the source video, since a stream copy carries the video stream plus the
audio stream plus container overhead. Anything smaller is treated as a truncated
merge — reported as a failure, and the partial file is removed. An existing
output that the failed run never touched is left alone.

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Everything merged |
| `1` | General error (bad arguments, unreadable source, …) |
| `2` | ffmpeg is missing |
| `3` | One or more merges failed |

### Resume

An interrupted batch leaves `.mixbilibili_state.json` in the source directory,
recording which pairs completed, which failed and which are still pending. Pass
`--resume` to skip the completed ones and finish the rest. The file is removed
once a batch runs to completion; an interrupted batch keeps it, and keeps both
source files of every unfinished pair, so the resume is real.

Pairs interrupted with Ctrl+C count as *pending*, not failed, so they are
retried on resume rather than being skipped as broken.

## aria2 integration

`scripts/aria2-hook.sh` merges a pair as soon as aria2 finishes downloading it.

1. Copy the hook somewhere aria2 can run it:
   ```bash
   cp scripts/aria2-hook.sh ~/.aria2/aria2-hook.sh
   chmod +x ~/.aria2/aria2-hook.sh
   ```
2. Point aria2 at it in `aria2.conf`:
   ```
   on-download-complete=/home/you/.aria2/aria2-hook.sh
   ```
3. Restart aria2.

On every completed download the hook checks that this file is `.mp4` or `.m4a`,
that its partner exists, and that no `.aria2` control file remains — then runs
`mixbilibili --once <stem> -s <dir> -o <dir> --sdel true --quiet`. Set
`MIXBILIBILI_BIN` if the binary is not on `PATH`.

`--quiet` is what makes this work in a hook: one summary line, no progress bar,
no per-pair output, and ffmpeg's own diagnostics suppressed. The hook only looks
at the exit code.

## Features

- **Automatic pairing** of `.mp4` with `.m4a`, by stem
- **aria2 aware**: in-flight downloads are skipped, not merged or reported as broken
- **Parallel**, with a configurable number of concurrent ffmpeg processes
- **Verified output**: a merge that produced a truncated file is caught and cleaned up
- **Resumable**: interrupted batches pick up where they stopped
- **Retries** for transient failures, with a backoff between attempts
- **Dry run** and **interactive** preview that touch nothing
- **Recursive** scanning that keeps same-named files in different folders distinct
- **Translated** output in Chinese or English, detected from the OS language
  settings (macOS `AppleLanguages`, Windows `GetUserDefaultUILanguage`, Linux
  `LANG`/`LC_ALL`)
- **Progress bar** on a terminal, plain text when the output is piped
- **Unicode or ASCII**, chosen automatically or forced with `--ascii`
- **ffmpeg bootstrap**: offers to install a missing ffmpeg, or explains how

## License

MIT
