# --once is a batch of one pair

Date: 2026-08-30
Status: accepted

## Context

`--once <stem>` was implemented as a second path through the program: it built
the output file name itself, deleted sources itself, cleaned partial output
itself, and skipped the state file. Within a release it had drifted from the
batch path — it cleaned partial files on failure while the batch cleaned them
only on interrupt, neither agreed about what to do with the sources of an
unfinished pair, and the preview could show a file name the run never wrote.

## Decision

There is one pipeline. `pipeline::process(pairs, stats, opts, runner)` owns the
whole life cycle of a batch, and `--once` calls it with a single pair and
`BatchOpts { progress: None, state: None, .. }`. Mode differences are option
values, not separate code.

## Consequences

- New batch behaviour applies to `--once` and vice versa. A feature that only
  makes sense for one mode belongs in `BatchOpts`, not in a new entry point.
- Three deliberate differences remain, all at the interface: `--once` writes no
  resume state, it does not start a rayon pool (`process` runs a batch of one
  inline), and it does not print the `--resume` hint after an interrupt — there
  is no resume point for it to point at.
- `--once --quiet` prints the one-line summary and nothing else: no progress, no
  per-pair lines, no ffmpeg diagnostics. `scripts/aria2-hook.sh` depends on that
  and only on the exit code, which it does not parse further; the contract is
  pinned by `test_once_quiet_contract_used_by_the_aria2_hook`.
