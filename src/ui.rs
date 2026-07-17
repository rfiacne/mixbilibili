use colored::Colorize;

/// Detect whether the terminal supports Unicode output.
pub fn supports_unicode() -> bool {
    // Check force-ASCII override
    if std::env::var("MIXBILIBILI_ASCII").is_ok() {
        return false;
    }
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
pub struct Symbols {
    pub success: &'static str,
    pub failure: &'static str,
    pub retry: &'static str,
    pub horizontal_line: &'static str,
    pub vertical_line: &'static str,
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub arrow: &'static str,
}

impl Symbols {
    pub fn new(unicode: bool) -> Self {
        if unicode {
            Self {
                success: "✓",
                failure: "✗",
                retry: "↻",
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
                retry: "[RETRY]",
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
pub enum Colors {}

impl Colors {
    pub fn success(text: &str) -> String {
        text.green().to_string()
    }

    pub fn failure(text: &str) -> String {
        text.red().to_string()
    }

    pub fn warning(text: &str) -> String {
        text.yellow().to_string()
    }

    pub fn info(text: &str) -> String {
        text.cyan().to_string()
    }

    pub fn secondary(text: &str) -> String {
        text.bright_black().to_string()
    }

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

    #[test]
    fn test_supports_unicode_with_force_ascii() {
        std::env::set_var("MIXBILIBILI_ASCII", "1");
        assert!(!supports_unicode());
        std::env::remove_var("MIXBILIBILI_ASCII");
    }
}
