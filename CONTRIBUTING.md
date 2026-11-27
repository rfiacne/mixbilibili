# Contributing to mixbilibili

Thank you for your interest in contributing!

## Development setup

1. Install Rust 1.88+ via [rustup](https://rustup.rs/)
2. Install ffmpeg — the integration tests skip themselves without it, but the
   merge paths then go untested
3. `cargo test` to verify your setup

## Code style

- `cargo fmt` before committing
- `cargo clippy --all-targets -- -D warnings` and fix everything
- `deslop-please` for the architectural lints (see below)
- Add tests for new behaviour

## Pull requests

1. Branch from `master`
2. Keep commits focused, with messages that say *why*
3. Make sure CI is green — it runs on Linux and Windows

## Checks

CI runs these; run them locally before pushing.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
deslop-please
```

`cargo test` covers three suites: 250 unit tests, the CLI integration tests
(which spawn the real binary, and skip ffmpeg-dependent cases when ffmpeg is not
installed), and `tests/merge_policy.rs` (merge policy through the `MergeRunner`
seam, no ffmpeg needed).

Narrow to a module while iterating:

```bash
cargo test scanner
cargo test merger
cargo test --test merge_policy
cargo test --test integration_test
```

### deslop-please

`deslop-please` reports architectural problems the compiler does not: module
cycles, tangled interfaces, fields known outside their type, shallow modules,
wide directories, long files. CI installs a pinned release binary and runs it;
installing locally is the same binary:

```bash
cargo install deslop-please   # or grab a prebuilt release
deslop-please                 # whole workspace
deslop-please mixbilibili     # one crate
```

It needs the `rust-analyzer` component installed (`rustup component add
rust-analyzer`).

## Conventions

`AGENTS.md` documents the module boundaries and the constraints that must hold —
no re-encoding, symlinks skipped while scanning, exit codes stable, translations
for every user-visible string. `CONTEXT.md` is the glossary for the domain
names (`Pair`, `Stem`, `Batch`, `Pipeline`, `Resume state`). Read both before
changing `merger`, `state` or `pipeline`.

The `docs/adr/` decisions are settled; open an issue before proposing to
revisit one.

## Releasing

1. Bump `version` in `Cargo.toml` (this also updates `Cargo.lock`)
2. Move the `CHANGELOG.md` `[Unreleased]` entries under the new version heading
   with today's date. The release workflow extracts its notes from that section
   by matching `## [<version>]`, so the version number in the heading must be
   bare — `## [0.10.0] - 2026-11-27`, not `## 0.10.0` or `## [v0.10.0]`
3. Open a PR, merge it, then tag the merge commit:
   ```bash
   git tag -a v0.10.0 -m "mixbilibili 0.10.0"
   git push origin v0.10.0
   ```
4. The tag runs the full pipeline: tests on Linux and Windows, the MSRV build,
   the release-profile build, then the cross-target matrix
   (linux x86_64 / linux aarch64 / windows msvc) and the GitHub release

Only tags build and publish; a branch push stops after the tests and the
release-profile check.
