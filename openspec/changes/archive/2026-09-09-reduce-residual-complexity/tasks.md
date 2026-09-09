# Tasks: reduce-residual-complexity

## 1. ffmpeg.rs 去自建 Os 与参数穿针

- [x] 1.1 删除 `Os` 枚举与 `detect_os()`，`package_managers` 改为 `first_package_manager() -> Option<(&'static str, &'static str)>`（基于 `std::env::consts::OS`，逐候选 `which` 首个命中），`get_manual_instructions` 改收 `&str` 语言键匹配，验证：`cargo check --all-targets` 无告警
- [x] 1.2 `prompt_and_install`/`run_install` 塌陷为 `ensure_ffmpeg` 主链 + `run_install(cmd: &str)`，验证：`cargo clippy -- -D warnings` 通过
- [x] 1.3 删除空转测试 `test_is_ffmpeg_available_does_not_panic`、`test_os_debug_format`、`test_ensure_ffmpeg_returns_true_if_available` 与 `os_tests` 模块，`pm_tests` 改直接断言四个指令键文案，验证：`cargo test ffmpeg` 通过

## 2. cli.rs 信任 clap 默认值

- [x] 2.1 `parse_args` 删除 source/output/format/retry/sdel/progress 六处不可达 `unwrap_or`（sdel/progress 收敛为统一 `s != "false"` 小助手），验证：`cargo test cli` 全绿（defaults/long_flags/bare_flags 三个既有测试即回归网）
- [x] 2.2 `make_args()` 改 `#[cfg(test)] pub(crate)`，验证：`cargo check --all-targets` 通过

## 3. main.rs 复用与去冗余

- [x] 3.1 `init()` 删除 `if !args.output.exists()` 包裹，直接 `create_dir_all`，验证：`cargo test` 通过
- [x] 3.2 `print_preview_pairs` 改收 `&[FilePair]`、估时打印移至调用方；`run_once` dry-run 块改调它，验证：`cargo test` 通过、`cargo run -- --dry-run --once <tmp/ep01 对>` 预览输出与改前一致
- [x] 3.3 `test_finalize_keeps_state_on_partial_failure` / `test_finalize_clears_state_on_full_success` 手搓 `Args` 改 `cli::make_args()` + 字段覆写，验证：`cargo test finalize` 通过

## 4. progress.rs 去重复包装

- [x] 4.1 `Renderer::Bar(Arc<ProgressBar>)` → `Bar(ProgressBar)`，删 `use std::sync::Arc`，验证：`cargo test progress` 通过
- [x] 4.2 `format_record` 删 `colored: bool` 参数，四分支统一着色，`Renderer::record` 两分支调用同步简化，验证：`cargo test progress` 通过；手工 `--progress`（TTY）确认条形对齐无错乱

## 5. merger.rs 删重复测试

- [x] 5.1 删除 `test_timing_empty_summary`、`test_merge_summary_avg_duration_some_when_merges_exist`、`test_merge_summary_throughput_computes_rate`、`test_merge_summary_accumulate`、`test_merge_summary_all_success_when_no_failures`，验证：`cargo test merger` 通过且保留 default/timing_methods/avg_none/throughput_none/all_success_false 各一份

## 6. 全量验证

- [x] 6.1 依次 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` 全绿
- [x] 6.2 `cargo run -- --dry-run -s <含一对文件的临时目录>` 冒烟，输出与改前一致；统计并报告 src 净减行数
- [x] 6.3 重建 ccc 索引（`uv tool run --from cocoindex-code ccc index`）
