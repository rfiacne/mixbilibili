# mixbilibili Progress Bar Redesign

**Date:** 2026-05-02
**Status:** Draft
**Scope:** Replace thin `MergeProgress` wrapper with a proper progress system

## Overview

Restructure `src/progress.rs` from a thin `Arc<ProgressBar>` wrapper into a facade that picks the right rendering strategy at construction time. Adds per-file status messages, speed/ETA, TTY fallback, and a cleaner `record()` API.

## Architecture

`MergeProgress` becomes a facade with an internal `Renderer` enum:

```rust
enum Renderer {
    Bar(Arc<ProgressBar>),
    Text { total: usize, completed: usize },
}

pub struct MergeProgress {
    inner: Renderer,
}
```

Both variants expose the same public API: `new()`, `record()`, `inc()`, `finish()`.

### TTY Detection

Use `console::user_attended()` (transitive dep of indicatif) at construction time:
- TTY detected → `ProgressBar` with full template
- No TTY → `Text` renderer (prints one line per file)

## Visual Design

### Progress Bar Mode

Template: `[ETA] {bar} {pos}/{len} ({per_sec}) {msg}`

```
[00:00:12] [████████████░░░░░░░░░░░░░░░░░░░] 5/12 (2.3 files/s) ✓ video_005
```

- Bar: 30 chars, cyan/blue, `=>-` progress chars
- Speed: files/sec, 1 decimal
- Message: `✓ stem`, `✗ stem: error`, `↻ stem (retry 2)`
- Elapsed + ETA via indicatif built-ins

### Text Mode

One line per file, printed immediately:

```
✓ video_001 (1.2s)
✓ video_002 (0.8s)
✗ video_003: ffmpeg exited with code 1
↻ video_004 retry 2 (3.1s)
```

In `--quiet` mode: `MergeProgress` is `None` — nothing printed.

## API

```rust
impl MergeProgress {
    pub fn new(total: usize) -> Self;
    pub fn record(&self, stem: &str, success: bool, duration: Duration,
                  error: Option<&str>, retry: Option<usize>);
    pub fn inc(&self);
    pub fn finish(&self);
}
```

`record()` replaces the pattern of `set_message()` + `inc()` + separate `println!`.

## File Changes

| File | Change |
|------|--------|
| `src/progress.rs` | Rewrite with internal `Renderer` enum |
| `src/merger.rs` | Replace `p.inc()` + `println!` with `p.record()` |
| `Cargo.toml` | No changes |

## Risks

- **indicatif version**: Current version may not support all template features. Verify before implementing.
- **Thread safety**: `Text` renderer uses `Mutex` for `completed` counter. `ProgressBar` is already thread-safe via `Arc`.
- **TTY detection**: `console::user_attended()` may not exist in all indicatif versions. Fallback: assume TTY.
