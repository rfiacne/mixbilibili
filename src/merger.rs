use crate::cli::OutputFormat;
use crate::ffmpeg::{self, MergeRunner, RunOutcome};
use crate::i18n::{t, tf};
use crate::interrupt::Interrupt;
use crate::progress::{format_duration, MergeProgress};
use crate::scanner::{FilePair, ScanStats};
use crate::ui::{supports_unicode, Colors, Symbols};
use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct MergeResult {
    pub pair_index: usize,
    pub pair_name: String,
    /// Where the merged file belongs, so cleanup never rebuilds the path.
    pub output_path: PathBuf,
    pub success: bool,
    pub error: Option<String>,
    /// Duration of the ffmpeg merge operation.
    pub duration: Duration,
    /// Set when the merge was skipped because the interrupt token fired.
    pub was_interrupted: bool,
}

#[derive(Debug, Default)]
pub struct MergeSummary {
    success_count: usize,
    failed_count: usize,
    skipped_count: usize,
    orphaned_count: usize,
    failures: Vec<(String, String)>,
    deletion_failures: usize,
    total_duration: Duration,
}

impl MergeSummary {
    /// Build the report from what actually happened. Interrupted pairs are
    /// neither successes nor failures — they stay pending for `--resume`.
    pub fn from_results(
        results: &[MergeResult],
        stats: &ScanStats,
        deletion_failures: usize,
    ) -> Self {
        let mut summary = MergeSummary {
            skipped_count: stats.skipped,
            orphaned_count: stats.orphaned,
            deletion_failures,
            ..Default::default()
        };

        for result in results {
            if result.success {
                summary.success_count += 1;
                summary.total_duration += result.duration;
            } else if !result.was_interrupted {
                summary.failed_count += 1;
                if let Some(ref err) = result.error {
                    summary
                        .failures
                        .push((result.pair_name.clone(), err.clone()));
                }
            }
        }

        summary
    }

    pub fn success_count(&self) -> usize {
        self.success_count
    }

    pub fn failed_count(&self) -> usize {
        self.failed_count
    }

    pub fn failures(&self) -> &[(String, String)] {
        &self.failures
    }

    pub fn avg_duration(&self) -> Option<Duration> {
        if self.success_count == 0 {
            return None;
        }
        Some(self.total_duration / self.success_count as u32)
    }

    /// Files processed per second.
    pub fn throughput(&self) -> Option<f64> {
        if self.total_duration.is_zero() {
            return None;
        }
        Some(self.success_count as f64 / self.total_duration.as_secs_f64())
    }

    pub fn all_success(&self) -> bool {
        self.failed_count == 0
    }

    pub fn print_report(&self, quiet: bool) {
        if quiet {
            self.print_quiet_report();
        } else {
            self.print_full_report();
        }
    }

    fn print_quiet_report(&self) {
        let total = self.success_count + self.failed_count;
        if self.failed_count > 0 {
            println!(
                "{}",
                tf(
                    "merged_summary_fail",
                    &[
                        &self.success_count.to_string(),
                        &total.to_string(),
                        &self.failed_count.to_string(),
                    ]
                )
            );
        } else {
            println!(
                "{}",
                tf(
                    "merged_summary_ok",
                    &[&self.success_count.to_string(), &total.to_string(),]
                )
            );
        }
    }

    fn print_full_report(&self) {
        // Many short lines: one buffered, locked writer instead of a syscall each.
        let symbols = Symbols::new(supports_unicode());
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());
        self.write_full_report(&mut out, &symbols);
        let _ = std::io::Write::flush(&mut out);
    }

    /// Renders the report box. Split out from [`print_full_report`] so the exact
    /// layout can be asserted in tests.
    ///
    /// Every line is `content_width + 4` columns wide (`border + " " + content +
    /// " " + border`), so the box closes on all four sides whatever the rows say.
    /// The width follows the content instead of a fixed 40 columns, and the mid
    /// separator uses the *middle* tees rather than a second pair of top corners.
    fn write_full_report(&self, out: &mut impl std::io::Write, symbols: &Symbols) {
        use crate::ui::write_padding;
        use unicode_width::UnicodeWidthStr;

        let title = t("merge_report").into_owned();

        // Rows carry both their plain and coloured text: the box width must be
        // measured from the plain text, since ANSI escapes have zero display
        // width but would still be counted by a naive width().
        let mut rows: Vec<ReportLine> = Vec::new();

        let success_count = self.success_count.to_string();
        let failed_count = self.failed_count.to_string();
        let mut stats = ReportLine::default();
        stats
            .styled(Style::Success, symbols.success)
            .text(" ")
            .text(&tf("succeeded_fmt", &[&success_count]))
            .text("    ")
            .styled(Style::Failure, symbols.failure)
            .text(" ")
            .text(&tf("failed_fmt", &[&failed_count]));
        rows.push(stats);

        if self.skipped_count > 0 {
            let mut row = ReportLine::default();
            row.text(&tf("skipped_fmt", &[&self.skipped_count.to_string()]))
                .text(" ")
                .styled(Style::Secondary, "(aria2)");
            rows.push(row);
        }

        if self.orphaned_count > 0 {
            let mut row = ReportLine::default();
            row.text(&tf("orphaned_fmt", &[&self.orphaned_count.to_string()]))
                .text(" ")
                .styled(Style::Secondary, "(orphan)");
            rows.push(row);
        }

        if self.success_count > 0 {
            let label = t("duration").into_owned();
            let mut row = ReportLine::default();
            row.styled(Style::Secondary, &label)
                .text(": ")
                .text(&format_duration(self.total_duration));
            rows.push(row);

            if let Some(avg) = self.avg_duration() {
                let label = t("avg").into_owned();
                let mut row = ReportLine::default();
                row.styled(Style::Secondary, &label)
                    .text(": ")
                    .text(&format_duration(avg));
                rows.push(row);
            }

            if let Some(tp) = self.throughput() {
                let label = t("throughput").into_owned();
                let mut row = ReportLine::default();
                row.styled(Style::Secondary, &label)
                    .text(&format!(": {tp:.2} pairs/sec"));
                rows.push(row);
            }
        }

        if self.deletion_failures > 0 {
            let mut row = ReportLine::default();
            row.text(&tf(
                "deletion_failures",
                &[&self.deletion_failures.to_string()],
            ))
            .text(" ")
            .styled(Style::Warning, "(warn)");
            rows.push(row);
        }

        let content_width = rows
            .iter()
            .map(|r| r.width())
            .max()
            .unwrap_or(0)
            .max(UnicodeWidthStr::width(title.as_str()));

        let dashes = symbols.horizontal_line.repeat(content_width + 2);
        let vertical = Colors::secondary(symbols.vertical_line);

        let top = format!("{}{}{}", symbols.top_left, dashes, symbols.top_right);
        let _ = writeln!(out, "{}", Colors::secondary(&top));

        let _ = write!(out, "{vertical} ");
        let _ = write!(out, "{}", Colors::title(&title));
        write_padding(out, content_width - UnicodeWidthStr::width(title.as_str()));
        let _ = writeln!(out, " {vertical}");

        let mid = format!("{}{}{}", symbols.middle_left, dashes, symbols.middle_right);
        let _ = writeln!(out, "{}", Colors::secondary(&mid));

        for row in &rows {
            let _ = write!(out, "{vertical} ");
            let _ = write!(out, "{}", row.colored);
            write_padding(out, content_width - row.width());
            let _ = writeln!(out, " {vertical}");
        }

        let bottom = format!("{}{}{}", symbols.bottom_left, dashes, symbols.bottom_right);
        let _ = writeln!(out, "{}", Colors::secondary(&bottom));

        if !self.failures.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "{}", Colors::failure(&t("failed_files")));
            for (i, (name, error)) in self.failures.iter().enumerate() {
                // Connectors come from `Symbols` too, so they follow --ascii.
                let connector = if i == self.failures.len() - 1 {
                    symbols.bottom_left
                } else {
                    symbols.middle_left
                };
                let _ = writeln!(
                    out,
                    "  {connector}{} {} {}: {}",
                    symbols.horizontal_line,
                    Colors::failure(symbols.failure),
                    name,
                    error
                );
            }
            let _ = writeln!(out);
        }
    }
}

/// Box-drawing style for a report line.
#[derive(Clone, Copy)]
enum Style {
    Success,
    Failure,
    Warning,
    Secondary,
}

/// A report line in both plain and coloured form.
///
/// The box width is measured from `plain`, since ANSI escapes have zero display
/// width but a naive `width()` over the coloured string would still count them.
#[derive(Default)]
struct ReportLine {
    plain: String,
    colored: String,
}

impl ReportLine {
    /// Appends unstyled text.
    fn text(&mut self, s: &str) -> &mut Self {
        self.plain.push_str(s);
        self.colored.push_str(s);
        self
    }

    /// Appends `s` rendered in `style` (plain copy kept for measuring).
    fn styled(&mut self, style: Style, s: &str) -> &mut Self {
        let painted = match style {
            Style::Success => Colors::success(s),
            Style::Failure => Colors::failure(s),
            Style::Warning => Colors::warning(s),
            Style::Secondary => Colors::secondary(s),
        };
        self.plain.push_str(s);
        self.colored.push_str(&painted);
        self
    }

    /// Display width of the plain text.
    fn width(&self) -> usize {
        unicode_width::UnicodeWidthStr::width(self.plain.as_str())
    }
}

/// Everything one merge needs to know.
pub struct MergePlan<'a> {
    pub output_dir: &'a Path,
    pub format: OutputFormat,
    pub retries: usize,
    pub verbose: bool,
    pub progress: Option<&'a MergeProgress>,
    /// Quiet mode has no per-pair lines at all: the summary is the output.
    pub quiet: bool,
    pub interrupt: &'a Interrupt,
}

pub fn merge_pair(
    pair: &FilePair,
    pair_index: usize,
    plan: &MergePlan,
    runner: &dyn MergeRunner,
) -> MergeResult {
    let output_path = pair.output_path(plan.output_dir, plan.format);

    if plan.interrupt.is_triggered() {
        return MergeResult {
            pair_index,
            pair_name: pair.stem.clone(),
            output_path,
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
    }

    // Safety check: the output must not be the input video itself. Canonicalize
    // so different path spellings of the same file are still caught.
    let dominated = std::fs::canonicalize(&output_path)
        .ok()
        .zip(std::fs::canonicalize(&pair.video).ok())
        .map(|(o, v)| o == v)
        .unwrap_or(false);
    if dominated {
        return record_failure(
            pair,
            pair_index,
            output_path,
            t("same_source_output_runtime").into_owned(),
            Duration::ZERO,
            plan,
        );
    }

    do_merge(pair, output_path, pair_index, plan, runner)
}

fn record_failure(
    pair: &FilePair,
    pair_index: usize,
    output_path: PathBuf,
    error_msg: String,
    duration: Duration,
    plan: &MergePlan,
) -> MergeResult {
    announce(plan, &pair.stem, false, duration, Some(&error_msg));
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        output_path,
        success: false,
        error: Some(error_msg),
        duration,
        was_interrupted: false,
    }
}

/// Tell the user about one finished pair: on the progress bar when there is
/// one, otherwise as a line on stdout — unless the run is quiet.
fn announce(plan: &MergePlan, stem: &str, success: bool, duration: Duration, error: Option<&str>) {
    match plan.progress {
        Some(p) => p.record(stem, success, duration, error, None),
        None => {
            if let Some(line) = status_line(stem, success, error, plan.quiet) {
                println!("{line}");
            }
        }
    }
}

fn status_line(stem: &str, success: bool, error: Option<&str>, quiet: bool) -> Option<String> {
    if quiet {
        return None;
    }
    let symbols = Symbols::new(supports_unicode());
    Some(if success {
        Colors::success(&format!("{} {}", symbols.success, stem))
    } else {
        Colors::failure(&format!(
            "{} {}: {}",
            symbols.failure,
            stem,
            error.unwrap_or_default()
        ))
    })
}

/// Validates that the merge output exists, is non-zero, and is at least
/// `min_expected` bytes.
///
/// With `-c copy`, the output holds the video stream plus the audio stream
/// plus container headers, so it must be >= the source video size; anything
/// smaller indicates a truncated or incomplete merge that ffmpeg still
/// reported as successful.
fn validate_output(output_path: &Path, min_expected: u64) -> Result<()> {
    let metadata = match std::fs::metadata(output_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "failed_create_output_file",
                    &[&output_path.display().to_string()]
                )
            );
        }
        Err(e) => return Err(e.into()),
    };
    if metadata.len() == 0 {
        if let Err(e) = std::fs::remove_file(output_path) {
            eprintln!("Warning: Failed to remove empty output file: {}", e);
        }
        anyhow::bail!(
            "{}",
            tf("output_file_empty", &[&output_path.display().to_string()])
        );
    }
    if metadata.len() < min_expected {
        if let Err(e) = std::fs::remove_file(output_path) {
            eprintln!("Warning: Failed to remove undersized output file: {}", e);
        }
        anyhow::bail!(
            "{}",
            tf(
                "output_too_small",
                &[
                    &output_path.display().to_string(),
                    &metadata.len().to_string(),
                    &min_expected.to_string(),
                ]
            )
        );
    }
    Ok(())
}

fn do_merge(
    pair: &FilePair,
    output_path: PathBuf,
    pair_index: usize,
    plan: &MergePlan,
    runner: &dyn MergeRunner,
) -> MergeResult {
    let start = std::time::Instant::now();
    let retries = plan.retries;

    for attempt in 0..=retries {
        // Stop early once cancelled: retrying after Ctrl+C is wasted work.
        if plan.interrupt.is_triggered() {
            return MergeResult {
                pair_index,
                pair_name: pair.stem.clone(),
                output_path,
                success: false,
                error: None,
                duration: start.elapsed(),
                was_interrupted: true,
            };
        }

        if attempt > 0 {
            announce_retry(pair, attempt, plan);
        } else if plan.verbose {
            println!(
                "Running: ffmpeg -i {} -i {} -> {}",
                pair.video.display(),
                pair.audio.display(),
                output_path.display()
            );
        }

        let mut cmd =
            ffmpeg::build_merge_command(&pair.video, &pair.audio, &output_path, plan.format);

        // Each arm below either returns, retries, or falls through to the one
        // failure exit at the bottom of the loop.
        let error_msg = match runner.run(&mut cmd, plan.interrupt) {
            Ok(RunOutcome::Success) => {
                // With `-c copy` the output carries the video stream plus the
                // audio stream, so it can never be smaller than the source
                // video. When the source cannot be stat'd the check degrades to
                // the non-zero test rather than inventing a size.
                let min_expected = std::fs::metadata(&pair.video).map(|m| m.len()).unwrap_or(0);
                match validate_output(&output_path, min_expected) {
                    // ffmpeg claims it worked; the file on disk has to agree.
                    Ok(()) => {
                        return record_success(pair, pair_index, output_path, start.elapsed(), plan)
                    }
                    Err(e) => e.to_string(),
                }
            }
            // A cancelled pair is not a failure: it stays pending for resume.
            Ok(RunOutcome::Cancelled) => {
                return MergeResult {
                    pair_index,
                    pair_name: pair.stem.clone(),
                    output_path,
                    success: false,
                    error: None,
                    duration: start.elapsed(),
                    was_interrupted: true,
                }
            }
            Ok(RunOutcome::TimedOut) if attempt < retries => continue,
            Ok(RunOutcome::Failed { .. }) if attempt < retries => continue,
            Err(_) if attempt < retries => continue,
            Ok(RunOutcome::TimedOut) => t("timed_out").into_owned(),
            Ok(RunOutcome::Failed { code, message }) => failure_message(code, &message, retries),
            Err(e) => tf("merge_failed_io", &[&e.to_string(), &retries.to_string()]),
        };

        return record_failure(
            pair,
            pair_index,
            output_path,
            error_msg,
            start.elapsed(),
            plan,
        );
    }

    unreachable!("the final attempt always returns")
}

fn record_success(
    pair: &FilePair,
    pair_index: usize,
    output_path: PathBuf,
    duration: Duration,
    plan: &MergePlan,
) -> MergeResult {
    announce(plan, &pair.stem, true, duration, None);
    MergeResult {
        pair_index,
        pair_name: pair.stem.clone(),
        output_path,
        success: true,
        error: None,
        duration,
        was_interrupted: false,
    }
}

/// Back off, then say so: on the progress bar if there is one, on stdout when
/// verbose.
fn announce_retry(pair: &FilePair, attempt: usize, plan: &MergePlan) {
    std::thread::sleep(Duration::from_secs(1));
    if let Some(p) = plan.progress {
        p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
    } else if plan.verbose {
        println!(
            "{} {}",
            "↻".yellow(),
            tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
        );
    }
}

/// What ffmpeg said beats a bare exit code, so the last line of its stderr is
/// appended when there is one.
fn failure_message(code: Option<i32>, stderr: &str, retries: usize) -> String {
    let base = tf(
        "merge_failed_exit",
        &[&exit_code_text(code), &retries.to_string()],
    );
    match stderr.trim().lines().last() {
        Some(reason) => format!("{base}: {reason}"),
        None => base,
    }
}

/// Render an exit code for a message, when there is one.
fn exit_code_text(code: Option<i32>) -> String {
    code.map(|c| c.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Remove a merge output that never completed, reporting whether anything was
/// actually deleted.
pub(crate) fn remove_partial(output_path: &Path) -> Result<bool> {
    match std::fs::remove_file(output_path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn delete_source_files(pair: &FilePair) -> Result<()> {
    // Delete audio first (less valuable file)
    // Attempt deletion directly and interpret errors to avoid TOCTOU race
    match std::fs::remove_file(&pair.audio) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "{}",
                tf(
                    "source_missing",
                    &["audio", &pair.audio.display().to_string()]
                )
            );
        }
        Err(e) => {
            // Audio deletion failed, preserve video
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_audio",
                    &[
                        &pair.audio.display().to_string(),
                        &e.to_string(),
                        &pair.video.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    // Then delete video
    match std::fs::remove_file(&pair.video) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Audio already deleted, video missing
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video_audio_gone",
                    &[
                        &pair.video.display().to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Err(e) => {
            // Video deletion failed, but audio already deleted
            anyhow::bail!(
                "{}",
                tf(
                    "failed_delete_video",
                    &[
                        &pair.video.display().to_string(),
                        &e.to_string(),
                        &pair.audio.display().to_string()
                    ]
                )
            );
        }
        Ok(_) => {}
    }

    Ok(())
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    fn result(name: &str, success: bool, interrupted: bool) -> MergeResult {
        MergeResult {
            pair_index: 0,
            pair_name: name.to_string(),
            output_path: PathBuf::from(format!("/out/{name}.mkv")),
            success,
            error: if success {
                None
            } else {
                Some("boom".to_string())
            },
            duration: if success {
                Duration::from_secs(2)
            } else {
                Duration::ZERO
            },
            was_interrupted: interrupted,
        }
    }

    #[test]
    fn counts_successes_and_failures() {
        let results = vec![result("a", true, false), result("b", false, false)];
        let summary = MergeSummary::from_results(&results, &ScanStats::default(), 0);
        assert_eq!(summary.success_count(), 1);
        assert_eq!(summary.failed_count(), 1);
        assert!(!summary.all_success());
        assert_eq!(summary.failures()[0].0, "b");
    }

    #[test]
    fn interrupted_pairs_are_not_failures() {
        let results = vec![result("a", true, false), result("b", false, true)];
        let summary = MergeSummary::from_results(&results, &ScanStats::default(), 0);
        assert_eq!(summary.success_count(), 1);
        assert_eq!(summary.failed_count(), 0);
        assert!(summary.all_success(), "an interrupt is not a merge failure");
    }

    #[test]
    fn scan_stats_and_deletions_are_carried_through() {
        let stats = ScanStats {
            pairs: 3,
            skipped: 2,
            orphaned: 1,
        };
        let summary = MergeSummary::from_results(&[], &stats, 4);
        assert_eq!(summary.success_count(), 0);
        assert!(summary.avg_duration().is_none());
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn timings_average_over_successes_only() {
        let results = vec![
            result("a", true, false),
            result("b", true, false),
            result("c", false, false),
        ];
        let summary = MergeSummary::from_results(&results, &ScanStats::default(), 0);
        assert_eq!(summary.avg_duration(), Some(Duration::from_secs(2)));
        assert!(
            (summary.throughput().unwrap() - 0.5).abs() < f64::EPSILON,
            "2 merges in 4s"
        );
    }

    #[test]
    fn empty_batch_has_no_timing_to_report() {
        let summary = MergeSummary::from_results(&[], &ScanStats::default(), 0);
        assert!(summary.all_success());
        assert!(summary.failures().is_empty());
        assert!(summary.avg_duration().is_none());
    }

    #[test]
    fn quiet_runs_print_no_per_pair_lines() {
        assert!(status_line("ep01", true, None, true).is_none());
        assert!(status_line("ep01", false, Some("boom"), true).is_none());
        assert!(status_line("ep01", true, None, false)
            .unwrap()
            .contains("ep01"));
        assert!(status_line("ep01", false, Some("boom"), false)
            .unwrap()
            .contains("boom"));
    }

    #[test]
    fn report_box_is_closed_and_every_line_has_one_width() {
        // Regression: the title row was unpadded, the mid separator was drawn
        // with the *top* corners (a second `┌───┐` line), and the width was
        // pinned at 40 columns so wider rows overflowed the box.
        use unicode_width::UnicodeWidthStr;

        let summary = MergeSummary::from_results(
            &[result("ep01", true, false), result("ep02", true, false)],
            &ScanStats::default(),
            0,
        );

        let mut buf = Vec::new();
        summary.write_full_report(&mut buf, &Symbols::new(true));
        let text = String::from_utf8(buf).expect("report output should be UTF-8");

        let widths: Vec<usize> = text.lines().map(UnicodeWidthStr::width).collect();
        assert!(
            widths.iter().all(|w| *w == widths[0]),
            "report box lines differ in width: {widths:?}\n{text}"
        );
        assert!(widths.len() >= 3, "box should have a top, title and rows");

        // The separator is a *middle* tee line, not a second top corner.
        assert!(
            text.contains("├"),
            "mid separator should use middle tees:\n{text}"
        );
        assert_eq!(
            text.matches("┌").count(),
            1,
            "only the top border may use a top-left corner:\n{text}"
        );
    }

    #[test]
    fn report_box_stays_closed_when_failure_names_are_long() {
        // The failure list below the box is free-form, but the box itself must
        // stay closed no matter how long a failure name is.
        use unicode_width::UnicodeWidthStr;

        let long = MergeResult {
            pair_index: 0,
            pair_name: "a_very_long_pair_name_that_exceeds_the_old_fixed_width".to_string(),
            output_path: PathBuf::from("/out/x.mkv"),
            success: false,
            error: Some(
                "ffmpeg said something quite long indeed, longer than forty columns".to_string(),
            ),
            duration: Duration::ZERO,
            was_interrupted: false,
        };
        let summary = MergeSummary::from_results(&[long], &ScanStats::default(), 0);

        let mut buf = Vec::new();
        summary.write_full_report(&mut buf, &Symbols::new(true));
        let text = String::from_utf8(buf).unwrap();

        // The box is everything up to and including the bottom border.
        let boxed: Vec<&str> = text
            .lines()
            .take_while(|l| !l.starts_with('└'))
            .chain(text.lines().filter(|l| l.starts_with('└')).take(1))
            .collect();
        let widths: Vec<usize> = boxed.iter().map(|l| UnicodeWidthStr::width(*l)).collect();
        assert!(
            widths.iter().all(|w| *w == widths[0]),
            "report box lines differ in width: {widths:?}\n{text}"
        );
        // The long failure is reported, just outside the box.
        assert!(text.contains("a_very_long_pair_name"));
    }

    #[test]
    fn report_box_uses_ascii_connectors_under_ascii_mode() {
        // The failure connectors used to hardcode Unicode box characters even
        // when --ascii asked for plain output.
        let failing = MergeResult {
            pair_index: 0,
            pair_name: "ep01".to_string(),
            output_path: PathBuf::from("/out/ep01.mkv"),
            success: false,
            error: Some("boom".to_string()),
            duration: Duration::ZERO,
            was_interrupted: false,
        };
        let summary = MergeSummary::from_results(&[failing], &ScanStats::default(), 0);

        let mut buf = Vec::new();
        summary.write_full_report(&mut buf, &Symbols::new(false));
        let text = String::from_utf8(buf).unwrap();

        for ch in ['└', '├', '─', '│', '┌', '┐', '┘'] {
            assert!(
                !text.contains(ch),
                "ASCII mode must not emit box character {ch}:\n{text}"
            );
        }
    }

    #[test]
    fn what_ffmpeg_said_beats_a_bare_exit_code() {
        let plain = failure_message(Some(183), "", 0);
        assert!(plain.contains("183"));
        let explained = failure_message(Some(183), "line one\nmoov atom not found\n", 0);
        assert!(explained.contains("moov atom not found"), "{explained}");
        assert!(
            !explained.contains("line one"),
            "only the last line: {explained}"
        );
    }
}
#[cfg(test)]
mod merge_tests {
    use super::*;

    // Note: These tests require ffmpeg to be installed
    // They test the function structure, not actual ffmpeg execution

    #[test]
    fn test_merge_result_debug() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            output_path: PathBuf::from("video.mkv"),
            success: true,
            error: None,
            duration: Duration::from_millis(123),
            was_interrupted: false,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("video"));
    }

    #[test]
    fn test_merge_result_with_error() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
            output_path: PathBuf::from("video.mkv"),
            success: false,
            error: Some("test error".to_string()),
            duration: Duration::from_secs(1),
            was_interrupted: false,
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_merge_validates_output_exists() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        // Simulate ffmpeg creating empty file (failure case)
        File::create(&output_path).unwrap();

        // Validation should reject empty file
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert_eq!(metadata.len(), 0, "Empty file should be detected");
    }

    #[test]
    fn test_merge_validates_output_nonzero_size() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("output.mkv");

        // Simulate ffmpeg creating valid file
        std::fs::write(&output_path, b"fake video data").unwrap();

        // Validation should accept non-zero file
        let metadata = std::fs::metadata(&output_path).unwrap();
        assert!(metadata.len() > 0, "Non-zero file should be accepted");
    }

    #[test]
    fn test_validate_output_helper() {
        use std::fs::File;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Test 1: Non-existent file
        let missing_path = dir.path().join("missing.mkv");
        let result = validate_output(&missing_path, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("未创建") || err.contains("not created"),
            "Error should mention file was not created: {}",
            err
        );

        // Test 2: Empty file
        let empty_path = dir.path().join("empty.mkv");
        File::create(&empty_path).unwrap();
        let result = validate_output(&empty_path, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty") || err.contains("为空"));
        // Empty file should be cleaned up
        assert!(!empty_path.exists(), "Empty file should be cleaned up");

        // Test 3: Valid file
        let valid_path = dir.path().join("valid.mkv");
        std::fs::write(&valid_path, b"video data").unwrap();
        let result = validate_output(&valid_path, 0);
        assert!(result.is_ok());
        assert!(valid_path.exists(), "Valid file should remain");
    }

    #[test]
    fn test_validate_output_rejects_undersized_output() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("truncated.mkv");
        std::fs::write(&output_path, b"short").unwrap();

        // A `-c copy` output is never smaller than the source video, so a
        // smaller one is a truncated merge ffmpeg reported as successful.
        let result = validate_output(&output_path, 1_000);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("incomplete") || err.contains("合并不完整"),
            "Error should say the merge may be incomplete: {}",
            err
        );
        // The bad output must not be left behind for the user to find.
        assert!(
            !output_path.exists(),
            "Undersized output should be cleaned up"
        );
    }

    #[test]
    fn test_validate_output_accepts_output_at_least_source_size() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("ok.mkv");
        std::fs::write(&output_path, vec![0u8; 1_000]).unwrap();

        // Exactly the source size is valid: the output also carries the audio
        // stream and headers, so the bound is a floor, not an equality.
        assert!(validate_output(&output_path, 1_000).is_ok());
        assert!(validate_output(&output_path, 999).is_ok());
        assert!(output_path.exists());
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_delete_source_files_success() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }

    #[test]
    fn test_delete_source_files_video_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Only create audio, video is missing
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // Check for i18n key or translated content mentioning video
        assert!(
            err.contains("failed_delete_video") || err.contains("video") || err.contains("视频"),
            "Error should mention video: {}",
            err
        );
    }

    #[test]
    fn test_delete_source_files_both_missing() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Neither file exists
        let pair = FilePair {
            video: video_path,
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // Should mention at least one missing file
        assert!(
            err.contains("video") || err.contains("audio"),
            "Error should mention missing file: {}",
            err
        );
    }

    #[test]
    fn test_deletion_failures_tracking() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("video.m4a");

        // Create video but NOT audio - deletion will fail for audio
        File::create(&video_path).unwrap();
        // audio_path does NOT exist

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        // delete_source_files should fail because audio doesn't exist
        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Deletion should fail when audio missing");

        // The error message should indicate which file failed
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("audio"),
            "Error should mention audio: {}",
            err_msg
        );

        // NEW BEHAVIOR: Video should be preserved (not deleted) because audio is missing
        // This is the atomic deletion guarantee
        assert!(
            video_path.exists(),
            "Video should be preserved when audio is missing (atomic deletion)"
        );
    }

    #[test]
    fn test_delete_preserves_video_on_audio_failure() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("nonexistent.m4a"); // Doesn't exist

        // Create only video
        File::create(&video_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path,
            stem: "video".to_string(),
        };

        let result = delete_source_files(&pair);
        assert!(result.is_err(), "Should fail when audio missing");

        // Video should still exist (not deleted)
        assert!(
            video_path.exists(),
            "Video should be preserved when audio deletion fails"
        );
    }

    #[test]
    fn test_delete_checks_existence_first() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("audio.m4a");

        // Create both files
        File::create(&video_path).unwrap();
        File::create(&audio_path).unwrap();

        let pair = FilePair {
            video: video_path.clone(),
            audio: audio_path.clone(),
            stem: "video".to_string(),
        };

        // Both exist, deletion should succeed
        let result = delete_source_files(&pair);
        assert!(result.is_ok());
        assert!(!video_path.exists());
        assert!(!audio_path.exists());
    }
}
