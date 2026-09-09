# Proposal: simplify-overengineering

## Why

ponytail-review 全项目审查发现约 390 行可删除的过度设计：未被引用的死代码与翻译键、只有一个实现/调用点的抽象、重复测试、标准库或依赖已内置功能的重复实现（indicatif 手写 `per_sec`/`eta` 闭包），以及 260KB 已交付版本的历史过程文档。这些不改变任何用户可见行为，只增加维护面。

## What Changes

- 删除死代码与未使用灵活性：
  - `ffmpeg::get_install_command`（仅自测引用）、`ffmpeg_path`（同义反复测试）
  - `i18n` 键 `retry_marker`（无引用）
  - `MergeProgress` 手写 `Clone` impl（生产侧只传引用）及其测试
  - `Renderer::Text` 中只写不读的 `completed: Arc<AtomicUsize>` 字段
  - `OutputFormat::parse`（`#[cfg(test)]` 垫片，clap `value_parser` 已覆盖）与 `Display for OutputFormat`（无生产引用）及其测试
  - 过期的 `#[allow(dead_code)]` / `#[allow(clippy::too_many_arguments)]`（scanner.rs 的 `estimated_duration`、merger.rs 的 `merge_pair`）
  - 无调用点的 `Clone` derive（`Args`、`Mode`、`FilePair`、`ScanStats`）
- 简化实现：
  - `merger::ChildExt` trait（单实现单调用）→ 自由函数
  - `scanner::is_downloading`（`HashSet::contains` 一行包装）→ 内联
  - `progress.rs` 手写 `per_sec`/`eta` 两个 with_key 闭包（30 行）→ indicatif 内置 `{per_sec}` / `{eta}` 键
  - `i18n` 的 `OnceLock<HashMap>` 二级索引 → 直接线性 `find`
  - `main.rs` 的 `fn line(text: String)` println! 别名 → 直接 `println!`
  - `ScanStats.pairs` 计数器（恒等于 `pairs.len()`，生产不读）→ 删字段
  - `MergeState.output_dir` 只写不读 → 删字段（`#[serde(default)]` 保证旧状态文件仍可加载，旧文件中的多余键 serde 本就忽略）
- 合并 CI workflow：`ci-check.yml`（PR 触发）与 `ci.yml` 的 test job 步骤几乎逐行相同 → 合并为单一 workflow `on: [push, pull_request]`，删除 `ci-check.yml`
- 删除 `docs/` 下 12 个历史 plan/design 过程文档（~260KB，git 历史可找回）
- 修复 `scripts/aria2-hook.sh` 硬编码的开发者本机绝对路径 → 优先 `$MIXBILIBILI_BIN`，否则从 PATH 查找 `mixbilibili`（范围外提醒项，经用户确认纳入）

不修改：`StateTracker` 的 seq/save_lock 快照写协议（防旧快照覆盖新快照，属数据安全而非装饰，审查清单中的 shrink 项经权衡后保留）、正确性/性能问题另走常规审查。

## Capabilities

### New Capabilities

（无——纯删除/简化，不新增能力）

### Modified Capabilities

（无——无 spec 级行为变更：CLI 参数、输出格式、状态文件对外兼容性均不变；状态文件仅少写一个从未读取的键，`#[serde(default)]` + serde 默认忽略未知键保证双向兼容。纯重构，按规范设 `skip_specs: true`）

## Impact

- 代码：`src/{cli,ffmpeg,i18n,main,merger,progress,scanner,state}.rs`，约 -250 行；测试中的重复/同义反复用例同步删除
- CI：`.github/workflows/ci-check.yml` 删除，`ci.yml` 增加 `pull_request` 触发
- 文档/脚本：`docs/` 整目录删除；`scripts/aria2-hook.sh` 一行路径修复
- 依赖：无增删（indicatif 已在依赖中，仅改用其内置模板键）
- 验证：`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test` 全绿；`--help`、`--dry-run`、`--once`、`--resume` 行为不变
