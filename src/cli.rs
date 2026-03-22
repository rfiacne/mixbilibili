use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Supported output formats for merged files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Matroska Video container (.mkv)
    Mkv,
    /// MPEG-4 Part 14 container (.mp4)
    Mp4,
    /// QuickTime File Format (.mov)
    Mov,
}

impl OutputFormat {
    /// Parse format string, returns error if invalid.
    ///
    /// # Supported formats
    ///
    /// - `mkv` - Matroska Video
    /// - `mp4` - MPEG-4
    /// - `mov` - QuickTime
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "mkv" => Ok(Self::Mkv),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            _ => bail!("Invalid format '{}'. Supported: mkv, mp4, mov", s),
        }
    }

    /// Get file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
        }
    }

    /// Returns true if format requires `-movflags +faststart` ffmpeg flag.
    pub fn needs_faststart(&self) -> bool {
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

/// A CLI tool for batch merging Bilibili video and audio files
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Source directory containing mp4/m4a files
    #[arg(short, long, default_value = ".")]
    pub source: PathBuf,

    /// Output directory for merged files (auto-created)
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    /// Delete source files after successful merge
    #[arg(short = 'd', long, default_value_t = true)]
    pub sdel: bool,

    /// Output format: mkv, mp4, mov
    #[arg(short, long, default_value = "mkv", value_name = "FORMAT")]
    pub format: String,

    /// Number of parallel ffmpeg processes
    #[arg(short = 'j', long, default_value_t = default_jobs())]
    pub jobs: usize,
}

/// Get default number of jobs based on available parallelism
fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

impl Args {
    /// Parse and validate the format string into OutputFormat
    pub fn parsed_format(&self) -> Result<OutputFormat> {
        OutputFormat::parse(&self.format)
    }

    /// Validate and normalize arguments
    pub fn validate(&mut self) -> Result<()> {
        // Clamp jobs to valid range
        if self.jobs < 1 {
            eprintln!("Warning: jobs must be >= 1, clamping to 1");
            self.jobs = 1;
        } else if self.jobs > 32 {
            eprintln!("Warning: jobs must be <= 32, clamping to 32");
            self.jobs = 32;
        }

        // Validate source directory
        if !self.source.exists() {
            bail!("Source directory does not exist: {}", self.source.display());
        }
        if !self.source.is_dir() {
            bail!("Source path is not a directory: {}", self.source.display());
        }

        // Validate output directory (if different from source)
        if self.output != self.source {
            // Output will be created if it doesn't exist
            if self.output.exists() && !self.output.is_dir() {
                bail!(
                    "Output path exists but is not a directory: {}",
                    self.output.display()
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod args_tests {
    use super::*;

    #[test]
    fn test_validate_jobs_clamp_to_min() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 0,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 1);
    }

    #[test]
    fn test_validate_jobs_clamp_to_max() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 100,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 32);
    }

    #[test]
    fn test_validate_jobs_in_range() {
        let mut args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        args.validate().unwrap();
        assert_eq!(args.jobs, 4);
    }

    #[test]
    fn test_parsed_format_valid() {
        let args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "mp4".to_string(),
            jobs: 4,
        };
        assert_eq!(args.parsed_format().unwrap(), OutputFormat::Mp4);
    }

    #[test]
    fn test_parsed_format_invalid() {
        let args = Args {
            source: PathBuf::from("."),
            output: PathBuf::from("."),
            sdel: true,
            format: "invalid".to_string(),
            jobs: 4,
        };
        assert!(args.parsed_format().is_err());
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_source_not_exists() {
        let mut args = Args {
            source: PathBuf::from("/nonexistent/path/12345"),
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_validate_source_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::File::create(&file).unwrap();

        let mut args = Args {
            source: file,
            output: PathBuf::from("."),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_validate_output_is_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("output.txt");
        fs::File::create(&file).unwrap();

        let mut args = Args {
            source: dir.path().to_path_buf(),
            output: file,
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_validate_success() {
        let dir = tempdir().unwrap();

        let mut args = Args {
            source: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            sdel: true,
            format: "mkv".to_string(),
            jobs: 4,
        };
        let result = args.validate();
        assert!(result.is_ok());
    }
}
