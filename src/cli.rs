use anyhow::{bail, Result};
use clap::{ArgAction, Parser, ValueEnum};
use std::path::PathBuf;

use crate::i18n::{t, tf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Mkv,
    Mp4,
    Mov,
}

impl OutputFormat {
    #[cfg(test)]
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "mkv" => Ok(Self::Mkv),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            _ => bail!("Invalid format '{}'. Supported: mkv, mp4, mov", s),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_formats_lowercase() {
        assert_eq!(OutputFormat::parse("mkv").unwrap(), OutputFormat::Mkv);
        assert_eq!(OutputFormat::parse("mp4").unwrap(), OutputFormat::Mp4);
        assert_eq!(OutputFormat::parse("mov").unwrap(), OutputFormat::Mov);
    }

    #[test]
    fn test_parse_valid_formats_case_insensitive() {
        assert_eq!(OutputFormat::parse("MKV").unwrap(), OutputFormat::Mkv);
        assert_eq!(OutputFormat::parse("Mp4").unwrap(), OutputFormat::Mp4);
        assert_eq!(OutputFormat::parse("MOV").unwrap(), OutputFormat::Mov);
    }

    #[test]
    fn test_parse_invalid_format() {
        let result = OutputFormat::parse("avi");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid format"));
    }

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

/// The whole CLI interface: one field per flag, defaults declared once.
#[derive(Debug, Clone, Parser)]
#[command(name = "mixbilibili", version, about = t("cli_about"))]
pub struct Args {
    #[arg(short = 's', long, default_value = ".", help = t("cli_source"), value_name = "DIR")]
    pub source: PathBuf,

    #[arg(short = 'o', long, default_value = ".", help = t("cli_output"), value_name = "DIR")]
    pub output: PathBuf,

    #[arg(
        short = 'd',
        long,
        help = t("cli_sdel"),
        default_value_t = true,
        action = ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    pub sdel: bool,

    #[arg(short = 'f', long, default_value = "mkv", help = t("cli_format"), value_name = "FMT")]
    pub format: OutputFormat,

    #[arg(short = 'j', long, default_value_t = default_jobs(), help = t("cli_jobs"), value_name = "N")]
    pub jobs: usize,

    #[arg(
        short = 'p',
        long,
        help = t("cli_progress"),
        default_value_t = true,
        action = ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    pub progress: bool,

    #[arg(short = 'n', long, help = t("cli_dry_run"))]
    pub dry_run: bool,

    #[arg(short = 'i', long, help = t("cli_interactive"))]
    pub interactive: bool,

    #[arg(short = 'v', long, help = t("cli_verbose"))]
    pub verbose: bool,

    #[arg(short = 'q', long, help = t("cli_quiet"))]
    pub quiet: bool,

    #[arg(short = 'r', long, help = t("cli_resume"))]
    pub resume: bool,

    #[arg(long, default_value_t = 0, help = t("cli_retry"), value_name = "N")]
    pub retry: usize,

    #[arg(short = 'R', long, help = t("cli_recursive"))]
    pub recursive: bool,

    #[arg(long, help = t("cli_once"), value_name = "STEM")]
    pub once: Option<String>,

    #[arg(long, help = t("cli_ascii"))]
    pub ascii: bool,
}

/// Defaults to the machine's parallelism; validate() clamps it.
fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

/// A `--once` stem becomes a filename, so it must not carry a path, a null
/// byte, or a directory shorthand.
fn is_plain_stem(stem: &str) -> bool {
    if stem.contains('\0') {
        return false;
    }
    if stem.trim().is_empty() || stem == "." || stem == ".." {
        return false;
    }
    !stem.contains('/') && !stem.contains('\\')
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

        // Prevent source==output + mp4/mov format from overwriting source video files
        if self.format == OutputFormat::Mp4 || self.format == OutputFormat::Mov {
            let src = std::fs::canonicalize(&self.source).ok();
            let out = self
                .output
                .exists()
                .then(|| std::fs::canonicalize(&self.output).ok())
                .flatten();
            if let (Some(s), Some(o)) = (src, out) {
                if s == o {
                    bail!("{}", tf("same_source_output", &[&self.format.to_string()]));
                }
            }
        }

        if let Some(stem) = &self.once {
            if !is_plain_stem(stem) {
                bail!("{}", tf("invalid_stem", &[stem]));
            }
        }

        Ok(())
    }
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
    fn test_dry_run_flag_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert!(!args.dry_run);
    }

    #[test]
    fn test_dry_run_flag_enabled() {
        let args = Args::try_parse_from(["mixbilibili", "--dry-run"]).unwrap();
        assert!(args.dry_run);
    }

    #[test]
    fn test_verbose_flag_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert!(!args.verbose);
    }

    #[test]
    fn test_verbose_flag_enabled() {
        let args = Args::try_parse_from(["mixbilibili", "--verbose"]).unwrap();
        assert!(args.verbose);
    }

    #[test]
    fn test_resume_flag_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert!(!args.resume);
    }

    #[test]
    fn test_resume_flag_enabled() {
        let args = Args::try_parse_from(["mixbilibili", "--resume"]).unwrap();
        assert!(args.resume);
    }

    #[test]
    fn test_retry_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert_eq!(args.retry, 0);
    }

    #[test]
    fn test_retry_custom() {
        let args = Args::try_parse_from(["mixbilibili", "--retry", "3"]).unwrap();
        assert_eq!(args.retry, 3);
    }

    #[test]
    fn test_quiet_flag_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert!(!args.quiet);
    }

    #[test]
    fn test_quiet_flag_enabled() {
        let args = Args::try_parse_from(["mixbilibili", "-q"]).unwrap();
        assert!(args.quiet);
    }

    #[test]
    fn test_quiet_flag_long() {
        let args = Args::try_parse_from(["mixbilibili", "--quiet"]).unwrap();
        assert!(args.quiet);
    }

    #[test]
    fn test_sdel_bare() {
        let args = Args::try_parse_from(["mixbilibili", "--sdel"]).unwrap();
        assert!(args.sdel); // bare --sdel uses default_missing_value("true")
    }

    #[test]
    fn test_sdel_bare_short() {
        let args = Args::try_parse_from(["mixbilibili", "-d"]).unwrap();
        assert!(args.sdel); // bare -d uses default_missing_value("true")
    }

    #[test]
    fn test_sdel_explicit_true() {
        let args = Args::try_parse_from(["mixbilibili", "--sdel", "true"]).unwrap();
        assert!(args.sdel);
    }

    #[test]
    fn test_progress_bare() {
        let args = Args::try_parse_from(["mixbilibili", "--progress"]).unwrap();
        assert!(args.progress);
    }

    #[test]
    fn test_progress_bare_short() {
        let args = Args::try_parse_from(["mixbilibili", "-p"]).unwrap();
        assert!(args.progress);
    }

    #[test]
    fn test_ascii_flag_default() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert!(!args.ascii);
    }

    #[test]
    fn test_ascii_flag_enabled() {
        let args = Args::try_parse_from(["mixbilibili", "--ascii"]).unwrap();
        assert!(args.ascii);
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn test_build_cli_defaults() {
        let args = Args::try_parse_from(["mixbilibili"]).unwrap();
        assert_eq!(args.source, PathBuf::from("."));
        assert_eq!(args.output, PathBuf::from("."));
        assert!(args.sdel);
        assert!(!args.dry_run);
        assert!(!args.quiet);
        assert!(!args.verbose);
        assert!(!args.resume);
        assert_eq!(args.retry, 0);
    }

    #[test]
    fn test_build_cli_short_flags() {
        let args = Args::try_parse_from([
            "mixbilibili",
            "-s",
            "/tmp",
            "-o",
            "/out",
            "-j",
            "4",
            "-f",
            "mp4",
        ])
        .unwrap();
        assert_eq!(args.source, PathBuf::from("/tmp"));
        assert_eq!(args.output, PathBuf::from("/out"));
        assert_eq!(args.jobs, 4);
        assert_eq!(args.format, OutputFormat::Mp4);
    }

    #[test]
    fn test_build_cli_long_flags() {
        let args = Args::try_parse_from([
            "mixbilibili",
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
        ])
        .unwrap();
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
        use clap::CommandFactory;
        let l = crate::i18n::lang();
        let help = Args::command().render_help().to_string();
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
