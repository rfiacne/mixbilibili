use crate::i18n::{t, tf};
use crate::progress::format_duration;
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
        use crate::ui::{Colors, Symbols};

        let symbols = Symbols::new(crate::ui::supports_unicode());
        let width = 40;

        let top_border = format!(
            "{}{}{}",
            symbols.top_left,
            symbols.horizontal_line.repeat(width),
            symbols.top_right
        );
        println!("{}", Colors::secondary(&top_border));

        let title = Colors::title(&t("merge_report"));
        println!(
            "{} {} {}",
            Colors::secondary(symbols.vertical_line),
            title,
            Colors::secondary(symbols.vertical_line)
        );

        let mid_border = format!(
            "{}{}{}",
            symbols.top_left,
            symbols.horizontal_line.repeat(width),
            symbols.top_right
        );
        println!("{}", Colors::secondary(&mid_border));

        let success_str = format!(
            "{} {}",
            Colors::success(symbols.success),
            tf("succeeded_fmt", &[&self.success_count.to_string()])
        );
        let fail_str = if self.failed_count > 0 {
            Colors::failure(&format!(
                "{} {}",
                symbols.failure,
                tf("failed_fmt", &[&self.failed_count.to_string()])
            ))
        } else {
            format!(
                "{} {}",
                symbols.failure,
                tf("failed_fmt", &[&self.failed_count.to_string()])
            )
        };
        println!("  {}    {}", success_str, fail_str);

        if self.skipped_count > 0 {
            println!(
                "  {} {}",
                tf("skipped_fmt", &[&self.skipped_count.to_string()]),
                Colors::secondary("(aria2)")
            );
        }
        if self.orphaned_count > 0 {
            println!(
                "  {} {}",
                tf("orphaned_fmt", &[&self.orphaned_count.to_string()]),
                Colors::secondary("(orphan)")
            );
        }

        if self.merge_count > 0 {
            let total = self.total_duration;
            println!(
                "  {}: {}",
                Colors::secondary(&t("duration")),
                format_duration(total)
            );
            if let Some(avg) = self.avg_duration() {
                println!(
                    "  {}: {}",
                    Colors::secondary(&t("avg")),
                    format_duration(avg)
                );
            }
            if let Some(tp) = self.throughput() {
                println!(
                    "  {}: {:.2} pairs/sec",
                    Colors::secondary(&t("throughput")),
                    tp
                );
            }
        }

        if self.deletion_failures > 0 {
            println!(
                "  {} {}",
                tf("deletion_failures", &[&self.deletion_failures.to_string()]),
                Colors::warning("(warn)")
            );
        }

        let bottom_border = format!(
            "{}{}{}",
            symbols.bottom_left,
            symbols.horizontal_line.repeat(width),
            symbols.bottom_right
        );
        println!("{}", Colors::secondary(&bottom_border));

        if !self.failures.is_empty() {
            println!("\n{}", Colors::failure(&t("failed_files")));
            for (i, (name, error)) in self.failures.iter().enumerate() {
                let connector = if i == self.failures.len() - 1 {
                    "└"
                } else {
                    "├"
                };
                println!(
                    "  {}─ {} {}: {}",
                    connector,
                    Colors::failure(symbols.failure),
                    name,
                    error
                );
            }
            println!();
        }
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
}
