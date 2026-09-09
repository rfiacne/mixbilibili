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
}

/// What a run does. --once is a distinct mode, not an early-return flag.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// Scan the source directory and merge every pair found.
    Batch,
    /// Merge exactly one pair by stem, no scan, no state file.
    Once(String),
}

#[derive(Debug)]
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
    pub mode: Mode,
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
}

/// Parse ArgMatches into Args struct.
pub fn parse_args(matches: &clap::ArgMatches) -> Args {
    // The builder sets default_value for every option below, so get_one is
    // Some by construction; a None here would be a builder bug, not user input.
    let string = |key: &str| {
        matches
            .get_one::<String>(key)
            .expect("builder default")
            .clone()
    };
    let enabled = |key: &str| string(key) != "false";

    let format = match string("format").as_str() {
        "mp4" => OutputFormat::Mp4,
        "mov" => OutputFormat::Mov,
        _ => OutputFormat::Mkv,
    };

    Args {
        source: string("source").into(),
        output: string("output").into(),
        sdel: enabled("sdel"),
        format,
        jobs: matches
            .get_one::<usize>("jobs")
            .copied()
            .unwrap_or_else(default_jobs),
        progress: enabled("progress"),
        dry_run: matches.get_flag("dry_run"),
        interactive: matches.get_flag("interactive"),
        verbose: matches.get_flag("verbose"),
        quiet: matches.get_flag("quiet"),
        resume: matches.get_flag("resume"),
        retry: *matches.get_one::<usize>("retry").expect("builder default"),
        recursive: matches.get_flag("recursive"),
        mode: match matches.get_one::<String>("once") {
            Some(stem) => Mode::Once(stem.clone()),
            None => Mode::Batch,
        },
    }
}

impl Args {
    pub fn validate(&mut self) -> Result<()> {
        self.jobs = self.jobs.clamp(1, 32);

        if self.interactive && self.dry_run {
            bail!("{}", t("conflict_interactive_dry_run"));
        }

        if let Mode::Once(ref stem) = self.mode {
            // Trust boundary: the stem becomes a path component; reject
            // anything that could escape the source directory.
            if stem.is_empty() || stem.contains('/') || stem.contains('\\') || stem.contains("..") {
                bail!("{}", tf("invalid_stem", &[stem]));
            }
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

        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn make_args() -> Args {
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
        mode: Mode::Batch,
    }
}

#[cfg(test)]
mod args_tests {
    use super::*;

    #[test]
    fn test_validate_jobs_clamps_out_of_range_only() {
        let mut args = make_args();
        args.jobs = 0;
        args.validate().unwrap();
        assert_eq!(args.jobs, 1);

        let mut args = make_args();
        args.jobs = 100;
        args.validate().unwrap();
        assert_eq!(args.jobs, 32);

        // In-range values pass through untouched (make_args uses jobs = 4).
        let mut args = make_args();
        args.validate().unwrap();
        assert_eq!(args.jobs, 4);
    }

    #[test]
    fn parse_once_builds_once_mode() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "--once", "ep01"])
            .unwrap();
        let args = parse_args(&matches);
        assert_eq!(args.mode, Mode::Once("ep01".to_string()));
    }

    #[test]
    fn validate_rejects_stem_that_escapes_source() {
        for stem in ["../evil", "a/b", "a\\b", ".."] {
            let mut args = make_args();
            args.mode = Mode::Once(stem.to_string());
            assert!(
                args.validate().is_err(),
                "stem {stem:?} must be rejected before it becomes a path"
            );
        }
    }

    #[test]
    fn validate_accepts_plain_stem() {
        let mut args = make_args();
        args.mode = Mode::Once("ep01".to_string());
        args.validate().unwrap();
    }

    // Flag defaults and explicit long flags are covered by builder_tests
    // (test_build_cli_defaults / test_build_cli_long_flags). What remains
    // unique here is the bare-flag `default_missing_value` path.
    #[test]
    fn bare_optional_flags_enable_their_switch() {
        for flag in ["--sdel", "-d", "--progress", "-p"] {
            let matches = build_cli()
                .try_get_matches_from(["mixbilibili", flag])
                .unwrap();
            let args = parse_args(&matches);
            assert!(args.sdel && args.progress, "{flag} bare form must enable");
        }
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
    fn test_validate_rejects_nonexistent_source() {
        let mut args = make_args();
        args.source = PathBuf::from("/nonexistent/path/that/does/not/exist");
        assert!(args.validate().is_err());
    }
}
