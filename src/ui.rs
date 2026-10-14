use colored::Colorize;

/// Detect whether the terminal supports Unicode output.
pub fn supports_unicode() -> bool {
    prefers_unicode(
        std::env::var("MIXBILIBILI_ASCII").is_ok(),
        std::env::var("LANG").ok().as_deref(),
        std::env::var("LC_ALL").ok().as_deref(),
    )
}

/// The rule, as a function of the only things it reads: an ASCII override and
/// the locale variables. Pure, so it is testable without touching the process
/// environment.
fn prefers_unicode(ascii_override: bool, lang: Option<&str>, lc_all: Option<&str>) -> bool {
    if ascii_override {
        return false;
    }
    // "C.utf8" is as valid a UTF-8 locale as "C.UTF-8", and both spellings
    // appear in the wild.
    let utf8 = |value: Option<&str>| {
        value
            .map(|v| {
                let v = v.to_lowercase();
                v.contains("utf-8") || v.contains("utf8")
            })
            .unwrap_or(false)
    };
    if utf8(lang) || utf8(lc_all) {
        return true;
    }
    // An explicitly set, non-UTF-8 locale (C, POSIX, ...) means ASCII output.
    // Only *no* locale information at all falls back to the platform default,
    // so a macOS terminal that sets LANG=C still gets ASCII symbols.
    let informed = |value: Option<&str>| value.is_some_and(|v| !v.trim().is_empty());
    if informed(lang) || informed(lc_all) {
        return false;
    }
    cfg!(target_os = "macos")
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
    /// Left/right tee, used for the separator inside a box and for list
    /// connectors so those strands stay ASCII in `--ascii` mode too.
    pub middle_left: &'static str,
    pub middle_right: &'static str,
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
                middle_left: "├",
                middle_right: "┤",
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
                middle_left: "+",
                middle_right: "+",
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

    /// Header for a destructive-action list (deletion preview).
    pub fn warning_title(text: &str) -> String {
        text.yellow().bold().to_string()
    }
}

/// Writes `width` spaces, in chunks, straight to `out`.
///
/// Box renderers pad to a shared content width; writing the spaces directly
/// avoids allocating a `String` per column per row.
pub(crate) fn write_padding(out: &mut impl std::io::Write, mut width: usize) {
    const SPACES: &[u8] = b"                                                                ";
    while width > 0 {
        let chunk = width.min(SPACES.len());
        if out.write_all(&SPACES[..chunk]).is_err() {
            return;
        }
        width -= chunk;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ascii_override_beats_everything() {
        assert!(!prefers_unicode(
            true,
            Some("zh_CN.UTF-8"),
            Some("en_US.UTF-8")
        ));
    }

    #[test]
    fn a_utf8_locale_enables_unicode_in_either_variable() {
        assert!(prefers_unicode(false, Some("zh_CN.UTF-8"), None));
        assert!(prefers_unicode(false, None, Some("en_US.utf8")));
        assert!(prefers_unicode(false, Some("C.utf8"), None));
    }

    #[test]
    fn no_locale_information_falls_back_to_the_platform() {
        assert_eq!(
            prefers_unicode(false, None, None),
            cfg!(target_os = "macos")
        );
        assert!(!prefers_unicode(false, Some("C"), Some("POSIX")));
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
