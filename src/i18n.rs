use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Cn,
    En,
}

static LANG_CACHE: OnceLock<Lang> = OnceLock::new();

pub fn lang() -> Lang {
    *LANG_CACHE.get_or_init(detect_lang)
}

fn detect_lang() -> Lang {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            let output = String::from_utf8_lossy(&output.stdout);
            if output.contains("zh") {
                return Lang::Cn;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        const LANG_CHINESE: u16 = 0x04;
        unsafe {
            if GetUserDefaultUILanguage() & 0x3FF == LANG_CHINESE {
                return Lang::Cn;
            }
        }
    }

    let lang = std::env::var("LANG")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("LC_ALL").ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    if lang.starts_with("zh") {
        Lang::Cn
    } else {
        Lang::En
    }
}

type TranslationKey = (Lang, &'static str);

const TRANSLATIONS: &[(TranslationKey, &str)] = &[
    // Error / status prefixes
    ((Lang::Cn, "error_prefix"), "错误："),
    ((Lang::En, "error_prefix"), "Error:"),
    ((Lang::Cn, "warning_prefix"), "警告："),
    ((Lang::En, "warning_prefix"), "Warning:"),
    // Scan phase
    ((Lang::Cn, "no_pairs"), "没有找到可合并的文件对"),
    ((Lang::En, "no_pairs"), "No file pairs to merge"),
    ((Lang::Cn, "all_merged"), "所有文件已在之前的会话中合并完成"),
    ((Lang::En, "all_merged"), "All files already merged from previous session"),
    // Processing
    ((Lang::Cn, "processing"), "正在处理 {0} 个文件对..."),
    ((Lang::En, "processing"), "Processing {0} file pairs..."),
    // Dry-run
    ((Lang::Cn, "dry_run_header"), "预览模式 — 将合并以下文件对："),
    ((Lang::En, "dry_run_header"), "Dry-run mode — the following pairs would be merged:"),
    ((Lang::Cn, "dry_run_sdel_header"), "将删除以下源文件："),
    ((Lang::En, "dry_run_sdel_header"), "The following source files would be deleted:"),
    ((Lang::Cn, "dry_run_summary"), "将合并 {0} 个文件对。"),
    ((Lang::En, "dry_run_summary"), "Would merge {0} pair(s)."),
    ((Lang::Cn, "dry_run_complete"), "预览完成。未修改任何文件。"),
    ((Lang::En, "dry_run_complete"), "Dry-run complete. No files were modified."),
    ((Lang::Cn, "dry_run_marker"), "[预览]"),
    ((Lang::En, "dry_run_marker"), "[dry-run]"),
    // Interrupt / resume
    ((Lang::Cn, "interrupted"), "\n已中断。状态已保存，可恢复。"),
    ((Lang::En, "interrupted"), "\nInterrupted. State saved for resume."),
    // Report
    ((Lang::Cn, "merge_report"), "合并报告"),
    ((Lang::En, "merge_report"), "Merge Report"),
    ((Lang::Cn, "succeeded_fmt"), "成功 {0} 个"),
    ((Lang::En, "succeeded_fmt"), "{0} succeeded"),
    ((Lang::Cn, "failed_fmt"), "失败 {0} 个"),
    ((Lang::En, "failed_fmt"), "{0} failed"),
    ((Lang::Cn, "merged_summary_fail"), "已合并 {0}/{1}，失败 {2} 个"),
    ((Lang::En, "merged_summary_fail"), "{0}/{1} merged, {2} failed"),
    ((Lang::Cn, "merged_summary_ok"), "已合并 {0}/{1}"),
    ((Lang::En, "merged_summary_ok"), "{0}/{1} merged"),
    ((Lang::Cn, "skipped_fmt"), "{0} 个跳过（aria2 下载中）"),
    ((Lang::En, "skipped_fmt"), "{0} skipped (aria2 downloads)"),
    ((Lang::Cn, "orphaned_fmt"), "{0} 个孤立（无匹配文件对）"),
    ((Lang::En, "orphaned_fmt"), "{0} orphaned (no matching pair)"),
    ((Lang::Cn, "duration"), "耗时"),
    ((Lang::En, "duration"), "Duration"),
    ((Lang::Cn, "avg"), "平均"),
    ((Lang::En, "avg"), "Avg"),
    ((Lang::Cn, "throughput"), "吞吐"),
    ((Lang::En, "throughput"), "Throughput"),
    ((Lang::Cn, "deletion_failures"), "{0} 个源文件删除失败"),
    ((Lang::En, "deletion_failures"), "{0} source file deletion failures"),
    ((Lang::Cn, "failed_files"), "失败的文件："),
    ((Lang::En, "failed_files"), "Failed files:"),
    // Retry / markers
    ((Lang::Cn, "retry_marker"), "重试 {0} {1}"),
    ((Lang::En, "retry_marker"), "retry {0} {1}"),
    ((Lang::Cn, "verbose_retry"), "正在重试 {0}（第 {1} 次）"),
    ((Lang::En, "verbose_retry"), "Retrying {0} (attempt {1})"),
    // Symbols
    ((Lang::Cn, "separator"), "================================"),
    ((Lang::En, "separator"), "================================"),
    ((Lang::Cn, "checkmark"), "✓"),
    ((Lang::En, "checkmark"), "✓"),
    ((Lang::Cn, "cross"), "✗"),
    ((Lang::En, "cross"), "✗"),
    ((Lang::Cn, "circle"), "○"),
    ((Lang::En, "circle"), "○"),
    // ffmpeg install
    ((Lang::Cn, "installing_ffmpeg"), "未找到 ffmpeg。是否安装？[Y/n]"),
    ((Lang::En, "installing_ffmpeg"), "ffmpeg not found. Install it? [Y/n]"),
    ((Lang::Cn, "installing"), "正在安装 ffmpeg..."),
    ((Lang::En, "installing"), "Installing ffmpeg..."),
    ((Lang::Cn, "install_success"), "ffmpeg 安装成功！"),
    ((Lang::En, "install_success"), "ffmpeg installed successfully!"),
    ((Lang::Cn, "install_failed_notfound"), "安装完成但 PATH 中未找到 ffmpeg。可能需要重启终端。"),
    ((Lang::En, "install_failed_notfound"), "Installation completed but ffmpeg not found in PATH. You may need to restart your terminal."),
    ((Lang::Cn, "install_failed_exit"), "安装失败，退出码：{0}"),
    ((Lang::En, "install_failed_exit"), "Installation failed with exit code: {0}"),
    ((Lang::Cn, "install_failed_run"), "无法运行安装程序：{0}"),
    ((Lang::En, "install_failed_run"), "Failed to run installation: {0}"),
    ((Lang::Cn, "manual_instructions"), "请从 https://ffmpeg.org/download.html 手动安装 ffmpeg"),
    ((Lang::En, "manual_instructions"), "Please install ffmpeg manually from https://ffmpeg.org/download.html"),
    ((Lang::Cn, "manual_instructions_windows"), "安装 ffmpeg 的方法：\n\
             1. 使用 winget：winget install ffmpeg\n\
             2. 使用 Chocolatey：choco install ffmpeg\n\
             3. 手动下载：https://ffmpeg.org/download.html\n\
                下载 Windows 版本，解压后添加到 PATH。"),
    ((Lang::En, "manual_instructions_windows"), "To install ffmpeg manually:\n\
             1. Using winget: winget install ffmpeg\n\
             2. Using Chocolatey: choco install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html\n\
                Download the Windows build, extract, and add to PATH."),
    ((Lang::Cn, "manual_instructions_macos"), "安装 ffmpeg 的方法：\n\
             1. 使用 Homebrew：brew install ffmpeg\n\
             2. 使用 MacPorts：sudo port install ffmpeg\n\
             3. 手动下载：https://ffmpeg.org/download.html"),
    ((Lang::En, "manual_instructions_macos"), "To install ffmpeg manually:\n\
             1. Using Homebrew: brew install ffmpeg\n\
             2. Using MacPorts: sudo port install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html"),
    ((Lang::Cn, "manual_instructions_linux"), "安装 ffmpeg 的方法：\n\
             1. 使用 apt：sudo apt update && sudo apt install ffmpeg\n\
             2. 使用 dnf：sudo dnf install ffmpeg\n\
             3. 使用 yum：sudo yum install ffmpeg\n\
             4. 使用 pacman：sudo pacman -S ffmpeg\n\
             5. 使用 zypper：sudo zypper install ffmpeg\n\
             6. 手动编译：https://trac.ffmpeg.org/wiki/CompilationGuide"),
    ((Lang::En, "manual_instructions_linux"), "To install ffmpeg manually:\n\
             1. Using apt: sudo apt update && sudo apt install ffmpeg\n\
             2. Using dnf: sudo dnf install ffmpeg\n\
             3. Using yum: sudo yum install ffmpeg\n\
             4. Using pacman: sudo pacman -S ffmpeg\n\
             5. Using zypper: sudo zypper install ffmpeg\n\
             6. Manual build: https://trac.ffmpeg.org/wiki/CompilationGuide"),
    ((Lang::Cn, "running_cmd"), "执行：{0}"),
    ((Lang::En, "running_cmd"), "Running: {0}"),
    ((Lang::Cn, "install_ffmpeg_prompt"), "未找到 ffmpeg。是否通过 {0} 安装？[y/N]："),
    ((Lang::En, "install_ffmpeg_prompt"), "ffmpeg not found. Install via {0}? [y/N]: "),
    // Validation errors
    ((Lang::Cn, "not_dir_source"), "源路径不是目录：{0}"),
    ((Lang::En, "not_dir_source"), "Source path is not a directory: {0}"),
    ((Lang::Cn, "not_dir_output"), "输出路径存在但不是目录：{0}"),
    ((Lang::En, "not_dir_output"), "Output path exists but is not a directory: {0}"),
    // Merge errors
    ((Lang::Cn, "ffmpeg_not_found"), "未找到 ffmpeg"),
    ((Lang::En, "ffmpeg_not_found"), "ffmpeg not found"),
    ((Lang::Cn, "merge_failed"), "{0} 个合并失败"),
    ((Lang::En, "merge_failed"), "{0} merge(s) failed"),
    ((Lang::Cn, "unreadable_source"), "源目录不可读：{0}"),
    ((Lang::En, "unreadable_source"), "source directory is not readable: {0}"),
    ((Lang::Cn, "failed_to_spawn"), "无法启动 ffmpeg 进程"),
    ((Lang::En, "failed_to_spawn"), "Failed to spawn ffmpeg process"),
    ((Lang::Cn, "failed_to_wait"), "无法等待 ffmpeg 进程"),
    ((Lang::En, "failed_to_wait"), "Failed to wait for ffmpeg process"),
    ((Lang::Cn, "timed_out"), "ffmpeg 进程超时（5 分钟）"),
    ((Lang::En, "timed_out"), "ffmpeg process timed out after 5 minutes"),
    ((Lang::Cn, "failed_delete"), "无法删除 {0}"),
    ((Lang::En, "failed_delete"), "Failed to delete {0}"),
    ((Lang::Cn, "failed_create_output"), "无法创建输出目录"),
    ((Lang::En, "failed_create_output"), "Failed to create output directory"),
    ((Lang::Cn, "failed_set_signal"), "无法设置信号处理器"),
    ((Lang::En, "failed_set_signal"), "Failed to set signal handler"),
    ((Lang::Cn, "failed_save_state"), "警告：无法保存增量状态：{0}"),
    ((Lang::En, "failed_save_state"), "Warning: failed to save incremental state: {0}"),
    ((Lang::Cn, "failed_read_dir"), "无法读取目录"),
    ((Lang::En, "failed_read_dir"), "Failed to read directory"),
    ((Lang::Cn, "failed_read_entry"), "无法读取目录条目"),
    ((Lang::En, "failed_read_entry"), "Failed to read directory entry"),
    ((Lang::Cn, "failed_read_state"), "无法读取状态文件"),
    ((Lang::En, "failed_read_state"), "Failed to read state file"),
    ((Lang::Cn, "failed_parse_state"), "无法解析状态文件"),
    ((Lang::En, "failed_parse_state"), "Failed to parse state file"),
    ((Lang::Cn, "failed_serialize_state"), "无法序列化状态"),
    ((Lang::En, "failed_serialize_state"), "Failed to serialize state"),
    ((Lang::Cn, "failed_write_state"), "无法写入状态文件"),
    ((Lang::En, "failed_write_state"), "Failed to write state file"),
    ((Lang::Cn, "failed_remove_state"), "无法删除状态文件"),
    ((Lang::En, "failed_remove_state"), "Failed to remove state file"),
    ((Lang::Cn, "failed_build_pool"), "无法创建线程池"),
    ((Lang::En, "failed_build_pool"), "Failed to build thread pool"),
    ((Lang::Cn, "merge_failed_exit"), "ffmpeg 退出码 {0}，已重试 {1} 次"),
    ((Lang::En, "merge_failed_exit"), "ffmpeg exited with code {0} after {1} retries"),
    ((Lang::Cn, "merge_failed_io"), "{0}，已重试 {1} 次"),
    ((Lang::En, "merge_failed_io"), "{0} after {1} retries"),
    // Clap help text
    ((Lang::Cn, "cli_about"), "批量合并 Bilibili 下载的音视频文件"),
    ((Lang::En, "cli_about"), "Batch merge Bilibili downloaded video and audio files"),
    ((Lang::Cn, "cli_source"), "源目录"),
    ((Lang::En, "cli_source"), "Source directory"),
    ((Lang::Cn, "cli_output"), "输出目录"),
    ((Lang::En, "cli_output"), "Output directory"),
    ((Lang::Cn, "cli_sdel"), "合并后删除源文件"),
    ((Lang::En, "cli_sdel"), "Delete source files after merge"),
    ((Lang::Cn, "cli_format"), "输出格式（mkv/mp4/mov）"),
    ((Lang::En, "cli_format"), "Output format (mkv/mp4/mov)"),
    ((Lang::Cn, "cli_jobs"), "并行 ffmpeg 进程数"),
    ((Lang::En, "cli_jobs"), "Parallel ffmpeg processes"),
    ((Lang::Cn, "cli_progress"), "显示进度条"),
    ((Lang::En, "cli_progress"), "Show progress bar during batch operations"),
    ((Lang::Cn, "cli_dry_run"), "预览操作，不实际执行（不创建/删除文件）"),
    ((Lang::En, "cli_dry_run"), "Preview operations without executing (no files created/deleted)"),
    ((Lang::Cn, "cli_verbose"), "显示详细信息，包括 ffmpeg 命令"),
    ((Lang::En, "cli_verbose"), "Show detailed output including ffmpeg commands"),
    ((Lang::Cn, "cli_quiet"), "抑制进度输出，仅显示最终摘要"),
    ((Lang::En, "cli_quiet"), "Suppress progress output; show only final summary"),
    ((Lang::Cn, "cli_resume"), "从之前中断的批次恢复"),
    ((Lang::En, "cli_resume"), "Resume interrupted batch from previous state"),
    ((Lang::Cn, "cli_retry"), "失败合并的重试次数（0 = 不重试）"),
    ((Lang::En, "cli_retry"), "Number of retries for failed merges (0 = no retry)"),
    ((Lang::Cn, "cli_recursive"), "递归扫描子目录"),
    ((Lang::En, "cli_recursive"), "Scan subdirectories recursively"),
];

static TRANSLATION_MAP: OnceLock<HashMap<TranslationKey, &'static str>> = OnceLock::new();

fn translation_map() -> &'static HashMap<TranslationKey, &'static str> {
    TRANSLATION_MAP.get_or_init(|| TRANSLATIONS.iter().map(|&(k, v)| (k, v)).collect())
}

fn translate(lang: Lang, key: &str) -> Cow<'static, str> {
    translation_map()
        .get(&(lang, key))
        .map(|s| Cow::Borrowed(*s))
        .unwrap_or_else(|| Cow::Owned(key.to_string()))
}

pub fn t(key: &str) -> Cow<'static, str> {
    translate(lang(), key)
}

#[cfg(test)]
pub fn t_for(for_lang: Lang, key: &str) -> Cow<'static, str> {
    translate(for_lang, key)
}

pub fn tf(key: &str, args: &[&str]) -> String {
    if args.is_empty() {
        return t(key).into_owned();
    }
    let mut result = t(key).into_owned();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_cn() {
        assert_eq!(translate(Lang::Cn, "error_prefix"), "错误：");
        assert_eq!(translate(Lang::Cn, "checkmark"), "✓");
    }

    #[test]
    fn test_translate_en() {
        assert_eq!(translate(Lang::En, "error_prefix"), "Error:");
        assert_eq!(translate(Lang::En, "checkmark"), "✓");
    }

    #[test]
    fn test_translate_fallback() {
        assert_eq!(translate(Lang::En, "nonexistent_key"), "nonexistent_key");
    }

    #[test]
    fn test_t_for_bypasses_auto_detection() {
        assert_eq!(t_for(Lang::Cn, "error_prefix").as_ref(), "错误：");
        assert_eq!(t_for(Lang::En, "error_prefix").as_ref(), "Error:");
    }

    #[test]
    fn test_tf_positional_args() {
        assert_eq!(
            t_for(Lang::En, "processing"),
            "Processing {0} file pairs..."
        );
        assert_eq!(t_for(Lang::Cn, "processing"), "正在处理 {0} 个文件对...");
        let en_result = translate(Lang::En, "processing")
            .into_owned()
            .replace("{0}", "5");
        assert_eq!(en_result, "Processing 5 file pairs...");
        let cn_result = translate(Lang::Cn, "processing")
            .into_owned()
            .replace("{0}", "5");
        assert_eq!(cn_result, "正在处理 5 个文件对...");
    }

    #[test]
    fn test_tf_multiple_args() {
        assert_eq!(t_for(Lang::En, "merged_summary_ok"), "{0}/{1} merged");
    }

    #[test]
    fn test_tf_chinese() {
        assert_eq!(
            t_for(Lang::Cn, "merged_summary_fail"),
            "已合并 {0}/{1}，失败 {2} 个"
        );
    }

    #[test]
    fn test_translation_map_completeness() {
        let map = translation_map();
        assert!(!map.is_empty());
        assert!(map.contains_key(&(Lang::Cn, "error_prefix")));
        assert!(map.contains_key(&(Lang::En, "error_prefix")));
    }

    #[test]
    fn test_tf_no_placeholders_returns_translation() {
        let result = tf("checkmark", &[]);
        assert_eq!(result, "✓");
    }

    #[test]
    fn test_tf_with_one_placeholder() {
        let result = tf("succeeded_fmt", &["42"]);
        assert!(result.contains("42"), "Expected '42' in: {}", result);
        assert!(
            !result.contains("{0}"),
            "Placeholder not replaced: {}",
            result
        );
    }

    #[test]
    fn test_tf_with_multiple_placeholders() {
        let result = tf("merged_summary_fail", &["3", "5", "2"]);
        assert!(result.contains("3"), "Expected '3' in: {}", result);
        assert!(result.contains("5"), "Expected '5' in: {}", result);
        assert!(result.contains("2"), "Expected '2' in: {}", result);
        assert!(!result.contains("{0}"), "Unreplaced {{0}} in: {}", result);
        assert!(!result.contains("{1}"), "Unreplaced {{1}} in: {}", result);
        assert!(!result.contains("{2}"), "Unreplaced {{2}} in: {}", result);
    }

    #[test]
    fn test_translate_unknown_key_returns_key_name() {
        let result = t_for(Lang::En, "totally_nonexistent_key_xyz");
        assert_eq!(result.as_ref(), "totally_nonexistent_key_xyz");
    }
}
