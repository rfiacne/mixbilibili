use crate::scanner::FilePair;
use colored::Colorize;
use std::io::Write;

/// Writes `width` spaces without allocating.
///
/// The preview aligns columns by *display* width (so CJK names line up), which
/// rules out `{:width$}`; padding is therefore written explicitly. Doing this
/// with `" ".repeat()` allocated a String per column per row.
pub(crate) fn write_padding(out: &mut impl Write, mut width: usize) {
    const SPACES: &[u8] = b"                                                                ";
    while width > 0 {
        let chunk = width.min(SPACES.len());
        if out.write_all(&SPACES[..chunk]).is_err() {
            return;
        }
        width -= chunk;
    }
}

/// Writes a right-aligned, colourised row index without allocating.
///
/// `format!("{:>width$}.", i)` plus `Colors::secondary(..)` allocated twice per
/// row; the digits are formatted into a stack buffer instead.
fn write_index(out: &mut impl Write, value: usize, width: usize) {
    let mut buf = [0u8; 20];
    let mut n = value;
    let mut pos = buf.len();
    loop {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let digits = &buf[pos..];
    write_padding(out, width.saturating_sub(digits.len()));
    if let Ok(text) = std::str::from_utf8(digits) {
        let _ = write!(out, "{}.", Colors::secondary(text));
    }
}

/// The file-name component of `path` as a `Cow`, so a valid UTF-8 name is
/// borrowed instead of copied.
fn file_name_cow(p: &std::path::Path) -> std::borrow::Cow<'_, str> {
    match p.file_name() {
        Some(name) => name.to_string_lossy(),
        None => std::borrow::Cow::Owned(p.display().to_string()),
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
///
/// These return the lazily-rendered `ColoredString` rather than `String`:
/// `.to_string()` forced a heap copy of every fragment purely so it could be
/// handed straight to a `write!`, which allocated several times per preview row.
/// `colored` already emits the plain text when color is disabled (non-TTY), so
/// behaviour is unchanged.
pub enum Colors {}

impl Colors {
    pub fn success(text: &str) -> impl std::fmt::Display + '_ {
        text.green()
    }

    pub fn failure(text: &str) -> impl std::fmt::Display + '_ {
        text.red()
    }

    pub fn warning(text: &str) -> impl std::fmt::Display + '_ {
        text.yellow()
    }

    /// Yellow and bold, for a warning used as a heading.
    pub fn warning_title(text: &str) -> impl std::fmt::Display + '_ {
        text.yellow().bold()
    }

    pub fn info(text: &str) -> impl std::fmt::Display + '_ {
        text.cyan()
    }

    pub fn secondary(text: &str) -> impl std::fmt::Display + '_ {
        text.bright_black()
    }

    pub fn title(text: &str) -> impl std::fmt::Display + '_ {
        text.cyan().bold()
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
    let symbols = Symbols::new(supports_unicode());
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    write_preview_pairs(&mut out, pairs, extension, header, time_estimate, &symbols);
    let _ = out.flush();
}

/// Prints the source files that a dry run would delete, two lines per pair.
///
/// Buffered for the same reason as [`print_preview_pairs`]: this used to be a
/// `println!` per pair, and std's stdout flushes on every newline, so 20000
/// pairs cost 40000 write syscalls.
pub fn print_deletion_preview(pairs: &[FilePair], header: &str) {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    write_deletion_preview(&mut out, pairs, header);
    let _ = out.flush();
}

/// Renders the deletion list into `out`; see [`print_deletion_preview`].
fn write_deletion_preview(out: &mut impl Write, pairs: &[FilePair], header: &str) {
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", Colors::warning_title(header));
    for pair in pairs {
        let _ = writeln!(out, "  {} (video)", pair.video.display());
        let _ = writeln!(out, "  {} (audio)", pair.audio.display());
    }
}

/// Renders the preview list into `out`. Split out from
/// [`print_preview_pairs`] so the exact layout (including CJK column
/// alignment) can be asserted in tests.
///
/// The box is closed on all four sides: every line is exactly
/// `inner_width + 2` columns wide, where `inner_width` is the dash span between
/// the corners. The header sits inside the top border, and the box is widened
/// if the header is wider than the rows.
fn write_preview_pairs(
    out: &mut impl Write,
    pairs: &[FilePair],
    extension: &str,
    header: &str,
    time_estimate: Option<&str>,
    symbols: &Symbols,
) {
    use unicode_width::UnicodeWidthStr;

    // Column widths, by *display* width so CJK names line up.
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
    let max_stem_width = pairs
        .iter()
        .map(|p| UnicodeWidthStr::width(p.stem.as_str()))
        .max()
        .unwrap_or(0);

    let idx_width = pairs.len().to_string().len();
    let arrow_width = UnicodeWidthStr::width(symbols.arrow);
    let ext_width = UnicodeWidthStr::width(extension);

    // Width of "{num}. {video} {arrow} {audio} {stem}.{ext}": everything between
    // the leading space and the trailing space of a row. The stem is *not*
    // padded, so the extension stays next to it; the slack is added at the end.
    let fields_width = |stem_width: usize| {
        (idx_width + 1)
            + 1
            + max_video_width
            + 1
            + arrow_width
            + 1
            + max_audio_width
            + 1
            + stem_width
            + 1
            + ext_width
    };

    let header_width = UnicodeWidthStr::width(header);

    // A row is `vertical + " " + fields + slack + " " + vertical`, so the dash
    // span must cover the widest row's fields plus two spaces; the header needs
    // two as well. Widening to fit the header keeps all four sides aligned.
    let inner_width = (fields_width(max_stem_width) + 2).max(header_width + 2);

    let dashes = |n: usize| symbols.horizontal_line.repeat(n);

    // Top border: `corner + " " + header + " " + dashes + corner`.
    let _ = writeln!(
        out,
        "{} {} {}{}",
        Colors::secondary(symbols.top_left),
        Colors::info(header),
        Colors::secondary(&dashes(inner_width - header_width - 2)),
        Colors::secondary(symbols.top_right)
    );

    for (idx, pair) in pairs.iter().enumerate() {
        // `Cow` rather than `into_owned()`: a valid UTF-8 name (the norm) is
        // borrowed, so the row needs no allocation for either name.
        let video_name = file_name_cow(&pair.video);
        let audio_name = file_name_cow(&pair.audio);
        let video_width = UnicodeWidthStr::width(&*video_name);
        let audio_width = UnicodeWidthStr::width(&*audio_name);
        let stem_width = UnicodeWidthStr::width(pair.stem.as_str());

        let _ = write!(out, "{} ", Colors::secondary(symbols.vertical_line));
        write_index(out, idx + 1, idx_width);
        let _ = write!(out, " {}", video_name);
        write_padding(out, max_video_width.saturating_sub(video_width));
        let _ = write!(out, " {} {}", Colors::info(symbols.arrow), audio_name);
        write_padding(out, max_audio_width.saturating_sub(audio_width));
        let _ = write!(out, " {}.{extension}", Colors::info(pair.stem.as_str()));
        // Right-align the closing border, leaving the extension against the stem.
        write_padding(
            out,
            inner_width.saturating_sub(2 + fields_width(stem_width)),
        );
        let _ = writeln!(out, " {}", Colors::secondary(symbols.vertical_line));
    }

    // Bottom border
    let _ = writeln!(
        out,
        "{}{}{}",
        Colors::secondary(symbols.bottom_left),
        Colors::secondary(&dashes(inner_width)),
        Colors::secondary(symbols.bottom_right)
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

    /// Renders a preview with an explicit symbol set, so expected text is
    /// readable and independent of terminal capabilities (and no test mutates
    /// process-wide env vars that other tests read).
    fn render_preview_with(pairs: &[FilePair], unicode: bool) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_preview_pairs(
            &mut buf,
            pairs,
            "mkv",
            "HEADER",
            None,
            &Symbols::new(unicode),
        );
        String::from_utf8(buf).expect("preview output should be UTF-8")
    }

    fn render_ascii_preview(pairs: &[FilePair]) -> String {
        render_preview_with(pairs, false)
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

    /// Display width of a line, so CJK rows can be compared against ASCII
    /// borders (byte length would not line up).
    fn line_width(s: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(s)
    }

    #[test]
    fn test_preview_box_is_closed_and_aligned() {
        let pairs = vec![
            pair("ab.mp4", "ab.m4a", "ab"),
            pair(
                "a_reasonably_long_name_here.mp4",
                "a_reasonably_long_name_here.m4a",
                "a_reasonably_long_name_here",
            ),
            pair(
                "第1集_中文标题测试.mp4",
                "第1集_中文标题测试.m4a",
                "第1集_中文标题测试",
            ),
        ];
        let out = render_ascii_preview(&pairs);
        let lines: Vec<&str> = out.lines().collect();
        // top border + one row per pair + bottom border
        let box_lines = &lines[..pairs.len() + 2];

        assert!(box_lines[0].starts_with("+ HEADER "), "{out}");
        for (i, line) in box_lines.iter().enumerate() {
            assert!(
                line.ends_with('+') || line.ends_with('|'),
                "line {i} must close the box: {line}"
            );
            assert!(
                line.starts_with('+') || line.starts_with('|'),
                "line {i} must open the box: {line}"
            );
        }

        // The whole point of closing the box: every line is exactly as wide as
        // the others, including when CJK names make byte length differ.
        let widths: Vec<usize> = box_lines.iter().map(|l| line_width(l)).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "box lines must share one display width, got {widths:?}:\n{out}"
        );
    }

    #[test]
    fn test_preview_keeps_extension_next_to_stem() {
        // Slack from the widest row is added after the extension, so "x.mkv"
        // stays readable instead of being pushed to the far right.
        let pairs = vec![
            pair("x.mp4", "x.m4a", "x"),
            pair(
                "a_much_longer_name.mp4",
                "a_much_longer_name.m4a",
                "a_much_longer_name",
            ),
        ];
        let out = render_ascii_preview(&pairs);
        let row1 = out.lines().nth(1).expect("first row should exist");

        assert!(
            row1.contains(" x.mkv"),
            "extension should sit right after the stem, got:\n{row1}"
        );
        assert!(
            !row1.contains("x .mkv") && !row1.contains("x  .mkv"),
            "stem must not be padded away from its extension:\n{row1}"
        );
        assert!(
            row1.ends_with('|') && row1.trim_end_matches('|').ends_with("  "),
            "the row should be padded out to the closing border:\n{row1}"
        );
    }

    #[test]
    fn test_preview_widens_box_for_long_header() {
        // A header wider than the rows must widen the box rather than overflow.
        let pairs = vec![pair("a.mp4", "a.m4a", "a")];
        let long_header = "H".repeat(200);

        let mut buf: Vec<u8> = Vec::new();
        write_preview_pairs(
            &mut buf,
            &pairs,
            "mkv",
            &long_header,
            None,
            &Symbols::new(false),
        );

        let out = String::from_utf8(buf).expect("UTF-8");
        let widths: Vec<usize> = out.lines().map(line_width).collect();

        assert_eq!(
            widths[0], widths[1],
            "header line must match the row:\n{out}"
        );
        assert_eq!(widths[0], widths[2], "borders must match:\n{out}");
        assert!(
            widths[0] > long_header.len(),
            "box must widen past the header"
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

    fn render_deletion_preview(pairs: &[FilePair]) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_deletion_preview(&mut buf, pairs, "HEADER");
        String::from_utf8(buf).expect("deletion preview should be UTF-8")
    }

    #[test]
    fn test_deletion_preview_lists_both_sources_per_pair() {
        let pairs = vec![pair("a.mp4", "b.m4a", "a")];
        let out = render_deletion_preview(&pairs);

        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            vec!["", "HEADER", "  a.mp4 (video)", "  b.m4a (audio)"],
            "unexpected deletion preview:\n{out}"
        );
    }

    #[test]
    fn test_deletion_preview_has_one_block_per_pair() {
        let pairs: Vec<FilePair> = (0..5)
            .map(|i| pair(&format!("v{i}.mp4"), &format!("a{i}.m4a"), &format!("v{i}")))
            .collect();
        let out = render_deletion_preview(&pairs);

        // blank line + header + two lines (video, audio) per pair
        assert_eq!(out.lines().count(), 2 + pairs.len() * 2, "{out}");
        assert!(out.contains("  v4.mp4 (video)"), "{out}");
        assert!(out.contains("  a4.m4a (audio)"), "{out}");
    }
}
