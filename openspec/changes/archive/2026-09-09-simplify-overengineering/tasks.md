# Tasks: simplify-overengineering

## 1. 删除死代码与未使用灵活性

- [x] 1.1 删除 `ffmpeg::get_install_command`、`ffmpeg_path` 及其对应测试（ffmpeg.rs），验证：`cargo clippy -- -D warnings` 无 unused 告警
- [x] 1.2 删除 i18n 键 `retry_marker`（Cn+En 两条），验证：`rg retry_marker src` 零命中且 clippy 通过
- [x] 1.3 删除 `MergeProgress` 手写 `Clone` impl、`Renderer::Text.completed` 字段、`test_progress_clone` 测试；`new_text` 改为 `#[cfg(test)]`，验证：`cargo test progress` 通过
- [x] 1.4 删除 `OutputFormat::parse`（含 3 个 parse 测试）与 `Display for OutputFormat`（含 2 个 Display 测试）（cli.rs），验证：`cargo test cli` 通过、`test_invalid_format` 集成测试仍通过（clap 路径不受影响）
- [x] 1.5 移除过期 lint 豁免：scanner.rs `estimated_duration` 的 `#[allow(dead_code)]`、merger.rs `merge_pair` 的 `#[allow(clippy::too_many_arguments)]`、`ffmpeg::Os` 枚举上确认无死变体后的 `#[allow(dead_code)]`（保留仍需的），验证：`cargo clippy -- -D warnings` 通过
- [x] 1.6 删除无调用点的 `Clone` derive：`Args`、`Mode`、`FilePair`、`ScanStats`，验证：`cargo build` 通过

## 2. 简化实现

- [x] 2.1 `merger::ChildExt` trait 改为自由函数 `wait_with_timeout(&mut Child, ...)`，更新 3 个 timeout 测试的调用写法，验证：`cargo test timeout` 通过
- [x] 2.2 内联 `scanner::is_downloading` 为 `aria2_files.contains(stem)`，删除其 3 个包装测试，验证：`cargo test scan` 通过
- [x] 2.3 progress.rs 进度条模板改用 indicatif 内置 `{per_sec}`/`{eta}`，删除两个 with_key 闭包（约 -30 行），验证：`cargo build` 通过、手工 `--progress` 渲染无 panic
- [x] 2.4 i18n 去掉 `TRANSLATION_MAP`/`OnceLock<HashMap>`，`translate` 改为 `TRANSLATIONS.iter().find`，验证：`cargo test i18n` 通过
- [x] 2.5 main.rs 删除 `fn line` 别名，20 处调用改回 `println!`，验证：`cargo build` 通过
- [x] 2.6 删除 `ScanStats.pairs` 字段，相关测试断言改用 `result.pairs.len()`，验证：`cargo test scanner` 通过
- [x] 2.7 删除 `MergeState.output_dir` 字段及 roundtrip 测试中对该字段的断言，验证：`cargo test state` 通过，`test_state_load_old_vec_format`（含多余键的旧文件）仍通过

## 3. 删除重复测试

- [x] 3.1 merger.rs：删除 `test_delete_source_files_video_missing` 重复份、`test_delete_source_files_both_succeed`（保留 `test_delete_source_files_success`）、`test_merge_summary_all_success_false_with_failures`（保留 `..._when_failures`）、同义反复的 `test_deletion_failures_counted_correctly`，验证：`cargo test merger` 通过且各保留一份原始用例
- [x] 3.2 state.rs：删除 `test_merge_state_clear_removes_file`（≡`test_clear_state`）与同义反复的 `test_merge_state_is_completed_false_for_unknown`，验证：`cargo test state` 通过
- [x] 3.3 main.rs：删除断言常量的 `test_exit_codes_constants`/`test_state_save_interval_constant` 与跨文件重复的 `test_resume_with_wrong_source_dir_warns`（state.rs 已有等价用例），验证：`cargo test` 通过
- [x] 3.4 cli.rs：将 12 个逐标志微测试合并为 2 个（defaults 全覆盖 + 全标志一次解析），验证：`cargo test cli` 通过

## 4. CI 与仓库文件

- [x] 4.1 `ci.yml` 的 `on:` 增加 `pull_request: branches: [master]`（并补上 ci-check 独有的 rustfmt/clippy components），删除 `.github/workflows/ci-check.yml`，验证：`Get-ChildItem .github/workflows` 仅剩 ci.yml，YAML 语法解析通过
- [x] 4.2 删除 `docs/` 整个目录（12 个历史 plan/design 文档），验证：`Test-Path docs` 为 False
- [x] 4.3 修复 `scripts/aria2-hook.sh:48`：绝对路径改为 `MIXBILIBILI_BIN="${MIXBILIBILI_BIN:-mixbilibili}"` + `"$MIXBILIBILI_BIN"` 调用，验证：`bash -n scripts/aria2-hook.sh` 语法通过（或人工复核 + rg 无 `/Users/` 残留）

## 5. 全量验证

- [x] 5.1 依次运行 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`，全部通过；统计并报告 src 净减行数
- [x] 5.2 重建 ccc 索引（`ccc index`），抽查 dry-run 冒烟：`cargo run -- --dry-run -s <tmpdir>` 正常输出预览
