use anyhow::{bail, Result};
use clap::{Arg, ArgAction, Command};
use std::path::PathBuf;

use crate::i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub verbose: bool,
    pub quiet: bool,
    pub resume: bool,
    pub retry: usize,
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
}

/// Parse ArgMatches into Args struct.
pub fn parse_args(matches: &clap::ArgMatches) -> Args {
    let jobs = matches
        .get_one::<usize>("jobs")
        .copied()
        .unwrap_or_else(default_jobs);
    let retry = matches.get_one::<usize>("retry").copied().unwrap_or(0);

    let format_str = matches
        .get_one::<String>("format")
        .map(|s| s.as_str())
        .unwrap_or("mkv");
    let format = match format_str {
        "mp4" => OutputFormat::Mp4,
        "mov" => OutputFormat::Mov,
        _ => OutputFormat::Mkv,
    };

    Args {
        source: matches
            .get_one::<String>("source")
            .map(|s| s.into())
            .unwrap_or_else(|| PathBuf::from(".")),
        output: matches
            .get_one::<String>("output")
            .map(|s| s.into())
            .unwrap_or_else(|| PathBuf::from(".")),
        sdel: matches
            .get_one::<String>("sdel")
            .map(|s| s != "false")
            .unwrap_or(true),
        format,
        jobs,
        progress: matches
            .get_one::<String>("progress")
            .map(|s| s != "false")
            .unwrap_or(true),
        dry_run: matches.get_flag("dry_run"),
        verbose: matches.get_flag("verbose"),
        quiet: matches.get_flag("quiet"),
        resume: matches.get_flag("resume"),
        retry,
    }
}

impl Args {
    pub fn validate(&mut self) -> Result<()> {
        self.jobs = self.jobs.clamp(1, 32);

        if !self.source.is_dir() {
            bail!(
                "{}",
                t("not_dir_source").replace("{0}", &self.source.display().to_string())
            );
        }

        if self.output.exists() && !self.output.is_dir() {
            bail!(
                "{}",
                t("not_dir_output").replace("{0}", &self.output.display().to_string())
            );
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
        verbose: false,
        quiet: false,
        resume: false,
        retry: 0,
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
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert!(!args.dry_run);
    }

    #[test]
    fn test_dry_run_flag_enabled() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--dry-run"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.dry_run);
    }

    #[test]
    fn test_verbose_flag_default() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert!(!args.verbose);
    }

    #[test]
    fn test_verbose_flag_enabled() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--verbose"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.verbose);
    }

    #[test]
    fn test_resume_flag_default() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert!(!args.resume);
    }

    #[test]
    fn test_resume_flag_enabled() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--resume"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.resume);
    }

    #[test]
    fn test_retry_default() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert_eq!(args.retry, 0);
    }

    #[test]
    fn test_retry_custom() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--retry", "3"])
            .unwrap();
        let args = parse_args(&matches);
        assert_eq!(args.retry, 3);
    }

    #[test]
    fn test_quiet_flag_default() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert!(!args.quiet);
    }

    #[test]
    fn test_quiet_flag_enabled() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "-q"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.quiet);
    }

    #[test]
    fn test_quiet_flag_long() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--quiet"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.quiet);
    }

    #[test]
    fn test_sdel_bare() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--sdel"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.sdel);
    }

    #[test]
    fn test_sdel_bare_short() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "-d"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.sdel);
    }

    #[test]
    fn test_progress_bare() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--progress"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.progress);
    }

    #[test]
    fn test_progress_bare_short() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "-p"])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.progress);
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn test_build_cli_defaults() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
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
        let matches = build_cli()
            .try_get_matches_from([
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
        let args = parse_args(&matches);
        assert_eq!(args.source, PathBuf::from("/tmp"));
        assert_eq!(args.output, PathBuf::from("/out"));
        assert_eq!(args.jobs, 4);
        assert_eq!(args.format, OutputFormat::Mp4);
    }

    #[test]
    fn test_build_cli_long_flags() {
        let matches = build_cli()
            .try_get_matches_from([
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
        let args = parse_args(&matches);
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
}
