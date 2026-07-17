# Chinese Localization (v0.6.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add auto-detecting Chinese/English localization for all user-facing strings including `--help`.

**Architecture:** New `src/i18n.rs` module with `Lang` enum, `lang()` detection via `LANG` env var, and `t()` translation function using a match expression table. Clap switches from derive to builder API for runtime-translated help text. Zero external dependencies.

**Tech Stack:** Rust 2021, clap builder API, std::env

---

### Task 1: Create `src/i18n.rs` — Lang enum, lang(), t() with full translation table

**Files:**
- Create: `src/i18n.rs`
- Modify: `src/main.rs` (add `mod i18n`)
- Test: `src/i18n.rs` (inline tests)

- [ ] **Step 1: Add `tf()` helper and multi-placeholder table entries**

Add to the bottom of `src/i18n.rs` (after the `t()` function, before the tests):

```rust
/// Format a translation with positional arguments: `tf("key", &["a", "b"])` → replaces `{0}`, `{1}`, etc.
/// For single-`{}` strings, use `t("key").replace("{}", &value)` instead.
pub fn tf(key: &str, args: &[&str]) -> String {
    let mut result = t(key).to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}
```

Usage convention:
- **One placeholder**: `t("processing").replace("{}", &count.to_string())` — uses `{}`
- **Multiple placeholders**: `tf("merged_summary_fail", &[&ok, &total, &fail])` — uses `{0}`, `{1}`, `{2}`

Update the translation table entries for multi-placeholder strings to use positional format:
```
("merged_summary_fail", Lang::Cn) => "已合并 {0}/{1}，失败 {2} 个",
("merged_summary_fail", Lang::En) => "{0}/{1} merged, {2} failed",
("merged_summary_ok", Lang::Cn) => "已合并 {0}/{1}",
("merged_summary_ok", Lang::En) => "{0}/{1} merged",
("retry_marker", Lang::Cn) => "重试 {0} {1}",
("retry_marker", Lang::En) => "retry {0} {1}",
("verbose_retry", Lang::Cn) => "正在重试 {0}（第 {1} 次）",
("verbose_retry", Lang::En) => "Retrying {0} (attempt {1})",
("install_failed_exit", Lang::Cn) => "安装失败，退出码：{0}",
("install_failed_exit", Lang::En) => "Installation failed with exit code: {0}",
("install_failed_run", Lang::Cn) => "无法运行安装程序：{0}",
("install_failed_run", Lang::En) => "Failed to run installation: {0}",
("running_cmd", Lang::Cn) => "执行：{0}",
("running_cmd", Lang::En) => "Running: {0}",
("not_dir_source", Lang::Cn) => "源路径不是目录：{0}",
("not_dir_source", Lang::En) => "Source path is not a directory: {0}",
("not_dir_output", Lang::Cn) => "输出路径存在但不是目录：{0}",
("not_dir_output", Lang::En) => "Output path exists but is not a directory: {0}",
("failed_save_state", Lang::Cn) => "警告：无法保存增量状态：{0}",
("failed_save_state", Lang::En) => "Warning: failed to save incremental state: {0}",
("failed_delete", Lang::Cn) => "无法删除 {0}",
("failed_delete", Lang::En) => "Failed to delete {0}",
("merge_failed", Lang::Cn) => "{0} 个合并失败",
("merge_failed", Lang::En) => "{0} merge(s) failed",
("unreadable_source", Lang::Cn) => "源目录不可读：{0}",
("unreadable_source", Lang::En) => "source directory is not readable: {0}",
("install_ffmpeg_prompt", Lang::Cn) => "未找到 ffmpeg。是否通过 {0} 安装？[y/N]：",
("install_ffmpeg_prompt", Lang::En) => "ffmpeg not found. Install via {0}? [y/N]: ",
("timed_out", Lang::Cn) => "ffmpeg 进程超时（5 分钟）",
("timed_out", Lang::En) => "ffmpeg process timed out after 5 minutes",
("processing", Lang::Cn) => "正在处理 {} 个文件对...",
("processing", Lang::En) => "Processing {} file pairs...",
("dry_run_summary", Lang::Cn) => "将合并 {} 个文件对。",
("dry_run_summary", Lang::En) => "Would merge {} pair(s).",
("succeeded_fmt", Lang::Cn) => "成功 {} 个",
("succeeded_fmt", Lang::En) => "{} succeeded",
("failed_fmt", Lang::Cn) => "失败 {} 个",
("failed_fmt", Lang::En) => "{} failed",
("skipped_fmt", Lang::Cn) => "{} 个跳过（aria2 下载中）",
("skipped_fmt", Lang::En) => "{} skipped (aria2 downloads)",
("orphaned_fmt", Lang::Cn) => "{} 个孤立（无匹配文件对）",
("orphaned_fmt", Lang::En) => "{} orphaned (no matching pair)",
("deletion_failures", Lang::Cn) => "{} 个源文件删除失败",
("deletion_failures", Lang::En) => "{} source file deletion failures",
```

- [ ] **Step 2: Write tests for `tf()` helper**

Add to `src/i18n.rs` tests:

```rust
#[test]
fn test_tf_single_arg() {
    env::remove_var("LANG");
    assert_eq!(tf("processing", &["5"]), "Processing 5 file pairs...");
}

#[test]
fn test_tf_multiple_args() {
    env::remove_var("LANG");
    assert_eq!(tf("merged_summary_ok", &["3", "5"]), "3/5 merged");
}

#[test]
fn test_tf_chinese() {
    env::set_var("LANG", "zh_CN.UTF-8");
    assert_eq!(tf("merged_summary_fail", &["3", "5", "2"]), "已合并 3/5，失败 2 个");
    env::remove_var("LANG");
}
```

- [ ] **Step 3: Write tests for lang() detection**

Add to the bottom of the new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_lang_default_is_en() {
        // LANG not set → English
        env::remove_var("LANG");
        assert!(matches!(lang(), Lang::En));
    }

    #[test]
    fn test_lang_zh_cn() {
        env::set_var("LANG", "zh_CN.UTF-8");
        assert!(matches!(lang(), Lang::Cn));
        env::remove_var("LANG");
    }

    #[test]
    fn test_lang_zh_tw() {
        env::set_var("LANG", "zh_TW.UTF-8");
        assert!(matches!(lang(), Lang::Cn));
        env::remove_var("LANG");
    }

    #[test]
    fn test_lang_en_us() {
        env::set_var("LANG", "en_US.UTF-8");
        assert!(matches!(lang(), Lang::En));
        env::remove_var("LANG");
    }

    #[test]
    fn test_t_returns_en_for_unknown_key() {
        env::remove_var("LANG");
        let result = t("nonexistent_key");
        assert_eq!(result, "nonexistent_key");
    }

    #[test]
    fn test_t_returns_cn_for_known_key() {
        env::set_var("LANG", "zh_CN.UTF-8");
        assert_eq!(t("error_prefix"), "错误：");
        env::remove_var("LANG");
    }

    #[test]
    fn test_t_returns_en_for_known_key() {
        env::remove_var("LANG");
        assert_eq!(t("error_prefix"), "Error:");
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test i18n -- --test-threads=1`
Expected: FAIL with "cannot find module `i18n`"

- [ ] **Step 5: Create `src/i18n.rs`**

```rust
/// Detected language for user-facing output.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Cn,
    En,
}

/// Detect language from the `LANG` environment variable.
/// Returns `Lang::Cn` if LANG starts with "zh", otherwise `Lang::En`.
pub fn lang() -> Lang {
    let lang = std::env::var("LANG").unwrap_or_default();
    if lang.starts_with("zh") {
        Lang::Cn
    } else {
        Lang::En
    }
}

/// Translate a key to the current language.
/// Returns the English string as fallback for unknown keys.
pub fn t(key: &str) -> &'static str {
    let l = lang();
    match (key, l) {
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
        ("merged_summary_fail", Lang::Cn) => "已合并 {}/{}，失败 {} 个",
        ("merged_summary_fail", Lang::En) => "{}/{} merged, {} failed",
        ("merged_summary_ok", Lang::Cn) => "已合并 {}/{}",
        ("merged_summary_ok", Lang::En) => "{}/{} merged",
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
        ("retry_marker", Lang::Cn) => "重试 {} {}",
        ("retry_marker", Lang::En) => "retry {} {}",

        // --- Symbols ---
        ("separator", Lang::Cn) => "================================",
        ("separator", Lang::En) => "================================",
        ("checkmark", Lang::Cn) => "✓",
        ("checkmark", Lang::En) => "✓",
        ("cross", Lang::En) => "✗",
        ("cross", Lang::Cn) => "✗",
        ("circle", Lang::En) => "○",
        ("circle", Lang::Cn) => "○",

        // --- ffmpeg install ---
        ("installing_ffmpeg", Lang::Cn) => "未找到 ffmpeg。是否安装？[Y/n]",
        ("installing_ffmpeg", Lang::En) => "ffmpeg not found. Install it? [Y/n]",
        ("installing", Lang::Cn) => "正在安装 ffmpeg...",
        ("installing", Lang::En) => "Installing ffmpeg...",
        ("install_success", Lang::Cn) => "ffmpeg 安装成功！",
        ("install_success", Lang::En) => "ffmpeg installed successfully!",
        ("install_failed_notfound", Lang::Cn) => "安装完成但 PATH 中未找到 ffmpeg。可能需要重启终端。",
        ("install_failed_notfound", Lang::En) => "Installation completed but ffmpeg not found in PATH. You may need to restart your terminal.",
        ("install_failed_exit", Lang::Cn) => "安装失败，退出码：{}",
        ("install_failed_exit", Lang::En) => "Installation failed with exit code: {}",
        ("install_failed_run", Lang::Cn) => "无法运行安装程序：{}",
        ("install_failed_run", Lang::En) => "Failed to run installation: {}",
        ("manual_instructions", Lang::Cn) => "请手动安装 ffmpeg。",
        ("manual_instructions", Lang::En) => "Please install ffmpeg manually.",
        ("running_cmd", Lang::Cn) => "执行：{}",
        ("running_cmd", Lang::En) => "Running: {}",

        // --- Validation errors ---
        ("not_dir_source", Lang::Cn) => "源路径不是目录：{}",
        ("not_dir_source", Lang::En) => "Source path is not a directory: {}",
        ("not_dir_output", Lang::Cn) => "输出路径存在但不是目录：{}",
        ("not_dir_output", Lang::En) => "Output path exists but is not a directory: {}",

        // --- Merge errors ---
        ("ffmpeg_not_found", Lang::Cn) => "未找到 ffmpeg",
        ("ffmpeg_not_found", Lang::En) => "ffmpeg not found",
        ("merge_failed", Lang::Cn) => "{} 个合并失败",
        ("merge_failed", Lang::En) => "{} merge(s) failed",
        ("unreadable_source", Lang::Cn) => "源目录不可读：{}",
        ("unreadable_source", Lang::En) => "source directory is not readable: {}",
        ("failed_to_spawn", Lang::Cn) => "无法启动 ffmpeg 进程",
        ("failed_to_spawn", Lang::En) => "Failed to spawn ffmpeg process",
        ("failed_to_wait", Lang::Cn) => "无法等待 ffmpeg 进程",
        ("failed_to_wait", Lang::En) => "Failed to wait for ffmpeg process",
        ("timed_out", Lang::Cn) => "ffmpeg 进程超时（5 分钟）",
        ("timed_out", Lang::En) => "ffmpeg process timed out after 5 minutes",
        ("failed_delete", Lang::Cn) => "无法删除 {}",
        ("failed_delete", Lang::En) => "Failed to delete {}",
        ("failed_create_output", Lang::Cn) => "无法创建输出目录",
        ("failed_create_output", Lang::En) => "Failed to create output directory",
        ("failed_set_signal", Lang::Cn) => "无法设置信号处理器",
        ("failed_set_signal", Lang::En) => "Failed to set signal handler",
        ("failed_save_state", Lang::Cn) => "警告：无法保存增量状态：{}",
        ("failed_save_state", Lang::En) => "Warning: failed to save incremental state: {}",
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

        // Fallback: return key as-is for unknown keys
        (key, _) => key,
    }
}
```

- [ ] **Step 6: Register module in `src/main.rs`**

Add `mod i18n;` after the existing `mod` declarations at the top of `main.rs`:

```rust
mod cli;
mod ffmpeg;
mod i18n;
mod merger;
mod progress;
mod scanner;
mod state;
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test i18n -- --test-threads=1`
Expected: PASS (7 tests)

- [ ] **Step 8: Commit**

```bash
git add src/i18n.rs src/main.rs
git commit -m "feat(i18n): add Lang enum, lang() detection, t() translation table"
```

---

### Task 2: Convert `src/cli.rs` from clap derive to builder API

**Files:**
- Modify: `src/cli.rs` (replace derive with builder)
- Modify: `src/main.rs` (change Args::parse() usage)

- [ ] **Step 1: Write tests for builder API**

Add these tests to `src/cli.rs` (they test the new builder API behavior):

```rust
#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn test_build_cli_defaults() {
        let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
        let args = parse_args(&matches);
        assert_eq!(args.source, PathBuf::from("."));
        assert_eq!(args.output, PathBuf::from("."));
        assert!(args.sdel);
        assert!(!args.dry_run);
        assert!(!args.quiet);
        assert!(!args.verbose);
        assert!(!args.resume);
        assert_eq!(args.retry, 0);
    }

    #[test]
    fn test_build_cli_short_flags() {
        let matches = build_cli()
            .try_get_matches_from(["mixbilibili", "-s", "/tmp", "-o", "/out", "-j", "4", "-f", "mp4"])
            .unwrap();
        let args = parse_args(&matches);
        assert_eq!(args.source, PathBuf::from("/tmp"));
        assert_eq!(args.output, PathBuf::from("/out"));
        assert_eq!(args.jobs, 4);
        assert_eq!(args.format, OutputFormat::Mp4);
    }

    #[test]
    fn test_build_cli_long_flags() {
        let matches = build_cli()
            .try_get_matches_from([
                "mixbilibili", "--dry-run", "--quiet", "--verbose", "--resume", "--retry", "3",
                "--sdel", "false", "--progress", "false",
            ])
            .unwrap();
        let args = parse_args(&matches);
        assert!(args.dry_run);
        assert!(args.quiet);
        assert!(args.verbose);
        assert!(args.resume);
        assert_eq!(args.retry, 3);
        assert!(!args.sdel);
        assert!(!args.progress);
    }

    #[test]
    fn test_help_contains_translated_text() {
        let l = crate::i18n::lang();
        let cmd = build_cli();
        let help = cmd.render_help().to_string();
        if matches!(l, crate::i18n::Lang::Cn) {
            assert!(help.contains("批量合并"));
        } else {
            assert!(help.contains("Batch merge"));
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test builder_tests -- --test-threads=1`
Expected: FAIL — `build_cli` and `parse_args` don't exist yet

- [ ] **Step 3: Replace clap derive with builder in `src/cli.rs`**

Remove the `Parser` derive and `#[command]` attribute from `Args`, replace the entire `Args` struct and add `build_cli()` / `parse_args()`:

```rust
use crate::i18n::t;
use anyhow::{bail, Result};
use clap::{Arg, ArgAction, Command};
use std::path::PathBuf;

// OutputFormat enum and its impls stay the same (no changes)

#[derive(Debug, Clone)]
pub struct Args {
    pub source: PathBuf,
    pub output: PathBuf,
    pub sdel: bool,
    pub format: OutputFormat,
    pub jobs: usize,
    pub progress: bool,
    pub dry_run: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub resume: bool,
    pub retry: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

/// Build the CLI command with translated help text.
pub fn build_cli() -> Command {
    Command::new("mixbilibili")
        .version(env!("CARGO_PKG_VERSION"))
        .about(t("cli_about"))
        .arg(
            Arg::new("source")
                .short('s')
                .long("source")
                .help(t("cli_source"))
                .default_value("."),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help(t("cli_output"))
                .default_value("."),
        )
        .arg(
            Arg::new("sdel")
                .short('d')
                .long("sdel")
                .help(t("cli_sdel"))
                .default_value("true")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("format")
                .short('f')
                .long("format")
                .help(t("cli_format"))
                .default_value("mkv")
                .value_parser(["mkv", "mp4", "mov"]),
        )
        .arg(
            Arg::new("jobs")
                .short('j')
                .long("jobs")
                .help(t("cli_jobs"))
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("progress")
                .short('p')
                .long("progress")
                .help(t("cli_progress"))
                .default_value("true")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("dry_run")
                .short('n')
                .long("dry-run")
                .help(t("cli_dry_run"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help(t("cli_verbose"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help(t("cli_quiet"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("resume")
                .short('r')
                .long("resume")
                .help(t("cli_resume"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("retry")
                .long("retry")
                .help(t("cli_retry"))
                .default_value("0")
                .value_parser(clap::value_parser!(usize)),
        )
}

/// Parse ArgMatches into Args struct.
pub fn parse_args(matches: &clap::ArgMatches) -> Args {
    let jobs = matches
        .get_one::<usize>("jobs")
        .copied()
        .unwrap_or_else(default_jobs);
    let retry = matches.get_one::<usize>("retry").copied().unwrap_or(0);

    let format_str = matches.get_one::<String>("format").map(|s| s.as_str()).unwrap_or("mkv");
    let format = match format_str {
        "mp4" => OutputFormat::Mp4,
        "mov" => OutputFormat::Mov,
        _ => OutputFormat::Mkv,
    };

    Args {
        source: matches
            .get_one::<String>("source")
            .map(|s| s.into())
            .unwrap_or_else(|| PathBuf::from(".")),
        output: matches
            .get_one::<String>("output")
            .map(|s| s.into())
            .unwrap_or_else(|| PathBuf::from(".")),
        sdel: matches
            .get_one::<String>("sdel")
            .map(|s| s != "false")
            .unwrap_or(true),
        format,
        jobs,
        progress: matches
            .get_one::<String>("progress")
            .map(|s| s != "false")
            .unwrap_or(true),
        dry_run: matches.get_flag("dry_run"),
        verbose: matches.get_flag("verbose"),
        quiet: matches.get_flag("quiet"),
        resume: matches.get_flag("resume"),
        retry,
    }
}

impl Args {
    pub fn validate(&mut self) -> Result<()> {
        self.jobs = self.jobs.clamp(1, 32);

        if !self.source.is_dir() {
            bail!("{}", t("not_dir_source").replace("{}", &self.source.display().to_string()));
        }

        if self.output.exists() && !self.output.is_dir() {
            bail!(
                "{}",
                t("not_dir_output").replace("{}", &self.output.display().to_string())
            );
        }

        Ok(())
    }
}
```

- [ ] **Step 4: Remove old clap derive code**

Delete these lines from `src/cli.rs`:
- `use clap::{Parser, ValueEnum};` → replace with `use clap::ArgAction;` (keep `ValueEnum` for OutputFormat if needed, or remove it)
- The `#[derive(Debug, Clone, Parser)]` and `#[command(version, about, long_about = None)]` lines
- The `#[arg(...)]` attributes on each field
- The `#[cfg(test)] fn make_args()` helper (replace with the new struct literal)

Actually, since the entire struct definition is replaced in Step 3, just replace the whole block from `use clap::{Parser, ValueEnum};` through the end of the `Args` struct definition.

Keep the `OutputFormat` enum and all its impls and tests unchanged.

- [ ] **Step 5: Update `make_args()` test helper**

Replace the old `make_args()` function with the new struct literal (no derive defaults):

```rust
#[cfg(test)]
fn make_args() -> Args {
    Args {
        source: PathBuf::from("."),
        output: PathBuf::from("."),
        sdel: true,
        format: OutputFormat::Mkv,
        jobs: 4,
        progress: true,
        dry_run: false,
        verbose: false,
        quiet: false,
        resume: false,
        retry: 0,
    }
}
```

- [ ] **Step 6: Remove `Parser` import from `src/main.rs`**

In `src/main.rs`, change:
```rust
use clap::Parser;
```
to:
```rust
use clap::ArgMatches;
```

Actually, remove the `clap` import entirely since we won't use it directly in main.rs.

- [ ] **Step 7: Update `init()` in `src/main.rs` to use builder API**

Replace:
```rust
fn init() -> Result<(Args, cli::OutputFormat)> {
    let mut args = Args::parse();
    args.validate()?;
    ...
}
```

With:
```rust
fn init() -> Result<(Args, cli::OutputFormat)> {
    let matches = cli::build_cli().get_matches();
    let mut args = cli::parse_args(&matches);
    args.validate()?;
    let format = args.format;
    if !ffmpeg::ensure_ffmpeg()? {
        return Err(AppError::FfmpegNotFound.into());
    }
    if !args.output.exists() {
        std::fs::create_dir_all(&args.output).context("Failed to create output directory")?;
    }
    Ok((args, format))
}
```

- [ ] **Step 8: Update existing tests to use builder API**

In `src/cli.rs`, update the existing `args_tests` module — replace all `Args::try_parse_from(...)` calls with:

```rust
let matches = build_cli().try_get_matches_from([...]).unwrap();
let args = parse_args(&matches);
```

For example:
```rust
#[test]
fn test_dry_run_flag_default() {
    let matches = build_cli().try_get_matches_from(["mixbilibili"]).unwrap();
    let args = parse_args(&matches);
    assert!(!args.dry_run);
}

#[test]
fn test_dry_run_flag_enabled() {
    let matches = build_cli()
        .try_get_matches_from(["mixbilibili", "--dry-run"])
        .unwrap();
    let args = parse_args(&matches);
    assert!(args.dry_run);
}
```

Apply the same pattern to ALL tests in `args_tests` that use `Args::try_parse_from`.

- [ ] **Step 9: Remove unused imports**

Remove `ValueEnum` from the clap import if it's no longer used. The `OutputFormat` derive had `ValueEnum` but with the builder API we parse format as a string, so remove it:

```rust
use clap::ArgAction;
```

Remove `#[derive(..., ValueEnum)]` from `OutputFormat`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
```

- [ ] **Step 10: Run tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 11: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): switch clap from derive to builder API for translated help"
```

---

### Task 3: Translate all user-facing strings in `src/main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add `use crate::i18n::t;`**

At the top of `main.rs`, after the mod declarations:
```rust
use crate::i18n::t;
```

- [ ] **Step 2: Translate `scan_and_filter` strings**

Replace:
```rust
println!(
    "{}",
    "All files already merged from previous session".green()
);
```
With:
```rust
println!("{}", t("all_merged").green());
```

Replace:
```rust
println!("{}", "No file pairs to merge".yellow());
```
With:
```rust
println!("{}", t("no_pairs").yellow());
```

- [ ] **Step 3: Translate `main()` error output**

Replace:
```rust
eprintln!("{} {}", "Error:".red(), e);
```
With:
```rust
eprintln!("{} {}", t("error_prefix").red(), e);
```

- [ ] **Step 4: Translate `run()` dry-run block**

Replace all the English strings in the dry-run block (lines ~171-203):

```rust
if args.dry_run {
    println!("{}", t("dry_run_header").cyan().bold());
    for pair in &ctx.pairs {
        println!(
            "  {} + {} -> {}.{}",
            pair.video.display(),
            pair.audio.display(),
            pair.stem,
            format.extension()
        );
    }
    if args.sdel {
        println!("\n{}", t("dry_run_sdel_header").yellow().bold());
        for pair in &ctx.pairs {
            println!(
                "  {} (video)\n  {} (audio)",
                pair.video.display(),
                pair.audio.display()
            );
        }
    }
    println!("\n{}", t("dry_run_summary").replace("{}", &ctx.pairs.len().to_string()));
    println!("{}", t("dry_run_complete").cyan());
    return Ok(());
}
```

- [ ] **Step 5: Translate interrupt message**

Replace:
```rust
println!("{}", "\nInterrupted. State saved for resume.".yellow());
```
With:
```rust
println!("{}", t("interrupted").yellow());
```

- [ ] **Step 6: Translate `execute()` strings**

Replace:
```rust
println!("Processing {} file pairs...", ctx.pairs.len());
```
With:
```rust
println!("{}", t("processing").replace("{}", &ctx.pairs.len().to_string()));
```

Replace:
```rust
eprintln!("Warning: failed to save incremental state: {e}");
```
With:
```rust
eprintln!("{}", t("failed_save_state").replace("{}", &e.to_string()));
```

- [ ] **Step 7: Translate `run()` signal handler context**

Replace:
```rust
.context("Failed to set signal handler")?;
```
With:
```rust
.context(t("failed_set_signal"))?;
```

Replace:
```rust
.context("Failed to create output directory")?;
```
With:
```rust
.context(t("failed_create_output"))?;
```

- [ ] **Step 8: Run tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "feat(i18n): translate all user-facing strings in main.rs"
```

---

### Task 4: Translate all user-facing strings in `src/merger.rs`

**Files:**
- Modify: `src/merger.rs`

- [ ] **Step 1: Add `use crate::i18n::t;`**

After existing imports:
```rust
use crate::i18n::t;
```

- [ ] **Step 2: Translate `print_report`**

Replace the entire `print_report` method. All hardcoded strings → `t()` calls:

```rust
pub fn print_report(&self, quiet: bool) {
    if quiet {
        let total = self.success_count + self.failed_count;
        if self.failed_count > 0 {
            println!(
                "{}",
                t("merged_summary_fail")
                    .replace("{}", &self.success_count.to_string())
                    // Second {} → total, third {} → failed
                    // We need a different approach for multi-placeholder strings
            );
        }
        ...
    }
    ...
}
```

Wait — `t()` returns `&'static str` with `{}` placeholders. For multi-placeholder strings, use a helper. Add this to `i18n.rs`:

```rust
/// Replace placeholders in order: {0}, {1}, {2}, ...
pub fn tf(key: &str, args: &[&str]) -> String {
    let mut result = t(key).to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}
```

But the translation table uses `{}` not `{0}`. Let me reconsider — the simplest approach is to keep the format strings using `{}` and use `replacen`:

Actually, for strings with multiple `{}` placeholders, the translation table should use positional `{0}`, `{1}`, etc. Let me update the translation table entries:

```
("merged_summary_fail", Lang::Cn) => "已合并 {0}/{1}，失败 {2} 个",
("merged_summary_fail", Lang::En) => "{0}/{1} merged, {2} failed",
("merged_summary_ok", Lang::Cn) => "已合并 {0}/{1}",
("merged_summary_ok", Lang::En) => "{0}/{1} merged",
```

And the `tf` helper:
```rust
/// Format a translation with positional arguments: tf("key", &["a", "b"]) → "a/b merged"
pub fn tf(key: &str, args: &[&str]) -> String {
    let mut result = t(key).to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}
```

Add `tf` to `src/i18n.rs` (Task 1 addition — do this before Task 4).

Now translate `print_report`:

```rust
pub fn print_report(&self, quiet: bool) {
    if quiet {
        let total = self.success_count + self.failed_count;
        if self.failed_count > 0 {
            println!(
                "{}",
                tf("merged_summary_fail", &[
                    &self.success_count.to_string(),
                    &total.to_string(),
                    &self.failed_count.to_string(),
                ])
            );
        } else {
            println!(
                "{}",
                tf("merged_summary_ok", &[
                    &self.success_count.to_string(),
                    &total.to_string(),
                ])
            );
        }
        return;
    }

    println!("{}", t("separator").bright_black());
    println!("{}", t("merge_report").cyan().bold());
    println!("{}", t("separator").bright_black());

    let success_str = format!(
        "{} {}",
        t("checkmark").green(),
        tf("succeeded_fmt", &[&self.success_count.to_string()])
    );
    let fail_str = if self.failed_count > 0 {
        format!(
            "{} {}",
            t("cross").red(),
            tf("failed_fmt", &[&self.failed_count.to_string()])
        )
        .red()
        .to_string()
    } else {
        format!(
            "{} {}",
            t("cross"),
            tf("failed_fmt", &[&self.failed_count.to_string()])
        )
    };
    println!("  {}    {}", success_str, fail_str);

    if self.skipped_count > 0 {
        println!(
            "  {} {}",
            tf("skipped_fmt", &[&self.skipped_count.to_string()]),
            "(aria2)".bright_black()
        );
    }
    if self.orphaned_count > 0 {
        println!(
            "  {} {}",
            tf("orphaned_fmt", &[&self.orphaned_count.to_string()]),
            "(orphan)".bright_black()
        );
    }

    if !self.durations.is_empty() {
        let total = self.total_duration();
        println!(
            "  {}: {}",
            t("duration").bright_black(),
            format_duration(total)
        );
        if let Some(avg) = self.avg_duration() {
            println!("  {}: {}", t("avg").bright_black(), format_duration(avg));
        }
        if let Some(tp) = self.throughput() {
            println!(
                "  {}: {:.2} pairs/sec",
                t("throughput").bright_black(),
                tp
            );
        }
    }

    if self.deletion_failures > 0 {
        println!(
            "  {} {}",
            tf("deletion_failures", &[&self.deletion_failures.to_string()]),
            "(warn)".yellow()
        );
    }

    println!("{}", t("separator").bright_black());

    if !self.failures.is_empty() {
        println!("\n{}", t("failed_files").red().bold());
        for (name, error) in &self.failures {
            println!("  {} {}: {}", t("cross").red(), name, error);
        }
        println!();
    }
}
```

- [ ] **Step 3: Translate `do_dry_run`**

Replace:
```rust
println!("{} {} [dry-run]", "○".cyan(), pair.stem);
```
With:
```rust
println!("{} {} {}", t("circle").cyan(), pair.stem, t("dry_run_marker"));
```

- [ ] **Step 4: Translate `do_merge`**

Replace the retry message:
```rust
p.set_message(&format!("retry {attempt} {}", pair.stem));
```
With:
```rust
p.set_message(&tf("retry_marker", &[&attempt.to_string(), &pair.stem]));
```

Replace:
```rust
println!("{} Retrying {} (attempt {attempt})", "↻".yellow(), pair.stem);
```
This string is not in the translation table. It's only shown in verbose mode — keep as-is or add to table. Add to table:

In `i18n.rs`, add:
```rust
("verbose_retry", Lang::Cn) => "正在重试 {}（第 {} 次）",
("verbose_retry", Lang::En) => "Retrying {} (attempt {})",
```

Then:
```rust
println!(
    "{} {}",
    "↻".yellow(),
    tf("verbose_retry", &[&pair.stem, &attempt.to_string()])
);
```

Replace the success line:
```rust
println!("{} {}", "✓".green(), pair.stem);
```
With:
```rust
println!("{} {}", t("checkmark").green(), pair.stem);
```

Replace the failure lines similarly with `t("cross")`.

- [ ] **Step 5: Translate `execute_merges` errors**

Replace:
```rust
.context("Failed to build thread pool")?;
```
With:
```rust
.context(t("failed_build_pool"))?;
```

- [ ] **Step 6: Translate `run_with_timeout`**

Replace:
```rust
.context("Failed to spawn ffmpeg process")?;
```
With:
```rust
.context(t("failed_to_spawn"))?;
```

Replace:
```rust
anyhow::bail!("ffmpeg process timed out after 5 minutes");
```
With:
```rust
anyhow::bail!("{}", t("timed_out"));
```

Replace:
```rust
Err(e).context("Failed to wait for ffmpeg process")
```
With:
```rust
Err(e).context(t("failed_to_wait"))
```

- [ ] **Step 7: Translate `delete_source_files`**

Replace:
```rust
Err(anyhow::anyhow!("Failed to delete {}", errors.join(", ")))
```
With:
```rust
Err(anyhow::anyhow!("{} {}", t("failed_delete"), errors.join(", ")))
```

- [ ] **Step 8: Update AppError display in main.rs**

In `main.rs`, the `AppError` enum uses `#[error("...")]` attributes which are compile-time constants. To translate these, override `Display` manually or translate at the call site. The simplest: keep the English `#[error(...)]` (for logs/structured errors) but translate at print time in `main()`:

In `main()`:
```rust
fn run() -> Result<()> {
    ...
    if let Err(e) = run() {
        let msg = match e.downcast_ref::<AppError>() {
            Some(AppError::FfmpegNotFound) => t("ffmpeg_not_found").to_string(),
            Some(AppError::MergeFailed { count }) => t("merge_failed").replace("{}", &count.to_string()),
            Some(AppError::UnreadableSource { path }) => t("unreadable_source").replace("{}", path),
            None => e.to_string(),
        };
        eprintln!("{} {}", t("error_prefix").red(), msg);
        std::process::exit(get_exit_code(&e));
    }
}
```

Actually, this is more complex. The `AppError` variants are used via `thiserror` which implements `Display`. For translation, the simplest approach: keep `thiserror` for type classification, but translate the display at the point of printing. Modify the error printing in `main()`:

```rust
fn main() {
    if let Err(e) = run() {
        let translated = translate_error(&e);
        eprintln!("{} {}", "Error:".red(), translated);
        std::process::exit(get_exit_code(&e));
    }
}

fn translate_error(e: &anyhow::Error) -> String {
    if let Some(app_err) = e.downcast_ref::<AppError>() {
        match app_err {
            AppError::FfmpegNotFound => t("ffmpeg_not_found").to_string(),
            AppError::MergeFailed { count } => {
                t("merge_failed").replace("{}", &count.to_string())
            }
            AppError::UnreadableSource { path } => {
                t("unreadable_source").replace("{}", path)
            }
        }
    } else {
        e.to_string()
    }
}
```

- [ ] **Step 9: Run tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 10: Commit**

```bash
git add src/merger.rs src/main.rs src/i18n.rs
git commit -m "feat(i18n): translate all user-facing strings in merger.rs"
```

---

### Task 5: Translate all user-facing strings in `src/ffmpeg.rs`

**Files:**
- Modify: `src/ffmpeg.rs`
- Modify: `src/i18n.rs` (add manual instruction translations)

- [ ] **Step 1: Add `use crate::i18n::t;`**

After existing imports in `ffmpeg.rs`.

- [ ] **Step 2: Translate `get_manual_instructions`**

The instructions are OS-specific multi-line strings. For translation, return different strings per language + OS. Update the function signature:

```rust
pub fn get_manual_instructions(os: Os) -> &'static str {
    let l = crate::i18n::lang();
    match (os, l) {
        (Os::Windows, Lang::Cn) => {
            "安装 ffmpeg 的方法：\n\
             1. 使用 winget：winget install ffmpeg\n\
             2. 使用 Chocolatey：choco install ffmpeg\n\
             3. 手动下载：https://ffmpeg.org/download.html\n\
                下载 Windows 版本，解压后添加到 PATH。"
        }
        (Os::Windows, Lang::En) => {
            "To install ffmpeg manually:\n\
             1. Using winget: winget install ffmpeg\n\
             2. Using Chocolatey: choco install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html\n\
                Download the Windows build, extract, and add to PATH."
        }
        (Os::MacOS, Lang::Cn) => {
            "安装 ffmpeg 的方法：\n\
             1. 使用 Homebrew：brew install ffmpeg\n\
             2. 使用 MacPorts：sudo port install ffmpeg\n\
             3. 手动下载：https://ffmpeg.org/download.html"
        }
        (Os::MacOS, Lang::En) => {
            "To install ffmpeg manually:\n\
             1. Using Homebrew: brew install ffmpeg\n\
             2. Using MacPorts: sudo port install ffmpeg\n\
             3. Manual download: https://ffmpeg.org/download.html"
        }
        (Os::Linux, Lang::Cn) => {
            "安装 ffmpeg 的方法：\n\
             1. 使用 apt：sudo apt update && sudo apt install ffmpeg\n\
             2. 使用 snap：sudo snap install ffmpeg\n\
             3. 手动编译：https://trac.ffmpeg.org/wiki/CompilationGuide"
        }
        (Os::Linux, Lang::En) => {
            "To install ffmpeg manually:\n\
             1. Using apt: sudo apt update && sudo apt install ffmpeg\n\
             2. Using snap: sudo snap install ffmpeg\n\
             3. Manual build: https://trac.ffmpeg.org/wiki/CompilationGuide"
        }
        (_, Lang::Cn) => "请从 https://ffmpeg.org/download.html 安装 ffmpeg",
        (_, Lang::En) => "Please install ffmpeg from https://ffmpeg.org/download.html",
    }
}
```

Import `Lang` from i18n:
```rust
use crate::i18n::Lang;
```

- [ ] **Step 3: Translate `prompt_and_install`**

Replace:
```rust
print!("ffmpeg not found. Install via {pm_name}? [y/N]: ");
```
With:
```rust
print!("{} [y/N]：", t("installing_ffmpeg").replace("{}", pm_name));
```

Wait — the original has the package manager name inline. Add a new key:
```rust
("install_ffmpeg_prompt", Lang::Cn) => "未找到 ffmpeg。是否通过 {} 安装？[y/N]：",
("install_ffmpeg_prompt", Lang::En) => "ffmpeg not found. Install via {}? [y/N]: ",
```

Then:
```rust
print!("{}", t("install_ffmpeg_prompt").replace("{}", pm_name));
```

- [ ] **Step 4: Translate `run_install`**

Replace:
```rust
println!("Running: {cmd}");
```
With:
```rust
println!("{}", t("running_cmd").replace("{}", cmd));
```

Replace:
```rust
println!("ffmpeg installed successfully!");
```
With:
```rust
println!("{}", t("install_success"));
```

Replace:
```rust
println!("Installation completed but ffmpeg not found in PATH.");
println!("You may need to restart your terminal.");
```
With:
```rust
println!("{}", t("install_failed_notfound"));
```

Replace:
```rust
println!("Installation failed with exit code: {:?}", status.code());
```
With:
```rust
println!(
    "{}",
    t("install_failed_exit")
        .replace("{}", &status.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()))
);
```

Replace:
```rust
println!("Failed to run installation: {e}");
```
With:
```rust
println!("{}", t("install_failed_run").replace("{}", &e.to_string()));
```

- [ ] **Step 5: Update `pm_tests`**

The tests in `pm_tests` check for English substrings in `get_manual_instructions`. Update them to check based on current LANG:

```rust
#[test]
fn test_get_manual_instructions_windows() {
    let instructions = get_manual_instructions(Os::Windows);
    // Check for platform identifiers (present in both languages)
    assert!(
        instructions.contains("winget")
            || instructions.contains("Chocolatey")
            || instructions.contains("choco")
    );
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src/ffmpeg.rs src/i18n.rs
git commit -m "feat(i18n): translate all user-facing strings in ffmpeg.rs"
```

---

### Task 6: Update `Cargo.toml` version to 0.6.0 and update README

**Files:**
- Modify: `Cargo.toml`
- Modify: `README.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Bump version in `Cargo.toml`**

Change:
```toml
version = "0.5.0"
```
To:
```toml
version = "0.6.0"
```

- [ ] **Step 2: Add language feature to README**

In the Features section, add:
```markdown
- **Language auto-detection**: Outputs Chinese or English based on system `LANG` environment variable
```

- [ ] **Step 3: Add v0.6.0 entry to CHANGELOG.md**

Add after the header:
```markdown
## [0.6.0] - 2026-05-02

### Added
- Automatic language detection: outputs Simplified Chinese when system `LANG` starts with `zh`
- Full translation of all user-facing strings including `--help` text
- Zero external dependencies — lightweight `i18n` module with match-based translation table

```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml README.md CHANGELOG.md
git commit -m "chore: bump version to 0.6.0, update README and CHANGELOG"
```

---

### Task 7: Final verification — build, test, run

**Files:**
- All modified files

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run formatter check**

Run: `cargo fmt --check`
Expected: No changes needed

- [ ] **Step 4: Verify Chinese output**

Run: `LANG=zh_CN.UTF-8 cargo run -- --help`
Expected: All help text in Chinese

Run: `LANG=en_US.UTF-8 cargo run -- --help`
Expected: All help text in English

Run: `LANG=zh_CN.UTF-8 cargo run -- --dry-run`
Expected: All output in Chinese

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "fix: address clippy/fmt issues from localization changes"
```
