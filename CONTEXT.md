# CONTEXT

Domain vocabulary for mixbilibili — a CLI that muxes Bilibili's separately
downloaded `.mp4` video and `.m4a` audio into one container via ffmpeg.

## Domain

- **Pair** — a video+audio file sharing a stem. `scanner` produces pairs, the merge module consumes them.
- **Stem** — filename without extension. The primary key across the whole pipeline: state tracking, progress lines, `--once`.
- **Batch** — the set of pairs one run merges after scan/resume filtering; executed in parallel.
- **Resume state** — `.mixbilibili_state.json` in the source dir; survives an interrupted batch so `--resume` skips what finished.
- **Cancel signal** — a `Cancel` token, set once by the Ctrl+C handler. An interrupted pair stays *pending*: its sources are never deleted and its output is removed.
- **Orphan** — an mp4 or m4a with no partner. Reported, never merged.
- **Downloading** — a pair whose `.aria2` control file still exists. Skipped, not an orphan. (aria2 integration: `scripts/aria2-hook.sh` invokes `--once`.)

## Architecture (refactor shape agreed 2026-09-09)

- **Merge module** (`src/merger.rs`) — the deep module.
  `run_batch(&[FilePair], &MergeOptions, Option<&MergeProgress>, &StateTracker, &Cancel) -> MergeSummary`
  plus the single-pair path `--once` shares. Behind that interface: rayon
  parallelism, retry, timeout, periodic state saves, interrupt cleanup of
  partial outputs, source deletion (`--sdel`), summary aggregation.
- **MergeOptions** — `{ output_dir, format, retry, verbose }`: the merge module's whole input contract. `Args` (cli) is not split further.
- **StateTracker** (`src/state.rs`) — thread-safe resume-state module: owns the file path, throttled atomic saves (temp+rename), mark/clear policy. Callers never see HashSets or save intervals.
- **Cancel** (`src/merger.rs`) — newtype over `Arc<AtomicBool>`; main's ctrl-c handler holds the handle, tests flip it directly. Replaces the `INTERRUPTED` static.
- **Display is outside the merge module** — progress recording and report printing read `MergeResult`/`MergeSummary` facts; `render_report` is a private fn in `main.rs`. The merge module never prints.

## Invariants

- Interrupted ⇒ sources untouched, partial output deleted, stem stays pending.
- `--dry-run` previews in *both* modes (batch and `--once`); it never merges or deletes.
- Resume state writes are atomic; a torn file must never be left behind.
- aria2 hook contract: flags and exit codes are frozen; stdout prose is not.
