use crate::scanner::FilePair;
use colored::Colorize;
use std::io::Write;

/// Writes `width` spaces without allocating.
///
/// The preview aligns columns by *display* width (so CJK names line up), which
/// rules out `{:width$}`; padding is therefore written explicitly. Doing this
/// with `" ".repeat()` allocated a String per column per row.
fn write_padding(out: &mut impl Write, mut width: usize) {
    const SPACES: &[u8] = b"                                                                ";
    while width > 0 {
        let chunk = width.min(SPACES.len());
        if out.write_all(&SPACES[..chunk]).is_err() {
            return;
        }
        width -= chunk;
    }
}

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
    // Render through one buffered, locked writer. std's stdout is line-buffered,
    // so a `println!` per row cost a write() syscall per row — over 20000 rows
    // that dominated the scan phase. Buffering collapses it to a few writes.
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    write_preview_pairs(&mut out, pairs, extension, header, time_estimate);
    let _ = out.flush();
}

/// Renders the preview list into `out`. Split out from
/// [`print_preview_pairs`] so the exact layout (including CJK column
/// alignment) can be asserted in tests.
fn write_preview_pairs(
    out: &mut impl Write,
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
    let _ = writeln!(
        out,
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
        let video_pad = max_video_width.saturating_sub(video_display_width);
        let audio_pad = max_audio_width.saturating_sub(audio_display_width);

        let num = Colors::secondary(&format!("{:>width$}.", idx + 1, width = idx_width));
        let arrow = Colors::info(symbols.arrow);

        // Same layout as before, but padding is written straight to the buffer
        // instead of being materialised as a String first:
        //   {vertical} {num} {video}{pad} {arrow} {audio}{pad} {stem}.{ext}
        let _ = write!(
            out,
            "{} {} {}",
            Colors::secondary(symbols.vertical_line),
            num,
            video_name
        );
        write_padding(out, video_pad);
        let _ = write!(out, " {} {}", arrow, audio_name);
        write_padding(out, audio_pad);
        let _ = writeln!(out, " {}.{}", Colors::info(&pair.stem), extension);
    }

    // Bottom border
    let _ = writeln!(
        out,
        "{}{}",
        Colors::secondary(symbols.bottom_left),
        Colors::secondary(&border)
    );

    // Time estimate
    if let Some(msg) = time_estimate {
        let _ = writeln!(out, "{}", Colors::secondary(msg));
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
    fn test_write_padding_writes_exact_spaces() {
        // Includes widths above the internal 64-byte chunk so the chunking loop
        // is exercised, not just the single-write fast path.
        for width in [0usize, 1, 5, 63, 64, 65, 130] {
            let mut buf: Vec<u8> = Vec::new();
            write_padding(&mut buf, width);
            assert_eq!(buf.len(), width, "padding width mismatch for {width}");
            assert!(
                buf.iter().all(|&b| b == b' '),
                "padding must contain only spaces for width {width}"
            );
        }
    }

    fn pair(video: &str, audio: &str, stem: &str) -> FilePair {
        FilePair {
            video: std::path::PathBuf::from(video),
            audio: std::path::PathBuf::from(audio),
            stem: stem.to_string(),
        }
    }

    /// Renders a preview with the ASCII symbol set so the expected text is
    /// readable and independent of terminal capabilities.
    fn render_ascii_preview(pairs: &[FilePair]) -> String {
        std::env::set_var("MIXBILIBILI_ASCII", "1");
        let mut buf: Vec<u8> = Vec::new();
        write_preview_pairs(&mut buf, pairs, "mkv", "HEADER", None);
        std::env::remove_var("MIXBILIBILI_ASCII");
        String::from_utf8(buf).expect("preview output should be UTF-8")
    }

    #[test]
    fn test_preview_pads_columns_to_longest_name() {
        let pairs = vec![
            pair("ab.mp4", "ab.m4a", "ab"),
            pair("abcdef.mp4", "abcdef.m4a", "abcdef"),
        ];
        let out = render_ascii_preview(&pairs);

        // "abcdef" is the longest stem, so the short row is padded out to it.
        assert!(
            out.contains("| 1. ab.mp4     -> ab.m4a     ab.mkv"),
            "short row should be padded to the longest name, got:\n{out}"
        );
        assert!(
            out.contains("| 2. abcdef.mp4 -> abcdef.m4a abcdef.mkv"),
            "long row should have no extra padding, got:\n{out}"
        );
    }

    #[test]
    fn test_preview_aligns_cjk_by_display_width() {
        // Each CJK char is 2 columns but 3 UTF-8 bytes, so byte-count based
        // padding would misalign these rows.
        let pairs = vec![
            pair("第1集.mp4", "第1集.m4a", "第1集"),
            pair("ab.mp4", "ab.m4a", "ab"),
        ];
        let out = render_ascii_preview(&pairs);

        // "第1集.mp4" is 9 display columns; "ab.mp4" is 6, so it needs 3 spaces.
        assert!(
            out.contains("| 2. ab.mp4    -> ab.m4a    ab.mkv"),
            "ASCII row should be padded for the CJK display width, got:\n{out}"
        );
        assert!(
            out.contains("| 1. 第1集.mp4 -> 第1集.m4a 第1集.mkv"),
            "CJK row should use display width, got:\n{out}"
        );
    }

    #[test]
    fn test_preview_border_spans_row_width() {
        let pairs = vec![pair("abcdef.mp4", "ghij.m4a", "abcdef")];
        let out = render_ascii_preview(&pairs);
        let lines: Vec<&str> = out.lines().collect();

        let top = lines[0];
        let row = lines[1];
        let bottom = lines[2];

        assert!(top.starts_with("+ HEADER "), "unexpected header: {top}");
        assert!(
            row.starts_with("| 1. abcdef.mp4 -> ghij.m4a abcdef.mkv"),
            "{row}"
        );

        // The header label sits inside the top border, so top and bottom borders
        // carry the same number of dashes (the label shifts the top one right).
        let dashes = |s: &str| s.chars().filter(|c| *c == '-').count();
        assert_eq!(
            dashes(top),
            dashes(bottom),
            "top and bottom borders must be the same length:\n{out}"
        );
        assert!(
            bottom.starts_with("+-"),
            "unexpected bottom border: {bottom}"
        );
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
