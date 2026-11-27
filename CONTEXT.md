# mixbilibili — domain glossary

The names below are the ones to use when reading or changing this codebase.
They name the seams; the modules behind them are in `src/`.

**Pair** — one Bilibili download: `X.mp4` (video) + `X.m4a` (audio). Owned by
`FilePair` (`scanner`), which owns the whole naming convention: the suffixes,
the aria2 control-file suffix, where merged output belongs
(`FilePair::output_path`), and how a composite key flattens into one file name.
Nothing else formats a filename.

**Stem** — a pair's identity. Bare (`ep01`) in a flat scan; a composite
`subdir/ep01` when `--recursive`, so same-named files in different directories
stay distinct. As a `--once` argument it must never carry a path separator or a
null byte; `Args::validate` enforces that at the CLI boundary.

**Batch** — the set of pairs one run will merge. `--once <stem>` is a batch of
one, not a separate code path.

**Pipeline** (`pipeline::process`) — the whole life cycle of a batch: merge
each pair, record the outcome, save resume state every few pairs, remove
partial outputs, delete sources when `--sdel`, and build the report. One place
decides what happens to a pair after ffmpeg returns.

**Merge runner** (`ffmpeg::MergeRunner`) — the seam at the ffmpeg process
boundary. `FfmpegRunner` is the production adapter; the tests use a scripted
one. Merge *policy* (retry, timeout, validate the output) lives above the seam
in `merger`; spawning, stderr capture and killing live behind it.

**Interrupt token** (`interrupt::Interrupt`) — cooperative cancellation. The
Ctrl+C handler is one adapter that triggers it; the pipeline and `merge_pair`
observe it. A cancelled pair stays *pending* and keeps both sources, so
`--resume` can finish it.

**Skipped** — a pair with a live `.aria2` control file; aria2 is still
downloading it. Not an error, not a candidate.

**Orphan** — a `.mp4` or `.m4a` with no partner. Reported, never touched.

**Resume state** (`state::MergeState`, written through `pipeline::StateSink`) —
`.mixbilibili_state.json` in the source directory: completed / failed / pending
stems, plus a schema version. Cleared only when a batch finishes without being
cancelled.

**Report** (`merger::MergeSummary`) — built from the batch's results by
`from_results`. Its counting rules (an interrupted pair is neither success nor
failure) belong to it, not to the caller.

**Output slot** — the file a pair will be written to, `FilePair::output_path`.
The batch snapshots every slot before it starts (`pipeline::OutputSnapshot`) and
only removes a slot that this run created or overwrote: a failure that never
touched an earlier, finished merge must not delete it.

**Preflight** (`lib::require_ffmpeg`) — a read-only "is ffmpeg there" check that
runs before the scan. It is separate from `prepare`, which may *install* ffmpeg
and creates the output directory. The split exists so that a run which cannot
merge fails before `scan_and_filter` writes the resume state, and so that a dry
run (`--dry-run`) or a declined `--interactive` prompt never touches the disk.

**Quiet** (`-q`) — no progress bar, no per-pair lines, and no ffmpeg
diagnostics, only the final summary. It is part of the merge plan, not of the
progress bar; this is what `scripts/aria2-hook.sh` relies on.

**Symbols / Colors** (`ui`) — how output looks. Everything else says what
happened; `ui` decides which glyphs and colours say it, honouring `--ascii`.

## Decisions not to re-litigate

- `docs/adr/0001` — ffmpeg runs behind an injected `MergeRunner`; merge
  behaviour is tested through that seam, never by spawning a process, and
  ffmpeg's stderr is captured rather than inherited.
- `docs/adr/0002` — `--once` is a batch of one pair through the shared
  pipeline; mode differences are `BatchOpts` values, not a second entry point.
- **Versions move in the third place only.** From `0.10.0` onward every release
  is a patch bump (`0.10.1`, `0.10.2`, …). The minor number stays at 10 and the
  major at 0, whatever the change is; do not argue the SemVer case for a minor
  or major bump. The second place moves only if that is explicitly asked for.
