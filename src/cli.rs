use clap::ValueEnum;

/// Supported output formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Mkv,
    Mp4,
    Mov,
}

impl OutputFormat {
    /// Parse format string, returns error message if invalid
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "mkv" => Ok(Self::Mkv),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            _ => Err(format!("Invalid format '{}'. Supported: mkv, mp4, mov", s)),
        }
    }

    /// Get file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
        }
    }

    /// Returns true if format requires -movflags +faststart
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
        assert_eq!(result.unwrap_err(), "Invalid format 'avi'. Supported: mkv, mp4, mov");
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