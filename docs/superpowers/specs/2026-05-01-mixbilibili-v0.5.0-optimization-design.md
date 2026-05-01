# mixbilibili v0.5.0 Optimization Design

**Date:** 2026-05-01  
**Status:** Draft  
**Scope:** Code quality, UX improvements, CI/CD enhancements

## Overview

Three-layer design, each layer independent and verifiable:

1. **Code Quality** — fix production `unwrap()`/`expect()`, extract `wait_timeout`, pure type-driven exit codes
2. **User Experience** — `--quiet`, `--dry-run`, structured report
3. **CI/CD** — clippy lint gate, test in PR, release changelog, binary size report

---

## Layer 1: Code Quality

### 1.1 Production `unwrap()` / `expect()` audit

Only production code is changed. Test `unwrap()` is preserved (Rust idiom: tests use `unwrap()` to signal "if this fails it's a logic error").

| File | Location | Change |
|------|----------|--------|
| `merger.rs:349` | `rayon::ThreadPoolBuilder::new().build().expect(...)` | Return `Result` from caller, bubble error up through `run_merge` |
| `src/ffmpeg.rs` | Check for any production `unwrap()` | Replace with `?` or `.context(...)` |

The `rayon` thread pool creation is the only production `expect()` found. Fix: change `run_merge` to return `Result<()>` and propagate the pool build error.

### 1.2 Extract `wait_with_timeout`

`merger.rs` has inline `Child::wait_timeout` polling logic. Extract as `trait ChildExt`:

```rust
trait ChildExt {
    fn wait_with_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>>;
}
```

Benefits:
- Reduces nesting in `merge_pair`
- Testable independently
- Clearer separation of concern (timeout logic vs merge logic)

### 1.3 Pure type-driven exit codes

`main.rs` `get_exit_code` uses string matching as fallback:
```rust
if msg.contains("ffmpeg") { ... }
if msg.contains("merge") { ... }
```

Remove the fallback entirely. All errors go through `AppError` variants which have `.exit_code()`. Any `anyhow::Error` that is not an `AppError` returns `GENERAL_ERROR`. This eliminates fragile string matching.

---

## Layer 2: User Experience

### 2.1 `--quiet` / `-q` flag

**cli.rs:**
```rust
#[arg(long, short = 'q', default_value = "false")]
pub quiet: bool,
```

Behavior:
- No `MergeProgress` created when `quiet` is true
- Per-pair output suppressed (no `✓ video`, `✗ video` lines)
- Final report: single line `3/5 merged, 2 failed` with exit code-based coloring
- Errors still printed (quiet ≠ silent)

### 2.2 `--dry-run` / `-n` flag (already exists, enhance)

The `--dry-run` flag exists in `cli.rs` and is wired through to `merger.rs`. Enhance it:
- Print list of pairs that would be merged (with file paths)
- Print list of source files that would be deleted (if `--sdel true`)
- In dry-run mode, `execute_merges` should skip ffmpeg and return simulated results

### 2.3 Structured final report

Replace current `println!` based report with formatted output:

```
Merge Report:
  ✓ 12 succeeded    ✗ 3 failed
  Duration: 2m 34s
  Throughput: 0.13 pairs/sec
```

In quiet mode, single line:
```
12/15 merged, 3 failed
```

Implementation: new `print_report` function in `merger.rs` that takes `MergeSummary`, `Duration`, and `quiet: bool`.

---

## Layer 3: CI/CD

### 3.1 PR check workflow

New or existing workflow, triggered on PR:

```yaml
- name: Clippy
  run: cargo clippy -- -D warnings

- name: Test
  run: cargo test
```

Only runs when Rust files change (`paths: ["src/**", "tests/**", "Cargo.toml", "Cargo.lock"]`).

### 3.2 Release workflow enhancement

**Changelog extraction:**
```bash
awk '/^## \[0.5.0\]/{p=1;next}/^## \[/{p=0}p' CHANGELOG.md
```
Pass extracted text to `gh release create --notes`.

**Test before release:**
Add `cargo test` step before build.

### 3.3 Binary size report

After `cargo build --release`:
```bash
echo "Binary size: $(du -h target/release/mixbilibili | cut -f1)"
```
Output to `$GITHUB_STEP_SUMMARY`.

---

## File Changes Summary

| File | Change |
|------|--------|
| `src/cli.rs` | Add `quiet`, `dry_run` flags |
| `src/main.rs` | Wire up flags, remove string-matching exit codes |
| `src/merger.rs` | Extract `wait_with_timeout`, fix `expect()`, structured report, dry-run support |
| `src/progress.rs` | No changes |
| `src/scanner.rs` | No changes |
| `src/state.rs` | No changes |
| `src/ffmpeg.rs` | Audit for `unwrap()` |
| `.github/workflows/ci.yml` | New PR check workflow |
| `.github/workflows/release.yml` | Add changelog, test, size report |

## Risks

- **`wait_with_timeout` extraction** — cross-platform process handling is tricky. Keep existing polling interval (`500ms`) and timeout (`300s`) exactly as-is.
- **`--quiet` flag** — must not suppress errors, only progress output.
- **Exit code removal** — ensure all error paths still return correct exit code. Integration tests cover this.
