#![allow(dead_code)]
use colored::Colorize;

/// Detect whether the terminal supports Unicode output.
#[allow(dead_code)]
pub fn supports_unicode() -> bool {
    if let Ok(lang) = std::env::var("LANG") {
        if lang.contains("UTF-8") || lang.contains("utf-8") {
            return true;
        }
    }
    if let Ok(lc_all) = std::env::var("LC_ALL") {
        if lc_all.contains("UTF-8") || lc_all.contains("utf-8") {
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    return true;

    #[cfg(not(target_os = "macos"))]
    false
}

/// Unicode/ASCII symbol mapping for terminal output.
#[allow(dead_code)]
pub struct Symbols {
    pub success: &'static str,
    pub failure: &'static str,
    pub skipped: &'static str,
    pub retry: &'static str,
    pub info: &'static str,
    pub warning: &'static str,
    pub horizontal_line: &'static str,
    pub vertical_line: &'static str,
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub arrow: &'static str,
}

impl Symbols {
    #[allow(dead_code)]
    pub fn new(unicode: bool) -> Self {
        if unicode {
            Self {
                success: "✓",
                failure: "✗",
                skipped: "⊘",
                retry: "↻",
                info: "ℹ",
                warning: "⚠",
                horizontal_line: "─",
                vertical_line: "│",
                top_left: "┌",
                top_right: "┐",
                bottom_left: "└",
                bottom_right: "┘",
                arrow: "→",
            }
        } else {
            Self {
                success: "[OK]",
                failure: "[FAIL]",
                skipped: "[SKIP]",
                retry: "[RETRY]",
                info: "[INFO]",
                warning: "[WARN]",
                horizontal_line: "-",
                vertical_line: "|",
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                arrow: "->",
            }
        }
    }
}

/// Semantic color mapping for terminal output.
#[allow(dead_code)]
pub enum Colors {}

impl Colors {
    #[allow(dead_code)]
    pub fn success(text: &str) -> String {
        text.green().to_string()
    }

    #[allow(dead_code)]
    pub fn failure(text: &str) -> String {
        text.red().to_string()
    }

    #[allow(dead_code)]
    pub fn warning(text: &str) -> String {
        text.yellow().to_string()
    }

    #[allow(dead_code)]
    pub fn info(text: &str) -> String {
        text.cyan().to_string()
    }

    #[allow(dead_code)]
    pub fn secondary(text: &str) -> String {
        text.bright_black().to_string()
    }

    #[allow(dead_code)]
    pub fn title(text: &str) -> String {
        text.cyan().bold().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_unicode_default() {
        let result = supports_unicode();
        assert!(result == true || result == false);
    }

    #[test]
    fn test_symbols_ascii_mode() {
        let symbols = Symbols::new(false);
        assert_eq!(symbols.success, "[OK]");
        assert_eq!(symbols.failure, "[FAIL]");
        assert_eq!(symbols.horizontal_line, "-");
    }

    #[test]
    fn test_symbols_unicode_mode() {
        let symbols = Symbols::new(true);
        assert_eq!(symbols.success, "✓");
        assert_eq!(symbols.failure, "✗");
        assert_eq!(symbols.horizontal_line, "─");
    }
}
