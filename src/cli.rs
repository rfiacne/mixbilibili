use anyhow::{bail, Result};
use clap::{Arg, ArgAction, Command};
use std::path::PathBuf;

use crate::i18n::{t, tf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Mkv,
    Mp4,
    Mov,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
        }
    }

    pub fn needs_faststart(self) -> bool {
        matches!(self, Self::Mp4 | Self::Mov)
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}

/// Validate `--once` stem argument to prevent path traversal attacks.
pub fn validate_stem(stem: &str) -> Result<()> {
    // Reject path separators
    if stem.contains('/') || stem.contains('\\') {
        bail!("Invalid stem '{}': must not contain path separators", stem);
    }

    // Reject null bytes (path injection)
    if stem.contains('\0') {
        bail!("Invalid stem: must not contain null bytes");
    }

    // Reject empty or whitespace-only stems
    if stem.trim().is_empty() {
        bail!("Invalid stem: cannot be empty");
    }

    // Reject special directory names
    if stem == "." || stem == ".." {
        bail!("Invalid stem '{}': cannot be . or ..", stem);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension() {
        assert_eq!(OutputFormat::Mkv.extension(), "mkv");
        assert_eq!(OutputFormat::Mp4.extension(), "mp4");
        assert_eq!(OutputFormat::Mov.extension(), "mov");
    }

    #[test]
    fn test_needs_faststart() {
        assert!(!OutputFormat::Mkv.needs_faststart());
        assert!(OutputFormat::Mp4.needs_faststart());
        assert!(OutputFormat::Mov.needs_faststart());
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(format!("{}", OutputFormat::Mkv), "mkv");
        assert_eq!(format!("{}", OutputFormat::Mp4), "mp4");
        assert_eq!(format!("{}", OutputFormat::Mov), "mov");
    }

    #[test]
    fn test_output_format_all_variants_have_extension() {
        let formats = [OutputFormat::Mkv, OutputFormat::Mp4, OutputFormat::Mov];
        for fmt in &formats {
            let ext = fmt.extension();
            assert!(!ext.is_empty(), "{:?} has empty extension", fmt);
            assert_eq!(ext, format!("{}", fmt));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Args {
    pub source: PathBuf,
    pub output: PathBuf,
    pub sdel: bool,
    pub format: OutputFormat,
    pub jobs: usize,
    pub progress: bool,
    pub dry_run: bool,
    pub interactive: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub resume: bool,
    pub retry: usize,
    pub recursive: bool,
    pub once: Option<String>,
    pub ascii: bool,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

/// Build the CLI command with translated help text.
pub fn build_cli() -> Command {
    Command::new("mixbilibili")
        .version(env!("CARGO_PKG_VERSION"))
        .about(t("cli_about"))
        .arg(
            Arg::new("source")
                .short('s')
                .long("source")
                .help(t("cli_source"))
                .default_value("."),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help(t("cli_output"))
                .default_value("."),
        )
        .arg(
            Arg::new("sdel")
                .short('d')
                .long("sdel")
                .help(t("cli_sdel"))
                .default_value("true")
                .action(ArgAction::Set)
                .num_args(0..=1)
                .default_missing_value("true"),
        )
        .arg(
            Arg::new("format")
                .short('f')
                .long("format")
                .help(t("cli_format"))
                .default_value("mkv")
                .value_parser(["mkv", "mp4", "mov"]),
        )
        .arg(
            Arg::new("jobs")
                .short('j')
                .long("jobs")
                .help(t("cli_jobs"))
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("progress")
                .short('p')
                .long("progress")
                .help(t("cli_progress"))
                .default_value("true")
                .action(ArgAction::Set)
                .num_args(0..=1)
                .default_missing_value("true"),
        )
        .arg(
            Arg::new("dry_run")
                .short('n')
                .long("dry-run")
                .help(t("cli_dry_run"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("interactive")
                .short('i')
                .long("interactive")
                .help(t("cli_interactive"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help(t("cli_verbose"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help(t("cli_quiet"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("resume")
                .short('r')
                .long("resume")
                .help(t("cli_resume"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("retry")
                .long("retry")
                .help(t("cli_retry"))
                .default_value("0")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("recursive")
                .short('R')
                .long("recursive")
                .help(t("cli_recursive"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("once")
                .long("once")
                .help(t("cli_once"))
                .value_name("STEM"),
        )
        .arg(
            Arg::new("ascii")
                .long("ascii")
                .help(t("cli_ascii"))
                .action(ArgAction::SetTrue),
        )
}

/// A path-valued option. Falls back to the current directory when absent — the
/// same default the clap definitions declare.
fn path_arg(matches: &clap::ArgMatches, name: &str) -> PathBuf {
    matches
        .get_one::<String>(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A boolean option that clap stores as a string so `--flag=false` is
/// expressible. Only an explicit "false" turns it off.
fn flag_default_true(matches: &clap::ArgMatches, name: &str) -> bool {
    matches
        .get_one::<String>(name)
        .is_none_or(|value| value != "false")
}

/// Parse ArgMatches into Args struct.
pub fn parse_args(matches: &clap::ArgMatches) -> Args {
    Args {
        source: path_arg(matches, "source"),
        output: path_arg(matches, "output"),
        sdel: flag_default_true(matches, "sdel"),
        format: match matches
            .get_one::<String>("format")
            .map(String::as_str)
            .unwrap_or("mkv")
        {
            "mp4" => OutputFormat::Mp4,
            "mov" => OutputFormat::Mov,
            _ => OutputFormat::Mkv,
        },
        jobs: matches
            .get_one::<usize>("jobs")
            .copied()
            .unwrap_or_else(default_jobs),
        progress: flag_default_true(matches, "progress"),
        dry_run: matches.get_flag("dry_run"),
        interactive: matches.get_flag("interactive"),
        verbose: matches.get_flag("verbose"),
        quiet: matches.get_flag("quiet"),
        resume: matches.get_flag("resume"),
        retry: matches.get_one::<usize>("retry").copied().unwrap_or(0),
        recursive: matches.get_flag("recursive"),
        once: matches.get_one::<String>("once").cloned(),
        ascii: matches.get_flag("ascii"),
    }
}

impl Args {
    pub fn validate(&mut self) -> Result<()> {
        self.jobs = self.jobs.clamp(1, 32);

        if self.interactive && self.dry_run {
            bail!("Cannot use --interactive with --dry-run");
        }

        if self.verbose && self.quiet {
            bail!("{}", t("verbose_quiet_conflict"));
        }

        if !self.source.is_dir() {
            bail!(
                "{}",
                tf("not_dir_source", &[&self.source.display().to_string()])
            );
        }

        if self.output.exists() && !self.output.is_dir() {
            bail!(
                "{}",
                tf("not_dir_output", &[&self.output.display().to_string()])
            );
        }

        // Prevent source==output + mp4/mov format from overwriting source video files.
        // canonicalize() only succeeds for paths that exist, so a missing output
        // directory simply skips the check.
        if self.format == OutputFormat::Mp4 || self.format == OutputFormat::Mov {
            let src = std::fs::canonicalize(&self.source).ok();
            let out = std::fs::canonicalize(&self.output).ok();
            if src.is_some() && src == out {
                bail!("{}", tf("same_source_output", &[&self.format.to_string()]));
            }
        }

        Ok(())
    }
}

/// Parse `argv` (without the program name) into `Args` through the real CLI.
#[cfg(test)]
fn args_from(argv: &[&str]) -> Args {
    let mut full = vec!["mixbilibili"];
    full.extend_from_slice(argv);
    let matches = build_cli()
        .try_get_matches_from(full)
        .expect("test argv should parse");
    parse_args(&matches)
}

#[cfg(test)]
fn make_args() -> Args {
    Args {
        source: PathBuf::from("."),
        output: PathBuf::from("."),
        sdel: true,
        format: OutputFormat::Mkv,
        jobs: 4,
        progress: true,
        dry_run: false,
        interactive: false,
        verbose: false,
        quiet: false,
        resume: false,
        retry: 0,
        recursive: false,
        once: None,
        ascii: false,
    }
}

#[cfg(test)]
mod args_tests {
    use super::*;

    #[test]
    fn test_validate_jobs_clamp_to_min() {
        let mut args = make_args();
        args.jobs = 0;
        args.validate().unwrap();
        assert_eq!(args.jobs, 1);
    }

    #[test]
    fn test_validate_jobs_clamp_to_max() {
        let mut args = make_args();
        args.jobs = 100;
        args.validate().unwrap();
        assert_eq!(args.jobs, 32);
    }

    #[test]
    fn test_validate_jobs_in_range() {
        let mut args = make_args();
        args.validate().unwrap();
        assert_eq!(args.jobs, 4);
    }

    #[test]
    fn test_dry_run_flag_enabled() {
        assert!(args_from(&["--dry-run"]).dry_run);
    }

    #[test]
    fn test_verbose_flag_enabled() {
        assert!(args_from(&["--verbose"]).verbose);
    }

    #[test]
    fn test_resume_flag_enabled() {
        assert!(args_from(&["--resume"]).resume);
    }

    #[test]
    fn test_retry_custom() {
        assert_eq!(args_from(&["--retry", "3"]).retry, 3);
    }

    #[test]
    fn test_quiet_flag_short_and_long() {
        assert!(args_from(&["-q"]).quiet);
        assert!(args_from(&["--quiet"]).quiet);
    }

    #[test]
    fn test_sdel_bare_uses_default_missing_value() {
        assert!(args_from(&["--sdel"]).sdel);
        assert!(args_from(&["-d"]).sdel);
        assert!(args_from(&["--sdel", "true"]).sdel);
    }

    #[test]
    fn test_progress_bare_uses_default_missing_value() {
        assert!(args_from(&["--progress"]).progress);
        assert!(args_from(&["-p"]).progress);
    }

    #[test]
    fn test_ascii_flag_enabled() {
        assert!(args_from(&["--ascii"]).ascii);
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let args = args_from(&[]);
        assert_eq!(args.source, PathBuf::from("."));
        assert_eq!(args.output, PathBuf::from("."));
        // --sdel and --progress are on by default.
        assert!(args.sdel);
        assert!(args.progress);
        assert!(!args.dry_run);
        assert!(!args.interactive);
        assert!(!args.quiet);
        assert!(!args.verbose);
        assert!(!args.resume);
        assert!(!args.recursive);
        assert!(!args.ascii);
        assert_eq!(args.retry, 0);
        assert_eq!(args.once, None);
        assert_eq!(args.format, OutputFormat::Mkv);
    }

    #[test]
    fn test_cli_short_flags() {
        let args = args_from(&["-s", "/tmp", "-o", "/out", "-j", "4", "-f", "mp4"]);
        assert_eq!(args.source, PathBuf::from("/tmp"));
        assert_eq!(args.output, PathBuf::from("/out"));
        assert_eq!(args.jobs, 4);
        assert_eq!(args.format, OutputFormat::Mp4);
    }

    #[test]
    fn test_cli_long_flags() {
        let args = args_from(&[
            "--dry-run",
            "--quiet",
            "--verbose",
            "--resume",
            "--retry",
            "3",
            "--sdel",
            "false",
            "--progress",
            "false",
        ]);
        assert!(args.dry_run);
        assert!(args.quiet);
        assert!(args.verbose);
        assert!(args.resume);
        assert_eq!(args.retry, 3);
        assert!(!args.sdel);
        assert!(!args.progress);
    }

    #[test]
    fn test_help_contains_translated_text() {
        let l = crate::i18n::lang();
        let mut cmd = build_cli();
        let help = cmd.render_help().to_string();
        if matches!(l, crate::i18n::Lang::Cn) {
            assert!(help.contains("批量合并"));
        } else {
            assert!(help.contains("Batch merge"));
        }
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_source_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::File::create(&file).unwrap();

        let mut args = make_args();
        args.source = file;
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validate_output_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("output.txt");
        fs::File::create(&file).unwrap();

        let mut args = make_args();
        args.source = dir.path().to_path_buf();
        args.output = file;
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validate_success() {
        let dir = tempdir().unwrap();
        let mut args = make_args();
        args.source = dir.path().to_path_buf();
        args.output = dir.path().to_path_buf();
        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_interactive_with_dry_run() {
        let mut args = make_args();
        args.interactive = true;
        args.dry_run = true;
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--interactive"));
    }

    #[test]
    fn test_validate_accepts_interactive_alone() {
        let mut args = make_args();
        args.interactive = true;
        args.dry_run = false;
        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_args_validate_rejects_nonexistent_source() {
        let mut args = make_args();
        args.source = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let result = args.validate();
        assert!(result.is_err(), "validate should reject nonexistent source");
    }

    #[test]
    fn test_validate_rejects_mp4_same_source_output() {
        let dir = tempdir().unwrap();
        let mut args = make_args();
        args.source = dir.path().to_path_buf();
        args.output = dir.path().to_path_buf();
        args.format = OutputFormat::Mp4;
        let result = args.validate();
        assert!(result.is_err(), "should reject mp4 when source == output");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("overwrite") || err_msg.contains("覆盖"));
    }

    #[test]
    fn test_validate_allows_mkv_same_source_output() {
        let dir = tempdir().unwrap();
        let mut args = make_args();
        args.source = dir.path().to_path_buf();
        args.output = dir.path().to_path_buf();
        args.format = OutputFormat::Mkv;
        // mkv extension differs from mp4 input, so no overwrite risk
        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_validate_allows_mp4_different_dirs() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let mut args = make_args();
        args.source = dir1.path().to_path_buf();
        args.output = dir2.path().to_path_buf();
        args.format = OutputFormat::Mp4;
        assert!(
            args.validate().is_ok(),
            "mp4 with different dirs should be allowed"
        );
    }

    #[test]
    fn test_validate_rejects_verbose_with_quiet() {
        let mut args = make_args();
        args.verbose = true;
        args.quiet = true;
        let result = args.validate();
        assert!(result.is_err(), "should reject --verbose with --quiet");
    }

    #[test]
    fn test_validate_accepts_verbose_alone() {
        let mut args = make_args();
        args.verbose = true;
        args.quiet = false;
        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_validate_accepts_quiet_alone() {
        let mut args = make_args();
        args.verbose = false;
        args.quiet = true;
        assert!(args.validate().is_ok());
    }
}
