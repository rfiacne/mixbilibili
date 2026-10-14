use crate::cli::Args;
use crate::cli::OutputFormat;
use crate::ffmpeg::MergeRunner;
use crate::i18n::{t, tf};
use crate::interrupt::Interrupt;
use crate::merger::{self, MergePlan, MergeResult, MergeSummary};
use crate::progress::MergeProgress;
use crate::scanner::{FilePair, ScanStats};
use crate::state::MergeState;
use anyhow::Result;
use colored::Colorize;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Merges between incremental state saves, so an interrupt loses little work.
pub const STATE_SAVE_INTERVAL: usize = 5;

/// Resume bookkeeping for one batch: record outcomes, save every N pairs, and
/// settle the state file at the end.
///
/// `--once` runs without one; nothing here knows which mode called.
pub struct StateSink {
    state: Mutex<MergeState>,
    source: PathBuf,
    save_every: usize,
    settled: AtomicUsize,
}

impl StateSink {
    pub fn new(state: MergeState, source: PathBuf) -> Self {
        Self {
            state: Mutex::new(state),
            source,
            save_every: STATE_SAVE_INTERVAL,
            settled: AtomicUsize::new(0),
        }
    }

    /// Record one outcome, saving a snapshot every few pairs. An interrupted
    /// pair is left pending so `--resume` retries it.
    fn record(&self, result: &MergeResult) {
        if result.was_interrupted {
            return;
        }
        if let Some(snapshot) = self.mark(result) {
            if let Err(e) = snapshot.save(&self.source) {
                eprintln!("{}", tf("failed_save_state", &[&e.to_string()]));
            }
        }
    }

    /// Mark the outcome and hand back a snapshot to persist. The lock is
    /// released before the caller touches the disk, so a slow write never
    /// blocks the other workers.
    fn mark(&self, result: &MergeResult) -> Option<MergeState> {
        // A panicking worker leaves the data intact; recover it.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if result.success {
            state.mark_completed(&result.pair_name);
        } else {
            state.mark_failed(&result.pair_name);
        }

        let settled = self.settled.fetch_add(1, Ordering::Relaxed) + 1;
        (settled.is_multiple_of(self.save_every)).then(|| state.clone())
    }

    /// Drop the state file once the batch is done, or keep it for `--resume`.
    fn finish(&self, completed: bool) -> Result<()> {
        if completed {
            MergeState::clear(&self.source)
        } else {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.save(&self.source)
        }
    }
}

/// How a batch should run. `--once` is a batch of one with `progress` and
/// `state` both absent.
pub struct BatchOpts<'a> {
    pub output_dir: &'a Path,
    pub format: OutputFormat,
    pub sdel: bool,
    pub retries: usize,
    pub verbose: bool,
    pub quiet: bool,
    pub progress: Option<&'a MergeProgress>,
    pub state: Option<&'a StateSink>,
    pub interrupt: &'a Interrupt,
}

impl<'a> BatchOpts<'a> {
    /// The flags every run passes down, with the batch-only options off.
    pub fn from_args(args: &'a Args, interrupt: &'a Interrupt) -> Self {
        Self {
            output_dir: &args.output,
            format: args.format,
            sdel: args.sdel,
            retries: args.retry,
            verbose: args.verbose,
            quiet: args.quiet,
            progress: None,
            state: None,
            interrupt,
        }
    }
}

/// Merge every pair, then clean up, delete sources and report.
///
/// This is the whole life cycle of a batch — the single place that decides
/// what happens to a pair after ffmpeg returns.
pub fn process(
    pairs: &[FilePair],
    stats: &ScanStats,
    opts: &BatchOpts,
    runner: &dyn MergeRunner,
) -> Result<MergeSummary> {
    let plan = MergePlan {
        output_dir: opts.output_dir,
        format: opts.format,
        retries: opts.retries,
        verbose: opts.verbose,
        quiet: opts.quiet,
        progress: opts.progress,
        interrupt: opts.interrupt,
    };

    let merge_one = |pair_index: usize, pair: &FilePair| {
        let result = merger::merge_pair(pair, pair_index, &plan, runner);
        if let Some(sink) = opts.state {
            sink.record(&result);
        }
        result
    };

    // A single pair (the --once path) must not drag in a thread pool.
    let results: Vec<MergeResult> = if pairs.len() < 2 {
        pairs
            .iter()
            .enumerate()
            .map(|(i, p)| merge_one(i, p))
            .collect()
    } else {
        pairs
            .par_iter()
            .enumerate()
            .map(|(i, p)| merge_one(i, p))
            .collect()
    };

    if let Some(progress) = opts.progress {
        progress.finish();
    }

    // Anything that did not finish leaves a partial file behind; its sources
    // are never deleted, so it can be resumed.
    for result in &results {
        if result.success {
            continue;
        }
        match merger::remove_partial(&result.output_path) {
            Ok(true) if opts.verbose => {
                eprintln!(
                    "{}",
                    tf(
                        "cleanup_partial",
                        &[&result.output_path.display().to_string()]
                    )
                    .yellow()
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("{} {}", t("warning_prefix").yellow(), e),
        }
    }

    let mut deletion_failures = 0;
    if opts.sdel {
        for result in &results {
            if !result.success {
                continue;
            }
            if let Err(e) = merger::delete_source_files(&pairs[result.pair_index]) {
                eprintln!("{} {}", t("warning_prefix").yellow(), e);
                deletion_failures += 1;
            }
        }
    }

    let summary = MergeSummary::from_results(&results, stats, deletion_failures);

    if let Some(sink) = opts.state {
        // Interrupted pairs are not failures, but the batch did not finish:
        // clearing the state then would delete what --resume reads.
        let completed = summary.all_success() && !opts.interrupt.is_triggered();
        sink.finish(completed)?;
    }

    Ok(summary)
}

/// One row of the preview: what will be merged, and where it lands.
#[derive(Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub output: String,
}

/// The plan a batch will follow, as data. Dry-run and interactive confirmation
/// render this; the run itself is driven by the same pairs. `output` is the
/// file name the run writes — for a recursive scan, the flattened composite
/// key, not a path under the output directory.
pub fn preview_rows(pairs: &[FilePair], format: OutputFormat) -> Vec<PreviewRow> {
    pairs
        .iter()
        .map(|pair| PreviewRow {
            video: pair.video.clone(),
            audio: pair.audio.clone(),
            output: pair.output_name(format),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_saves_every_few_pairs_not_every_one() {
        const _: () = assert!(
            STATE_SAVE_INTERVAL > 1 && STATE_SAVE_INTERVAL <= 20,
            "saving every pair is I/O bound; saving never loses too much"
        );
    }

    #[test]
    fn preview_rows_name_the_file_the_run_will_write() {
        let dir = std::env::temp_dir();
        let pair = FilePair {
            video: dir.join("season1_ep01.mp4"),
            audio: dir.join("season1_ep01.m4a"),
            stem: "season1/ep01".to_string(),
        };
        let rows = preview_rows(std::slice::from_ref(&pair), OutputFormat::Mkv);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].output, "season1_ep01.mkv");
    }
}
