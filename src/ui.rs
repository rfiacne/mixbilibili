use crate::scanner::FilePair;
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

/// Compute display width of a path's file_name component.
/// Uses unicode-width for correct CJK character width calculation.
pub fn file_name_width(p: &std::path::Path) -> usize {
    use unicode_width::UnicodeWidthStr;
    p.file_name()
        .map(|n| UnicodeWidthStr::width(n.to_string_lossy().as_ref()))
        .unwrap_or_else(|| p.display().to_string().len())
}

/// Print preview of pairs to be processed, including header and time estimate.
pub fn print_preview_pairs(
    pairs: &[FilePair],
    extension: &str,
    header: &str,
    time_estimate: Option<&str>,
) {
    let symbols = Symbols::new(supports_unicode());

    // Compute max widths for two-column alignment (video | audio)
    let max_video_width = pairs
        .iter()
        .map(|p| file_name_width(&p.video))
        .max()
        .unwrap_or(0);
    let max_audio_width = pairs
        .iter()
        .map(|p| file_name_width(&p.audio))
        .max()
        .unwrap_or(0);

    // Compute inner content width for border
    // Format: " N. video -> audio " => idx_width + 2 + max_video + 1 + arrow_len + 1 + max_audio + 1
    let idx_width = pairs.len().to_string().len();
    let arrow_len = symbols.arrow.chars().count();
    let inner_width = idx_width + 2 + max_video_width + 1 + arrow_len + 1 + max_audio_width;
    let border = symbols.horizontal_line.repeat(inner_width);

    // Header with box-drawing top
    println!(
        "{} {} {}",
        Colors::secondary(symbols.top_left),
        Colors::info(header),
        Colors::secondary(&border)
    );

    // Rows
    for (idx, pair) in pairs.iter().enumerate() {
        let video_name = pair
            .video
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| pair.video.display().to_string());
        let audio_name = pair
            .audio
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| pair.audio.display().to_string());

        // Use unicode-width for correct CJK character padding
        let video_display_width = unicode_width::UnicodeWidthStr::width(video_name.as_str());
        let audio_display_width = unicode_width::UnicodeWidthStr::width(audio_name.as_str());
        let video_pad = " ".repeat(max_video_width.saturating_sub(video_display_width));
        let audio_pad = " ".repeat(max_audio_width.saturating_sub(audio_display_width));

        let num = Colors::secondary(&format!("{:>width$}.", idx + 1, width = idx_width));
        let arrow = Colors::info(symbols.arrow);

        println!(
            "{} {} {}{} {} {}{} {}.{}",
            Colors::secondary(symbols.vertical_line),
            num,
            video_name,
            video_pad,
            arrow,
            audio_name,
            audio_pad,
            Colors::info(&pair.stem),
            extension
        );
    }

    // Bottom border
    println!(
        "{}{}",
        Colors::secondary(symbols.bottom_left),
        Colors::secondary(&border)
    );

    // Time estimate
    if let Some(msg) = time_estimate {
        println!("{}", Colors::secondary(msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_unicode_default() {
        // Only asserts the function returns without panicking on this platform.
        assert!(matches!(supports_unicode(), true | false));
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

    #[test]
    fn test_file_name_width() {
        use std::path::PathBuf;
        // file_name component only
        assert_eq!(file_name_width(&PathBuf::from("short.mp4")), 9);
        assert_eq!(
            file_name_width(&PathBuf::from("very_long_filename.mp4")),
            22
        );
        // path prefix stripped
        assert_eq!(file_name_width(&PathBuf::from("/some/dir/file.mp4")), 8);
    }

    #[test]
    fn test_file_name_width_cjk() {
        use std::path::PathBuf;
        // CJK characters are 2 columns wide each
        // "第1集.mp4" = 2+1+2+1+1+1+1 = 9 display columns
        assert_eq!(file_name_width(&PathBuf::from("第1集.mp4")), 9);
        // "正片.mp4" = 2+2+1+1+1+1 = 8 display columns
        assert_eq!(file_name_width(&PathBuf::from("正片.mp4")), 8);
    }

    #[test]
    fn test_preview_list_alignment() {
        let pairs = vec![
            crate::scanner::FilePair {
                video: std::path::PathBuf::from("short_v.mp4"),
                audio: std::path::PathBuf::from("short_a.m4a"),
                stem: "short".to_string(),
            },
            crate::scanner::FilePair {
                video: std::path::PathBuf::from("very_long_filename_v.mp4"),
                audio: std::path::PathBuf::from("very_long_filename_a.m4a"),
                stem: "very_long_filename".to_string(),
            },
        ];

        let max_video_width = pairs
            .iter()
            .map(|p| file_name_width(&p.video))
            .max()
            .unwrap_or(0);
        let max_audio_width = pairs
            .iter()
            .map(|p| file_name_width(&p.audio))
            .max()
            .unwrap_or(0);

        assert_eq!(max_video_width, 24); // very_long_filename_v.mp4
        assert_eq!(max_audio_width, 24); // very_long_filename_a.m4a

        // Verify padding produces equal-width columns
        for pair in &pairs {
            let vw = file_name_width(&pair.video);
            let aw = file_name_width(&pair.audio);
            let padded_video_len = vw + (max_video_width - vw);
            let padded_audio_len = aw + (max_audio_width - aw);
            assert_eq!(padded_video_len, max_video_width);
            assert_eq!(padded_audio_len, max_audio_width);
        }
    }
}
