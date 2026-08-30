//! Merge policy, driven through the library interface.
//!
//! These tests never touch ffmpeg: the seam at the process boundary has a
//! second adapter (`Scripted`) that replays canned outcomes per pair, so
//! retry, timeout, output validation, partial cleanup, source deletion and the
//! resume state file are all exercised in milliseconds.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use mixbilibili::cli::OutputFormat;
use mixbilibili::ffmpeg::{MergeRunner, RunOutcome};
use mixbilibili::i18n::{t, tf};
use mixbilibili::interrupt::Interrupt;
use mixbilibili::merger::{self, MergePlan};
use mixbilibili::pipeline::{self, BatchOpts, StateSink};
use mixbilibili::scanner::{FilePair, ScanStats};
use mixbilibili::state::MergeState;
use tempfile::{tempdir, TempDir};

/// Stands in for ffmpeg. Each pair gets its own script, keyed by the stem in
/// the output path, so parallel batches stay deterministic. On `Success` it
/// writes the output file the real command would have left behind.
struct Scripted {
    scripts: Mutex<HashMap<String, Vec<RunOutcome>>>,
    calls: AtomicUsize,
    payload: &'static [u8],
}

impl Scripted {
    fn new(plan: Vec<(&str, Vec<RunOutcome>)>) -> Self {
        Self {
            scripts: Mutex::new(
                plan.into_iter()
                    .map(|(stem, outcomes)| (stem.to_string(), outcomes))
                    .collect(),
            ),
            calls: AtomicUsize::new(0),
            payload: b"merged stream",
        }
    }

    /// Writes a zero-byte output, so validation sees what ffmpeg left after
    /// claiming success.
    fn leaves_empty(mut self) -> Self {
        self.payload = b"";
        self
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl MergeRunner for Scripted {
    fn run(&self, cmd: &mut Command, interrupt: &Interrupt) -> io::Result<RunOutcome> {
        self.calls.fetch_add(1, Ordering::Relaxed);

        let output = PathBuf::from(
            cmd.get_args()
                .last()
                .expect("the ffmpeg command ends with the output path"),
        );
        let stem = output
            .file_stem()
            .expect("output path has a stem")
            .to_string_lossy()
            .into_owned();

        if interrupt.is_triggered() {
            // A real adapter kills the child; either way the caller sees a
            // cancelled run.
            return Ok(RunOutcome::Cancelled);
        }

        let outcome = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&stem)
            .and_then(|script| {
                (!script.is_empty()).then(|| {
                    let outcome = script[0].clone();
                    script.remove(0);
                    outcome
                })
            })
            .unwrap_or_else(|| panic!("no scripted outcome left for {stem}"));

        if matches!(outcome, RunOutcome::Success) {
            fs::write(&output, self.payload).unwrap();
        }

        Ok(outcome)
    }
}

const SUCCESS: RunOutcome = RunOutcome::Success;
const TIMEOUT: RunOutcome = RunOutcome::TimedOut;

fn failed(code: i32) -> RunOutcome {
    RunOutcome::Failed {
        code: Some(code),
        message: String::new(),
    }
}

fn failed_with(code: i32, stderr: &str) -> RunOutcome {
    RunOutcome::Failed {
        code: Some(code),
        message: stderr.to_string(),
    }
}

/// A pair whose sources exist in `dir`.
fn pair(dir: &Path, stem: &str) -> FilePair {
    let pair = FilePair {
        video: FilePair::video_path(dir, stem),
        audio: FilePair::audio_path(dir, stem),
        stem: stem.to_string(),
    };
    fs::write(&pair.video, b"video").unwrap();
    fs::write(&pair.audio, b"audio").unwrap();
    pair
}

fn opts<'a>(dir: &'a Path, interrupt: &'a Interrupt) -> BatchOpts<'a> {
    BatchOpts {
        output_dir: dir,
        format: OutputFormat::Mkv,
        sdel: false,
        retries: 0,
        verbose: false,
        quiet: false,
        progress: None,
        state: None,
        interrupt,
    }
}

fn merge(
    dir: &Path,
    stem: &str,
    runner: &dyn MergeRunner,
    retries: usize,
    interrupt: &Interrupt,
) -> merger::MergeResult {
    let plan = MergePlan {
        output_dir: dir,
        format: OutputFormat::Mkv,
        retries,
        verbose: false,
        quiet: false,
        progress: None,
        interrupt,
    };
    merger::merge_pair(&pair(dir, stem), 0, &plan, runner)
}

#[test]
fn a_successful_merge_writes_the_output_named_after_the_stem() {
    let dir = tempdir().unwrap();
    let result = merge(
        dir.path(),
        "ep01",
        &Scripted::new(vec![("ep01", vec![SUCCESS])]),
        0,
        &Interrupt::new(),
    );

    assert!(result.success, "{:?}", result.error);
    assert_eq!(result.output_path, dir.path().join("ep01.mkv"));
    assert!(result.output_path.exists());
}

#[test]
fn a_failing_merge_is_retried_until_the_budget_runs_out() {
    let dir = tempdir().unwrap();
    let runner = Scripted::new(vec![("ep01", vec![failed(1), failed(1), SUCCESS])]);

    let result = merge(dir.path(), "ep01", &runner, 2, &Interrupt::new());

    assert!(result.success, "the third attempt should have worked");
    assert_eq!(runner.calls(), 3, "first attempt plus two retries");
}

#[test]
fn retries_are_reported_when_they_do_not_help() {
    let dir = tempdir().unwrap();
    let runner = Scripted::new(vec![("ep01", vec![failed(1), failed(1)])]);

    // Same locale on both sides, so the assertion holds wherever it runs.
    let result = merge(dir.path(), "ep01", &runner, 1, &Interrupt::new());
    assert_eq!(result.error.unwrap(), tf("merge_failed_exit", &["1", "1"]));
}

#[test]
fn what_ffmpeg_said_reaches_the_report_line() {
    let dir = tempdir().unwrap();
    let runner = Scripted::new(vec![(
        "ep01",
        vec![failed_with(183, "moov atom not found\n")],
    )]);

    let result = merge(dir.path(), "ep01", &runner, 0, &Interrupt::new());

    let err = result.error.unwrap();
    assert!(
        err.contains("moov atom not found"),
        "the reason must survive: {err}"
    );
    assert!(
        err.contains('1'),
        "the retry budget is still reported: {err}"
    );
}

#[test]
fn a_timeout_is_retried_and_then_reported_as_such() {
    let dir = tempdir().unwrap();
    let runner = Scripted::new(vec![("ep01", vec![TIMEOUT, TIMEOUT])]);

    let result = merge(dir.path(), "ep01", &runner, 1, &Interrupt::new());

    assert_eq!(runner.calls(), 2, "a timeout is retryable");
    assert_eq!(result.error.unwrap(), t("timed_out").into_owned());
}

#[test]
fn a_zero_byte_output_is_not_a_success() {
    let dir = tempdir().unwrap();
    let runner = Scripted::new(vec![("ep01", vec![SUCCESS])]).leaves_empty();

    let result = merge(dir.path(), "ep01", &runner, 0, &Interrupt::new());

    assert!(!result.success, "ffmpeg lied; the file decides");
    assert!(result.error.unwrap().contains("ep01.mkv"));
    assert!(
        !dir.path().join("ep01.mkv").exists(),
        "empty output removed"
    );
}

#[test]
fn a_spawn_failure_is_retried_then_reported() {
    let dir = tempdir().unwrap();
    struct Broken;
    impl MergeRunner for Broken {
        fn run(&self, _cmd: &mut Command, _interrupt: &Interrupt) -> io::Result<RunOutcome> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no ffmpeg"))
        }
    }

    let result = merge(dir.path(), "ep01", &Broken, 1, &Interrupt::new());

    assert!(!result.success);
    assert!(result.error.unwrap().contains("no ffmpeg"));
}

#[test]
fn an_interrupted_run_never_starts_a_merge() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    interrupt.trigger();
    let runner = Scripted::new(vec![]);

    let result = merge(dir.path(), "ep01", &runner, 0, &interrupt);

    assert!(result.was_interrupted);
    assert!(!result.success);
    assert_eq!(runner.calls(), 0, "nothing should be spawned");
}

#[test]
fn a_failed_merge_leaves_no_partial_output_behind() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    let pairs = vec![pair(dir.path(), "ep01")];
    fs::write(dir.path().join("ep01.mkv"), b"half a file").unwrap();

    let summary = pipeline::process(
        &pairs,
        &ScanStats::default(),
        &opts(dir.path(), &interrupt),
        &Scripted::new(vec![(
            "ep01",
            vec![RunOutcome::Failed {
                code: None,
                message: String::new(),
            }],
        )]),
    )
    .unwrap();

    assert_eq!(summary.failed_count(), 1);
    assert!(
        !dir.path().join("ep01.mkv").exists(),
        "the partial file must not survive the batch"
    );
}

#[test]
fn sources_are_deleted_only_for_pairs_that_merged() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    let pairs = vec![pair(dir.path(), "good"), pair(dir.path(), "bad")];
    let mut opts = opts(dir.path(), &interrupt);
    opts.sdel = true;

    let summary = pipeline::process(
        &pairs,
        &ScanStats::default(),
        &opts,
        &Scripted::new(vec![("good", vec![SUCCESS]), ("bad", vec![failed(2)])]),
    )
    .unwrap();

    assert_eq!(summary.success_count(), 1);
    assert_eq!(summary.failed_count(), 1);
    assert!(!FilePair::video_path(dir.path(), "good").exists());
    assert!(!FilePair::audio_path(dir.path(), "good").exists());
    assert!(
        FilePair::video_path(dir.path(), "bad").exists(),
        "a failed pair keeps its sources so it can be retried"
    );
    assert!(FilePair::audio_path(dir.path(), "bad").exists());
}

#[test]
fn the_state_file_records_what_a_resume_needs() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    let pairs = vec![pair(dir.path(), "ok"), pair(dir.path(), "bad")];
    let mut state = MergeState::new(dir.path(), dir.path());
    state.add_pending("ok");
    state.add_pending("bad");
    let sink = StateSink::new(state, dir.path().to_path_buf());
    let mut opts = opts(dir.path(), &interrupt);
    opts.state = Some(&sink);

    let summary = pipeline::process(
        &pairs,
        &ScanStats::default(),
        &opts,
        &Scripted::new(vec![("ok", vec![SUCCESS]), ("bad", vec![failed(1)])]),
    )
    .unwrap();

    assert!(!summary.all_success());
    let loaded = MergeState::load(dir.path()).unwrap().expect("state saved");
    assert!(loaded.is_completed("ok"));
    assert!(loaded.is_failed("bad"));
    assert!(!loaded.is_pending("ok"));
    assert!(!loaded.is_pending("bad"));
}

#[test]
fn the_state_file_disappears_when_everything_merged() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    let pairs = vec![pair(dir.path(), "a"), pair(dir.path(), "b")];
    let mut state = MergeState::new(dir.path(), dir.path());
    state.add_pending("a");
    state.add_pending("b");
    let sink = StateSink::new(state, dir.path().to_path_buf());
    let mut opts = opts(dir.path(), &interrupt);
    opts.state = Some(&sink);

    pipeline::process(
        &pairs,
        &ScanStats::default(),
        &opts,
        &Scripted::new(vec![("a", vec![SUCCESS]), ("b", vec![SUCCESS])]),
    )
    .unwrap();

    assert!(
        MergeState::load(dir.path()).unwrap().is_none(),
        "a clean batch leaves nothing to resume"
    );
}

#[test]
fn an_interrupted_batch_keeps_its_pairs_resumable() {
    let dir = tempdir().unwrap();
    let interrupt = Interrupt::new();
    interrupt.trigger();
    let pairs = vec![pair(dir.path(), "a"), pair(dir.path(), "b")];
    let mut state = MergeState::new(dir.path(), dir.path());
    state.add_pending("a");
    state.add_pending("b");
    let sink = StateSink::new(state, dir.path().to_path_buf());
    let mut opts = opts(dir.path(), &interrupt);
    opts.state = Some(&sink);

    let summary = pipeline::process(&pairs, &ScanStats::default(), &opts, &Scripted::new(vec![]))
        .expect("an interrupt is not an error");

    assert!(summary.all_success(), "an interrupt is not a merge failure");
    assert_eq!(summary.success_count(), 0);
    let loaded = MergeState::load(dir.path())
        .unwrap()
        .expect("the resume state survives the interrupt");
    assert!(loaded.is_pending("a"), "both pairs stay pending");
    assert!(loaded.is_pending("b"));
    assert!(
        FilePair::video_path(dir.path(), "a").exists(),
        "sources of an unfinished pair are never deleted"
    );
}

#[test]
fn preview_and_deletion_agree_with_what_the_batch_does() {
    let dir: TempDir = tempdir().unwrap();
    let pairs = vec![pair(dir.path(), "ep01")];

    let rows = pipeline::preview_rows(&pairs, OutputFormat::Mkv);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].output, "ep01.mkv");
    assert!(rows[0].video.ends_with("ep01.mp4"));
    assert!(rows[0].audio.ends_with("ep01.m4a"));
}
