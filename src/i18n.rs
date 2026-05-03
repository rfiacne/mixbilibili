//! Internationalization module — Lang enum, lang detection, t() translation table.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Cn,
    En,
}

/// Detect the user's preferred language from OS-level settings.
/// - **macOS**: `defaults read -g AppleLanguages` (system preferences)
/// - **Windows**: `GetUserDefaultUILanguage` (registry-backed)
/// - **Linux**: `LANG` / `LC_ALL` environment variables (standard)
pub fn lang() -> Lang {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            let output = String::from_utf8_lossy(&output.stdout);
            // Output looks like: ( "zh-Hans-US", "en-US" )
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
        const LANG_CHINESE: u16 = 0x04; // Primary language ID for Chinese
        unsafe {
            if GetUserDefaultUILanguage() & 0x3FF == LANG_CHINESE {
                return Lang::Cn;
            }
        }
    }

    // Linux fallback: LANG / LC_ALL environment variable
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

fn translate(lang: Lang, key: &str) -> std::borrow::Cow<'static, str> {
    let resolved = match (key, lang) {
        // --- Error / status prefixes ---
        ("error_prefix", Lang::Cn) => "错误：",
        ("error_prefix", Lang::En) => "Error:",
        ("warning_prefix", Lang::Cn) => "警告：",
        ("warning_prefix", Lang::En) => "Warning:",

        // --- Scan phase ---
        ("no_pairs", Lang::Cn) => "没有找到可合并的文件对",
        ("no_pairs", Lang::En) => "No file pairs to merge",
        ("all_merged", Lang::Cn) => "所有文件已在之前的会话中合并完成",
        ("all_merged", Lang::En) => "All files already merged from previous session",

        // --- Processing ---
        ("processing", Lang::Cn) => "正在处理 {} 个文件对...",
        ("processing", Lang::En) => "Processing {} file pairs...",

        // --- Dry-run ---
        ("dry_run_header", Lang::Cn) => "预览模式 — 将合并以下文件对：",
        ("dry_run_header", Lang::En) => "Dry-run mode — the following pairs would be merged:",
        ("dry_run_sdel_header", Lang::Cn) => "将删除以下源文件：",
        ("dry_run_sdel_header", Lang::En) => "The following source files would be deleted:",
        ("dry_run_summary", Lang::Cn) => "将合并 {} 个文件对。",
        ("dry_run_summary", Lang::En) => "Would merge {} pair(s).",
        ("dry_run_complete", Lang::Cn) => "预览完成。未修改任何文件。",
        ("dry_run_complete", Lang::En) => "Dry-run complete. No files were modified.",
        ("dry_run_marker", Lang::Cn) => "[预览]",
        ("dry_run_marker", Lang::En) => "[dry-run]",

        // --- Interrupt / resume ---
        ("interrupted", Lang::Cn) => "\n已中断。状态已保存，可恢复。",
        ("interrupted", Lang::En) => "\nInterrupted. State saved for resume.",

        // --- Report ---
        ("merge_report", Lang::Cn) => "合并报告",
        ("merge_report", Lang::En) => "Merge Report",
        ("succeeded_fmt", Lang::Cn) => "成功 {} 个",
        ("succeeded_fmt", Lang::En) => "{} succeeded",
        ("failed_fmt", Lang::Cn) => "失败 {} 个",
        ("failed_fmt", Lang::En) => "{} failed",
        ("merged_summary_fail", Lang::Cn) => "已合并 {0}/{1}，失败 {2} 个",
        ("merged_summary_fail", Lang::En) => "{0}/{1} merged, {2} failed",
        ("merged_summary_ok", Lang::Cn) => "已合并 {0}/{1}",
        ("merged_summary_ok", Lang::En) => "{0}/{1} merged",
        ("skipped_fmt", Lang::Cn) => "{} 个跳过（aria2 下载中）",
        ("skipped_fmt", Lang::En) => "{} skipped (aria2 downloads)",
        ("orphaned_fmt", Lang::Cn) => "{} 个孤立（无匹配文件对）",
        ("orphaned_fmt", Lang::En) => "{} orphaned (no matching pair)",
        ("duration", Lang::Cn) => "耗时",
        ("duration", Lang::En) => "Duration",
        ("avg", Lang::Cn) => "平均",
        ("avg", Lang::En) => "Avg",
        ("throughput", Lang::Cn) => "吞吐",
        ("throughput", Lang::En) => "Throughput",
        ("deletion_failures", Lang::Cn) => "{} 个源文件删除失败",
        ("deletion_failures", Lang::En) => "{} source file deletion failures",
        ("failed_files", Lang::Cn) => "失败的文件：",
        ("failed_files", Lang::En) => "Failed files:",

        // --- Retry / markers ---
        ("retry_marker", Lang::Cn) => "重试 {0} {1}",
        ("retry_marker", Lang::En) => "retry {0} {1}",
        ("verbose_retry", Lang::Cn) => "正在重试 {0}（第 {1} 次）",
        ("verbose_retry", Lang::En) => "Retrying {0} (attempt {1})",

        // --- Symbols (identical for both languages) ---
        ("separator", Lang::Cn | Lang::En) => "================================",
        ("checkmark", Lang::Cn | Lang::En) => "✓",
        ("cross", Lang::Cn | Lang::En) => "✗",
        ("circle", Lang::Cn | Lang::En) => "○",

        // --- ffmpeg install ---
        ("installing_ffmpeg", Lang::Cn) => "未找到 ffmpeg。是否安装？[Y/n]",
        ("installing_ffmpeg", Lang::En) => "ffmpeg not found. Install it? [Y/n]",
        ("installing", Lang::Cn) => "正在安装 ffmpeg...",
        ("installing", Lang::En) => "Installing ffmpeg...",
        ("install_success", Lang::Cn) => "ffmpeg 安装成功！",
        ("install_success", Lang::En) => "ffmpeg installed successfully!",
        ("install_failed_notfound", Lang::Cn) => "安装完成但 PATH 中未找到 ffmpeg。可能需要重启终端。",
        ("install_failed_notfound", Lang::En) => "Installation completed but ffmpeg not found in PATH. You may need to restart your terminal.",
        ("install_failed_exit", Lang::Cn) => "安装失败，退出码：{0}",
        ("install_failed_exit", Lang::En) => "Installation failed with exit code: {0}",
        ("install_failed_run", Lang::Cn) => "无法运行安装程序：{0}",
        ("install_failed_run", Lang::En) => "Failed to run installation: {0}",
        ("manual_instructions", Lang::Cn) => "请手动安装 ffmpeg。",
        ("manual_instructions", Lang::En) => "Please install ffmpeg manually.",
        ("running_cmd", Lang::Cn) => "执行：{0}",
        ("running_cmd", Lang::En) => "Running: {0}",
        ("install_ffmpeg_prompt", Lang::Cn) => "未找到 ffmpeg。是否通过 {0} 安装？[y/N]：",
        ("install_ffmpeg_prompt", Lang::En) => "ffmpeg not found. Install via {0}? [y/N]: ",

        // --- Validation errors ---
        ("not_dir_source", Lang::Cn) => "源路径不是目录：{0}",
        ("not_dir_source", Lang::En) => "Source path is not a directory: {0}",
        ("not_dir_output", Lang::Cn) => "输出路径存在但不是目录：{0}",
        ("not_dir_output", Lang::En) => "Output path exists but is not a directory: {0}",

        // --- Merge errors ---
        ("ffmpeg_not_found", Lang::Cn) => "未找到 ffmpeg",
        ("ffmpeg_not_found", Lang::En) => "ffmpeg not found",
        ("merge_failed", Lang::Cn) => "{0} 个合并失败",
        ("merge_failed", Lang::En) => "{0} merge(s) failed",
        ("unreadable_source", Lang::Cn) => "源目录不可读：{0}",
        ("unreadable_source", Lang::En) => "source directory is not readable: {0}",
        ("failed_to_spawn", Lang::Cn) => "无法启动 ffmpeg 进程",
        ("failed_to_spawn", Lang::En) => "Failed to spawn ffmpeg process",
        ("failed_to_wait", Lang::Cn) => "无法等待 ffmpeg 进程",
        ("failed_to_wait", Lang::En) => "Failed to wait for ffmpeg process",
        ("timed_out", Lang::Cn) => "ffmpeg 进程超时（5 分钟）",
        ("timed_out", Lang::En) => "ffmpeg process timed out after 5 minutes",
        ("failed_delete", Lang::Cn) => "无法删除 {0}",
        ("failed_delete", Lang::En) => "Failed to delete {0}",
        ("failed_create_output", Lang::Cn) => "无法创建输出目录",
        ("failed_create_output", Lang::En) => "Failed to create output directory",
        ("failed_set_signal", Lang::Cn) => "无法设置信号处理器",
        ("failed_set_signal", Lang::En) => "Failed to set signal handler",
        ("failed_save_state", Lang::Cn) => "警告：无法保存增量状态：{0}",
        ("failed_save_state", Lang::En) => "Warning: failed to save incremental state: {0}",
        ("failed_build_pool", Lang::Cn) => "无法创建线程池",
        ("failed_build_pool", Lang::En) => "Failed to build thread pool",

        // --- Clap help text ---
        ("cli_about", Lang::Cn) => "批量合并 Bilibili 下载的音视频文件",
        ("cli_about", Lang::En) => "Batch merge Bilibili downloaded video and audio files",
        ("cli_source", Lang::Cn) => "源目录",
        ("cli_source", Lang::En) => "Source directory",
        ("cli_output", Lang::Cn) => "输出目录",
        ("cli_output", Lang::En) => "Output directory",
        ("cli_sdel", Lang::Cn) => "合并后删除源文件",
        ("cli_sdel", Lang::En) => "Delete source files after merge",
        ("cli_format", Lang::Cn) => "输出格式（mkv/mp4/mov）",
        ("cli_format", Lang::En) => "Output format (mkv/mp4/mov)",
        ("cli_jobs", Lang::Cn) => "并行 ffmpeg 进程数",
        ("cli_jobs", Lang::En) => "Parallel ffmpeg processes",
        ("cli_progress", Lang::Cn) => "显示进度条",
        ("cli_progress", Lang::En) => "Show progress bar during batch operations",
        ("cli_dry_run", Lang::Cn) => "预览操作，不实际执行（不创建/删除文件）",
        ("cli_dry_run", Lang::En) => "Preview operations without executing (no files created/deleted)",
        ("cli_verbose", Lang::Cn) => "显示详细信息，包括 ffmpeg 命令",
        ("cli_verbose", Lang::En) => "Show detailed output including ffmpeg commands",
        ("cli_quiet", Lang::Cn) => "抑制进度输出，仅显示最终摘要",
        ("cli_quiet", Lang::En) => "Suppress progress output; show only final summary",
        ("cli_resume", Lang::Cn) => "从之前中断的批次恢复",
        ("cli_resume", Lang::En) => "Resume interrupted batch from previous state",
        ("cli_retry", Lang::Cn) => "失败合并的重试次数（0 = 不重试）",
        ("cli_retry", Lang::En) => "Number of retries for failed merges (0 = no retry)",

        // Fallback
        _ => return std::borrow::Cow::Owned(key.to_string()),
    };
    std::borrow::Cow::Borrowed(resolved)
}

/// Translate a key using auto-detected language.
pub fn t(key: &str) -> std::borrow::Cow<'static, str> {
    translate(lang(), key)
}

/// Translate a key for a specific language, bypassing auto-detection.
#[allow(dead_code)]
pub fn t_for(for_lang: Lang, key: &str) -> std::borrow::Cow<'static, str> {
    translate(for_lang, key)
}

/// Format a translation with positional arguments: `tf("key", &["a", "b"])` replaces `{0}`, `{1}`, etc.
/// For single-`{}` strings, use `t("key").replace("{}", &value)` instead.
pub fn tf(key: &str, args: &[&str]) -> String {
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
    fn test_tf_no_single_arg_fallback() {
        // tf() only replaces {0}, {1}, etc. It does NOT replace {}.
        assert_eq!(t_for(Lang::En, "processing"), "Processing {} file pairs...");
    }

    #[test]
    fn test_tf_multiple_args() {
        assert_eq!(t_for(Lang::En, "merged_summary_ok"), "{0}/{1} merged");
    }

    #[test]
    fn test_tf_chinese() {
        // Verify the Chinese template exists and has correct placeholders
        assert_eq!(
            t_for(Lang::Cn, "merged_summary_fail"),
            "已合并 {0}/{1}，失败 {2} 个"
        );
    }
}
