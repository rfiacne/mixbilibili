use crate::i18n::{t, tf};
use crate::progress::format_duration;
use std::io::Write;
use std::time::Duration;

#[derive(Debug)]
pub struct MergeResult {
    pub pair_index: usize,
    pub pair_name: String,
    pub success: bool,
    pub error: Option<String>,
    /// Duration of the ffmpeg merge operation.
    pub duration: Duration,
    /// Set to true when the merge was skipped due to INTERRUPTED flag.
    pub was_interrupted: bool,
}

#[derive(Debug, Default)]
pub struct MergeSummary {
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub orphaned_count: usize,
    pub failures: Vec<(String, String)>,
    pub deletion_failures: usize,
    pub total_duration: Duration,
    pub merge_count: usize,
}

impl MergeSummary {
    /// Average duration per merge operation.
    pub fn avg_duration(&self) -> Option<Duration> {
        if self.merge_count == 0 {
            return None;
        }
        Some(self.total_duration / self.merge_count as u32)
    }

    /// Files processed per second.
    pub fn throughput(&self) -> Option<f64> {
        if self.total_duration.is_zero() {
            return None;
        }
        Some(self.merge_count as f64 / self.total_duration.as_secs_f64())
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
        let symbols = crate::ui::Symbols::new(crate::ui::supports_unicode());
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());
        self.write_full_report(&mut out, &symbols);
        let _ = out.flush();
    }

    /// Renders the report box. Split out from [`print_full_report`] so the exact
    /// layout can be asserted in tests.
    ///
    /// Every line is `content_width + 4` columns wide (`border + " " + content +
    /// " " + border`), so the box closes on all four sides whatever the rows say.
    fn write_full_report(&self, out: &mut impl Write, symbols: &crate::ui::Symbols) {
        use crate::ui::{write_padding, Colors};
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

        if self.merge_count > 0 {
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
        use crate::ui::Colors;
        let painted = match style {
            Style::Success => Colors::success(s).to_string(),
            Style::Failure => Colors::failure(s).to_string(),
            Style::Warning => Colors::warning(s).to_string(),
            Style::Secondary => Colors::secondary(s).to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_summary_default() {
        let summary = MergeSummary::default();
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert_eq!(summary.merge_count, 0);
        assert!(summary.total_duration.is_zero());
        assert!(summary.all_success());
    }

    #[test]
    fn test_merge_summary_all_success_false_with_failures() {
        let summary = MergeSummary {
            failed_count: 1,
            ..Default::default()
        };
        assert!(!summary.all_success());
    }

    #[test]
    fn test_timing_methods() {
        let mut summary = MergeSummary::default();
        summary.total_duration += Duration::from_millis(100);
        summary.merge_count += 1;
        summary.total_duration += Duration::from_millis(300);
        summary.merge_count += 1;
        summary.total_duration += Duration::from_millis(200);
        summary.merge_count += 1;

        assert_eq!(summary.total_duration, Duration::from_millis(600));
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(200)));
        assert!(summary.throughput().is_some());
        assert!(summary.throughput().unwrap() > 0.0);
    }

    #[test]
    fn test_timing_empty_summary() {
        let summary = MergeSummary::default();
        assert!(summary.total_duration.is_zero());
        assert!(summary.avg_duration().is_none());
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn test_merge_summary_avg_duration_none_when_zero_merges() {
        let summary = MergeSummary::default();
        assert_eq!(summary.merge_count, 0);
        assert!(summary.avg_duration().is_none());
    }

    #[test]
    fn test_merge_summary_avg_duration_some_when_merges_exist() {
        let summary = MergeSummary {
            total_duration: Duration::from_secs(6),
            merge_count: 3,
            ..Default::default()
        };
        assert_eq!(summary.avg_duration(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn test_merge_summary_throughput_none_when_zero_duration() {
        let summary = MergeSummary {
            merge_count: 5,
            total_duration: Duration::ZERO,
            ..Default::default()
        };
        assert!(summary.throughput().is_none());
    }

    #[test]
    fn test_merge_summary_throughput_computes_rate() {
        let summary = MergeSummary {
            merge_count: 10,
            total_duration: Duration::from_secs(2),
            ..Default::default()
        };
        let tp = summary.throughput().unwrap();
        assert!((tp - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_format_duration() {
        assert!(format_duration(Duration::from_millis(50)).contains("ms"));
        assert!(format_duration(Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(Duration::from_secs(5)).contains("s"));
        assert!(!format_duration(Duration::from_secs(5)).contains("ms"));
        assert!(format_duration(Duration::from_secs(90)).contains("m"));
    }

    #[test]
    fn test_merge_summary_accumulate() {
        let mut summary = MergeSummary::default();
        summary.success_count += 3;
        summary.failed_count += 1;
        summary.total_duration += Duration::from_secs(10);
        summary.merge_count += 4;

        assert_eq!(summary.success_count, 3);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.merge_count, 4);
        assert_eq!(summary.total_duration, Duration::from_secs(10));
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(2500)));
    }

    #[test]
    fn test_merge_summary_all_success_when_no_failures() {
        let summary = MergeSummary {
            success_count: 5,
            failed_count: 0,
            ..Default::default()
        };
        assert!(summary.all_success());
    }

    #[test]
    fn test_merge_summary_all_success_false_when_failures() {
        let summary = MergeSummary {
            success_count: 3,
            failed_count: 2,
            ..Default::default()
        };
        assert!(!summary.all_success());
    }

    #[test]
    fn test_merge_result_debug() {
        let result = MergeResult {
            pair_index: 0,
            pair_name: "video".to_string(),
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
            success: false,
            error: Some("test error".to_string()),
            duration: Duration::from_secs(1),
            was_interrupted: false,
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_merge_result_interrupted_flag() {
        let result = MergeResult {
            pair_index: 3,
            pair_name: "interrupted_video".to_string(),
            success: false,
            error: Some("Skipped (interrupted)".to_string()),
            duration: Duration::ZERO,
            was_interrupted: true,
        };
        assert!(result.was_interrupted);
        assert!(!result.success);
        assert_eq!(result.duration, Duration::ZERO);
    }

    #[test]
    fn test_merge_summary_with_skipped_and_orphaned() {
        let summary = MergeSummary {
            success_count: 5,
            failed_count: 1,
            skipped_count: 2,
            orphaned_count: 3,
            failures: vec![("bad.mp4".to_string(), "codec error".to_string())],
            deletion_failures: 1,
            total_duration: Duration::from_secs(12),
            merge_count: 6,
        };
        assert!(!summary.all_success());
        assert_eq!(summary.avg_duration(), Some(Duration::from_secs(2)));
        assert!(summary.throughput().unwrap() > 0.0);
    }

    #[test]
    fn test_merge_summary_throughput_precision() {
        let summary = MergeSummary {
            merge_count: 3,
            total_duration: Duration::from_secs(3),
            ..Default::default()
        };
        let tp = summary.throughput().unwrap();
        assert!((tp - 1.0).abs() < f64::EPSILON, "3 merges / 3s = 1.0/s");
    }

    #[test]
    fn test_merge_summary_avg_with_subsecond_duration() {
        let summary = MergeSummary {
            merge_count: 4,
            total_duration: Duration::from_millis(400),
            ..Default::default()
        };
        assert_eq!(summary.avg_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_merge_summary_failures_vec() {
        let mut summary = MergeSummary::default();
        summary
            .failures
            .push(("a.mp4".to_string(), "err1".to_string()));
        summary
            .failures
            .push(("b.mp4".to_string(), "err2".to_string()));
        summary.failed_count = 2;
        assert_eq!(summary.failures.len(), 2);
        assert!(!summary.all_success());
    }

    /// Renders with an explicit symbol set: deterministic on every platform,
    /// and no test mutates process-wide env vars that other tests read.
    fn render_report_with(summary: &MergeSummary, symbols: &crate::ui::Symbols) -> String {
        let mut buf: Vec<u8> = Vec::new();
        summary.write_full_report(&mut buf, symbols);
        String::from_utf8(buf).expect("report output should be UTF-8")
    }

    fn render_report(summary: &MergeSummary) -> String {
        render_report_with(summary, &crate::ui::Symbols::new(false))
    }

    /// Only the box strands, so the trailing failure list is excluded.
    fn box_lines(out: &str) -> Vec<&str> {
        out.lines()
            .filter(|l| l.starts_with(['┌', '│', '├', '└', '+', '|']))
            .collect()
    }

    fn width_of(s: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(s)
    }

    fn sample_summary() -> MergeSummary {
        MergeSummary {
            success_count: 12,
            failed_count: 3,
            merge_count: 12,
            total_duration: Duration::from_secs(5),
            failures: vec![("a.mp4".into(), "boom".into())],
            ..Default::default()
        }
    }

    #[test]
    fn test_report_box_lines_share_one_width() {
        let out = render_report(&sample_summary());
        let lines = box_lines(&out);
        assert!(lines.len() >= 4, "expected a closed box, got:\n{out}");

        // This holds in both symbol modes, so it needs no env override.
        let widths: Vec<usize> = lines.iter().map(|l| width_of(l)).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "box lines must share one display width, got {widths:?}:\n{out}"
        );

        for line in &lines {
            assert!(
                line.ends_with(['┐', '│', '┤', '┘', '+', '|']),
                "line must close the box: {line}"
            );
        }
    }

    #[test]
    fn test_report_uses_separator_not_a_second_top_border() {
        // Unicode mode is required to even see this defect: in ASCII mode every
        // corner is "+", which is exactly why it stayed hidden there.
        let symbols = crate::ui::Symbols::new(true);
        let out = render_report_with(&Default::default(), &symbols);

        // The old report drew its mid border with the top-left/right corners, so
        // the box visually opened twice. Exactly one top border is allowed.
        let top_borders = out
            .lines()
            .filter(|l| l.starts_with(symbols.top_left))
            .count();
        assert_eq!(
            top_borders, 1,
            "expected exactly one top border, found {top_borders}:\n{out}"
        );
        assert!(
            out.contains(symbols.middle_left) && out.contains(symbols.middle_right),
            "expected a tee separator inside the box:\n{out}"
        );
    }

    #[test]
    fn test_report_failure_connectors_follow_symbol_style() {
        let summary = MergeSummary {
            failed_count: 2,
            failures: vec![
                ("one.mp4".into(), "e1".into()),
                ("two.mp4".into(), "e2".into()),
            ],
            ..Default::default()
        };
        let symbols = crate::ui::Symbols::new(false);
        let out = render_report_with(&summary, &symbols);

        // Connectors come from `Symbols`, so `--ascii` no longer leaks Unicode
        // box characters into the failure list.
        assert!(
            out.contains(&format!(
                "  {}{} ",
                symbols.middle_left, symbols.horizontal_line
            )),
            "first failure should use a tee connector:\n{out}"
        );
        assert!(
            out.contains(&format!(
                "  {}{} ",
                symbols.bottom_left, symbols.horizontal_line
            )),
            "last failure should use a corner connector:\n{out}"
        );
    }

    #[test]
    fn test_report_box_grows_with_content() {
        let base = MergeSummary::default();
        let wide = MergeSummary {
            success_count: 123_456_789,
            ..Default::default()
        };
        let short_out = render_report(&base);
        let long_out = render_report(&wide);
        let short = box_lines(&short_out);
        let long = box_lines(&long_out);

        let short_width = width_of(short[0]);
        let long_width = width_of(long[0]);

        // The box used to be pinned at 40 dashes, so wide rows overflowed it.
        assert!(
            long_width > short_width,
            "box must widen for a longer row ({short_width} vs {long_width})"
        );
        for line in &long {
            assert_eq!(width_of(line), long_width, "line overflows the box: {line}");
        }
    }
}
