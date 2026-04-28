use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

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
}

#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = ".")]
    pub source: PathBuf,

    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    #[arg(short = 'd', long, default_value_t = true)]
    pub sdel: bool,

    #[arg(short, long, default_value_t = OutputFormat::Mkv, value_name = "FORMAT")]
    pub format: OutputFormat,

    #[arg(short = 'j', long, default_value_t = default_jobs())]
    pub jobs: usize,

    /// Show progress bar during batch operations
    #[arg(short = 'p', long, default_value_t = true)]
    pub progress: bool,

    /// Preview operations without executing (no files created/deleted)
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Show detailed output including ffmpeg commands
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Resume interrupted batch from previous state
    #[arg(short = 'r', long)]
    pub resume: bool,

    /// Number of retries for failed merges (0 = no retry)
    #[arg(long, default_value_t = 0)]
    pub retry: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

impl Args {
    pub fn validate(&mut self) -> Result<()> {
        self.jobs = self.jobs.clamp(1, 32);

        if !self.source.is_dir() {
            bail!("Source path is not a directory: {}", self.source.display());
        }

        if self.output.exists() && !self.output.is_dir() {
            bail!(
                "Output path exists but is not a directory: {}",
                self.output.display()
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
        assert!(args
            .validate()
            .unwrap_err()
            .to_string()
            .contains("not a directory"));
    }

    #[test]
    fn test_validate_output_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("output.txt");
        fs::File::create(&file).unwrap();

        let mut args = make_args();
        args.source = dir.path().to_path_buf();
        args.output = file;
        assert!(args
            .validate()
            .unwrap_err()
            .to_string()
            .contains("not a directory"));
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
