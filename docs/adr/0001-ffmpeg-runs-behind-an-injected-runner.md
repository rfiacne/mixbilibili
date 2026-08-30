# ffmpeg runs behind an injected runner

Date: 2026-08-30
Status: accepted

## Context

Merge policy — retry, timeout, "ffmpeg exited 0 but the file is empty",
partial-output cleanup — is where almost every bug this project has fixed
lived. `do_merge` built the `Command` and spawned it in the same function, so
exercising that policy meant installing ffmpeg and waiting out a 300-second
timeout. Two things could not be fixed at all while the child owned its own
stderr: `-q/--quiet` leaked ffmpeg's diagnostics, and a cancelled merge was
indistinguishable from a timed-out one, so Ctrl+C left the pair recorded as a
failure in the resume state.

## Decision

`merger` takes the process boundary as a parameter: `ffmpeg::MergeRunner` is a
seam with two adapters — `FfmpegRunner` in production, a scripted
outcome-replayer in `tests/merge_policy.rs`. Outcomes cross the seam as
`RunOutcome` (`Success` / `Failed { code, message }` / `TimedOut` /
`Cancelled`), not `std::process::ExitStatus`, which tests cannot construct.

The adapter also owns the poll loop, kills the child on timeout or cancel, and
pipes the child's stderr into a bounded read rather than letting it inherit the
terminal.

## Consequences

- Merge policy is covered by `tests/merge_policy.rs` in milliseconds with no
  ffmpeg installed. Test merge behaviour through this seam; never spawn a
  process or match on `ExitStatus` above `FfmpegRunner`.
- `RunOutcome` must stay a plain data enum: adding a variant forces a decision
  about retryability in exactly one `match`. `Cancelled` and `TimedOut` look
  alike to the process — both mean "still running" — but must not be merged,
  because only a cancel leaves the pair pending.
- Quiet mode is real, and the progress bar is never interleaved with ffmpeg
  output. The price is that ffmpeg no longer streams live diagnostics; the
  report carries the last line of what it wrote, and `--verbose` all of it.
- The reader hands the buffer back over a channel with a receive timeout
  instead of a join: a killed child can leave the pipe's write end open in a
  grandchild, and joining on it hangs the batch instead of failing it.
- SIGCHLD must still be `SIG_DFL` before any thread starts (`run()` in
  `src/lib.rs`), because aria2's hook raises with `SIG_IGN` and `waitpid`
  would fail with `ECHILD`. Do not move that into the adapter: it is a
  process-level invariant, and the adapter runs from worker threads.
