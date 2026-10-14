# Proposal: reduce-residual-complexity

## Why

上一轮 `simplify-overengineering`（已归档）后，ponytail-review 复查发现仍有约 120 行残余过度设计：`ffmpeg.rs` 自建 `Os` 枚举重复 `std::env::consts::OS`、`package_managers` 返回 Vec 但调用点只取首个元素、clap 已声明 `default_value` 的参数在 `parse_args` 中二次 `unwrap_or`（不可达分支）、重复的 `ProgressBar` Arc 包装、`format_record` 用 bool 参数复制四个着色分支、若干空转/子集重复测试。均为纯删除/简化，不改变任何用户可见行为。

## What Changes

- `ffmpeg.rs`：
  - 删除 `Os` 枚举与 `detect_os()`（cfg 三分支 + `#[allow(dead_code)]`）→ 改用 `std::env::consts::OS` 匹配 `"windows"/"macos"/"linux"`
  - `package_managers(os) -> Vec<..>`（调用点仅取 `pms[0]`）→ `first_package_manager() -> Option<(&'static str, &'static str)>`，逐候选 `which` 首个命中即返回
  - `prompt_and_install(os)` / `run_install(&[(&str,&str)])` 的 os 参数与 slice 随上述合并塌陷：`ensure_ffmpeg` 直接串联，`run_install(cmd: &str)`
  - 删除空转测试：`test_is_ffmpeg_available_does_not_panic`、`test_os_debug_format`、`test_ensure_ffmpeg_returns_true_if_available`；`os_tests` 模块随 `detect_os` 删除；`pm_tests` 改按语言键直接断言
- `cli.rs`：
  - `parse_args` 删除 clap `default_value` 之后不可达的 6 处 `unwrap_or` 兜底（source/output/format/retry/sdel/progress）
  - `make_args()` 由 `#[cfg(test)] fn` 改为 `#[cfg(test)] pub(crate) fn`，供 `main.rs` 测试复用
- `main.rs`：
  - `init()` 删除 `if !args.output.exists()` 包裹（`create_dir_all` 本身幂等）
  - `print_preview_pairs` 改为接收 `&[FilePair]`，`run_once` 的 dry-run 预览复用之（估时行留在批量路径）
  - `test_finalize_*` 两个测试的手搓 16 字段 `Args` 改用 `cli::make_args()`
- `progress.rs`：
  - `Renderer::Bar(Arc<ProgressBar>)` → `Bar(ProgressBar)`（`ProgressBar` 内部已 Arc，`Clone` 即共享状态）
  - `format_record` 删除 `colored: bool` 参数及其四个分支的 normal 复制 → 始终着色（indicatif 测宽会剥 ANSI）
- `merger.rs`：删除 5 个子集/逐字重复的 MergeSummary 测试（`test_timing_empty_summary`、`test_merge_summary_avg_duration_some_when_merges_exist`、`test_merge_summary_throughput_computes_rate`、`test_merge_summary_accumulate`、`test_merge_summary_all_success_when_no_failures`），各保留一份原始用例

## Capabilities

### New Capabilities

（无——纯删除/简化，不新增能力）

### Modified Capabilities

（无——无 spec 级行为变更：CLI 参数、退出码、输出文案、状态文件格式均不变。进度条文本模式着色差异不构成行为契约。纯重构，设 `skip_specs: true`）

## 不修改

- `StateTracker` 的 seq/save_lock 快照写协议——上一轮 design.md 已论证（锁外写文件时防旧快照覆盖新快照），维持原决定
- `i18n` 线性查表 + `OnceLock`（刻意不引依赖，属正确取舍）
- 正确性/性能问题——另走常规审查

## Impact

- 代码：`src/{cli,ffmpeg,main,progress}.rs` 与 `src/merger.rs` 测试，约 -120 行
- 依赖：无增删
- 验证：`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test` 全绿；`--dry-run` 冒烟输出不变
