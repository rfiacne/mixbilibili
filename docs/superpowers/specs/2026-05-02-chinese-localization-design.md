# mixbilibili v0.6.0 Chinese Localization Design

**Date:** 2026-05-02  
**Status:** Draft  
**Scope:** Auto-detect system language, output Chinese or English accordingly

## Overview

Add a lightweight `i18n` module that detects `LANG` environment variable and translates all user-facing strings to Simplified Chinese when appropriate. Zero external dependencies.

## Architecture

### `src/i18n.rs` (new file)

```rust
#[derive(Clone, Copy)]
pub enum Lang { Cn, En }

pub fn lang() -> Lang {
    let lang = std::env::var("LANG").unwrap_or_default();
    if lang.starts_with("zh") { Lang::Cn } else { Lang::En }
}

pub fn t(key: &str) -> &'static str {
    let l = lang();
    match (key, l) {
        ("merge_report", Lang::Cn) => "合并报告",
        ("merge_report", Lang::En) => "Merge Report",
        ...
    }
}
```

Key design decisions:
- `lang()` is called once per `t()` call — negligible cost, called only in output paths (not hot path)
- Translation table is a single `match` expression — compiled to a jump table, O(1)
- `Lang` enum — type-safe, no runtime string matching after initial detection

### Translation Table (all user-facing strings)

| Key | English | Chinese |
|-----|---------|---------|
| `error_prefix` | `Error:` | `错误：` |
| `no_pairs` | `No file pairs to merge` | `没有找到可合并的文件对` |
| `all_merged` | `All files already merged from previous session` | `所有文件已在之前的会话中合并完成` |
| `processing` | `Processing {} file pairs...` | `正在处理 {} 个文件对...` |
| `dry_run_header` | `Dry-run mode — the following pairs would be merged:` | `预览模式 — 将合并以下文件对：` |
| `dry_run_sdel_header` | `The following source files would be deleted:` | `将删除以下源文件：` |
| `dry_run_summary` | `Would merge {} pair(s).` | `将合并 {} 个文件对。` |
| `dry_run_complete` | `Dry-run complete. No files were modified.` | `预览完成。未修改任何文件。` |
| `interrupted` | `Interrupted. State saved for resume.` | `已中断。状态已保存，可恢复。` |
| `merge_report` | `Merge Report` | `合并报告` |
| `succeeded_fmt` | `{} succeeded` | `成功 {} 个` |
| `failed_fmt` | `{} failed` | `失败 {} 个` |
| `merged_summary_fail` | `{}/{} merged, {} failed` | `已合并 {}/{}，失败 {} 个` |
| `merged_summary_ok` | `{}/{} merged` | `已合并 {}/{}` |
| `skipped_fmt` | `{} skipped (aria2 downloads)` | `跳过 {} 个（aria2 下载中）` |
| `orphaned_fmt` | `{} orphaned (no matching pair)` | `孤立 {} 个（无匹配文件对）` |
| `duration` | `Duration` | `耗时` |
| `avg` | `Avg` | `平均` |
| `throughput` | `Throughput` | `吞吐` |
| `deletion_failures` | `{} source file deletion failures` | `源文件删除失败 {} 个` |
| `failed_files` | `Failed files:` | `失败的文件：` |
| `dry_run_marker` | `[dry-run]` | `[预览]` |
| `retry_marker` | `retry {} {}` | `重试 {} {}` |
| `warning_prefix` | `Warning:` | `警告：` |
| `installing_ffmpeg` | `ffmpeg not found. Install it? [Y/n]` | `未找到 ffmpeg。是否安装？[Y/n]` |
| `installing` | `Installing ffmpeg...` | `正在安装 ffmpeg...` |
| `install_success` | `ffmpeg installed successfully!` | `ffmpeg 安装成功！` |
| `install_failed_notfound` | `Installation completed but ffmpeg not found in PATH. You may need to restart your terminal.` | `安装完成但 PATH 中未找到 ffmpeg。可能需要重启终端。` |
| `install_failed_exit` | `Installation failed with exit code: {}` | `安装失败，退出码：{}` |
| `install_failed_run` | `Failed to run installation: {}` | `无法运行安装程序：{}` |
| `manual_instructions` | `Please install ffmpeg manually。` | `请手动安装 ffmpeg。` |
| `running_cmd` | `Running: {}` | `执行：{}` |
| `not_dir_source` | `Source path is not a directory: {}` | `源路径不是目录：{}` |
| `not_dir_output` | `Output path exists but is not a directory: {}` | `输出路径存在但不是目录：{}` |
| `ffmpeg_not_found` | `ffmpeg not found` | `未找到 ffmpeg` |
| `merge_failed` | `{} merge(s) failed` | `{} 个合并失败` |
| `unreadable_source` | `source directory is not readable: {}` | `源目录不可读：{}` |
| `failed_to_spawn` | `Failed to spawn ffmpeg process` | `无法启动 ffmpeg 进程` |
| `failed_to_wait` | `Failed to wait for ffmpeg process` | `无法等待 ffmpeg 进程` |
| `timed_out` | `ffmpeg process timed out after 5 minutes` | `ffmpeg 进程超时（5 分钟）` |
| `failed_delete` | `Failed to delete {}` | `无法删除 {}` |
| `failed_create_output` | `Failed to create output directory` | `无法创建输出目录` |
| `failed_set_signal` | `Failed to set signal handler` | `无法设置信号处理器` |
| `failed_save_state` | `Warning: failed to save incremental state: {}` | `警告：无法保存增量状态：{}` |
| `failed_build_pool` | `Failed to build thread pool` | `无法创建线程池` |
| `separator` | `================================` | `================================` |
| `checkmark` | `✓` | `✓` |
| `cross` | `✗` | `✗` |
| `circle` | `○` | `○` |

### clap help text — builder API

所有用户可见文本包括 `--help` 都需要中文。clap derive 的 `#[arg(help = "...")]` 是编译时常量，无法运行时切换语言。因此需要从 derive API 切换到 builder API。

```rust
pub fn build_cli() -> clap::Command {
    let l = lang();
    clap::Command::new("mixbilibili")
        .version(env!("CARGO_PKG_VERSION"))
        .about(t("cli_about", l))
        .arg(clap::Arg::new("source")
            .short('s').long("source")
            .help(t("cli_source", l))
            .default_value("."))
        // ... 其余 9 个参数
}

pub fn parse_args(matches: &clap::ArgMatches) -> Args {
    Args {
        source: matches.get_one::<String>("source").unwrap().into(),
        // ...
    }
}
```

`Args` struct 保留用于类型安全访问，但不再 derive `Parser`。新增 `build_cli()` 和 `parse_args()` 函数。

翻译表新增 11 个 clap keys（`cli_about`、`cli_source`、`cli_output`、`cli_sdel`、`cli_format`、`cli_jobs`、`cli_progress`、`cli_dry_run`、`cli_quiet`、`cli_resume`、`cli_retry`）。

### File Changes Summary

| File | Change |
|------|--------|
| `src/i18n.rs` | **New** — `Lang` enum, `lang()`, `t()` function with translation table |
| `src/cli.rs` | Switch from derive to builder API, all help text translated |
| `src/main.rs` | Replace ~15 `println!`/`eprintln!` with `t()` calls |
| `src/merger.rs` | Replace ~20 user-facing strings with `t()` calls |
| `src/ffmpeg.rs` | Replace ~8 install-related strings with `t()` calls |
| `Cargo.toml` | No changes (zero dependency) |
| `README.md` | Mention language auto-detection in Features |

### Risks

- **Windows `LANG`**: Windows may not set `LANG`. On Windows, default to English (safe fallback). `std::env::var("LANG")` returns empty string if not set.
- **Terminal encoding**: Chinese characters in some Windows CMD/PowerShell terminals may render as garbled text. This is a known terminal limitation, not a code issue.
- **clap builder API**: Switching from derive to builder adds ~40 lines to `cli.rs` but enables full runtime translation. No external dependency needed.
