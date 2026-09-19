# 阶段二「核心健壮性」实现计划

## 目标
完成 roadmap 阶段二剩余三项：① `merger.rs` 物理拆分子模块；② `state` 引入逐版本迁移链 + size 锚点（schema 升 v2）；③ 输出校验从「非零大小」升级为「size 一致性」。全程不破坏 AGENTS.md 十条硬约束。

---

## 任务一：`state` 迁移链 + size 锚点（schema v1 → v2）

**文件**：`src/state.rs`

1. 版本常量重构：
   - `LEGACY_SCHEMA_VERSION: u32 = 1`（无 `version` 字段的旧文件默认视为 v1）
   - `CURRENT_SCHEMA_VERSION: u32 = 2`
   - `default_schema_version()` 改为返回 `LEGACY_SCHEMA_VERSION`（关键：让 `#[serde(default)]` 把旧文件正确识别为 v1，而非误判成当前版本）

2. `MergeState` 新增字段：
   ```rust
   #[serde(default, skip_serializing_if = "HashMap::is_empty")]
   pub anchors: HashMap<String, u64>, // stem → 合并后输出文件字节数
   ```
   - `#[serde(default)]` 保证旧 JSON 反序列化天然兼容（新字段缺失 → 空 map）
   - `MergeState::new()` 显式 `version: CURRENT_SCHEMA_VERSION`，`anchors: HashMap::new()`

3. 迁移链（替换现有「version > CURRENT 即 bail」的一刀切）：
   ```rust
   type Migration = fn(&mut MergeState);
   fn migrations() -> &'static [(u32, Migration)] { &[(1, migrate_v1_to_v2)] }
   fn migrate_v1_to_v2(s: &mut MergeState) { s.version = 2; } // anchors 已由 serde default 填充
   ```
   - `load()` 流程：解析 → 若 `version > CURRENT` 仍 bail（前向拒绝不变）→ 若 `version < CURRENT` 依次应用 `migrations()[version..]` → source_dir 校验

4. 新增方法（不含业务判断，只存/取）：
   - `record_anchor(&mut self, stem: &str, size: u64)`
   - `anchor_for(&self, stem: &str) -> Option<u64>`

**不变**：`save()` 的 tmp+rename 原子写、`mark_completed/failed/pending` 语义、三个 HashSet 字段。

---

## 任务二：输出校验升级（size 一致性）

**文件**：`src/merger/`（拆分后落在 `merge.rs`）

`validate_output` 签名升级为 `validate_output(output_path: &Path, min_expected: u64)`：
- 保留「文件存在 + 非零」检查
- 新增：`output_size >= min_expected`（`-c copy` 无损下，输出 = 视频流 + 音频流 + 容器头，必然 ≥ 源视频字节数）。不满足则删除异常输出并报错
- `do_merge` 成功分支：`let min = fs::metadata(&pair.video)?.len()` 传入；校验通过后 `state` 记录锚点由 workflow 层完成（见任务四）

---

## 任务三：`merger.rs` 拆分为 `src/merger/` 子模块

**行为不变、纯机械重构**，`crate::merger::*` 公共 API 保持不变（workflow 引用面不变）。

| 新文件 | 内容 | 约行数 |
|--------|------|--------|
| `src/merger/mod.rs` | `mod` 声明 + `pub use` 重导出（merge_pair / delete_source_files / MergeResult / MergeSummary） | ~15 |
| `src/merger/summary.rs` | `MergeResult`、`MergeSummary` + 报告打印 | ~200 |
| `src/merger/runner.rs` | `ChildExt`、`Runner` 类型、`run_with_timeout`、`FFMPEG_TIMEOUT`/`POLL_INTERVAL` | ~60 |
| `src/merger/merge.rs` | `merge_pair`、`do_merge`、`record_failure`、`validate_output` | ~200 |
| `src/merger/delete.rs` | `delete_source_files` | ~60 |

- 测试随职责归位：`summary_logic_tests`→summary.rs（MergeSummary 部分）+ merge.rs（validate_output）；`timeout_tests`→runner.rs；`runner_tests`/`merge_tests`→merge.rs；`delete_tests`→delete.rs；顶层 `tests` 拆到对应模块
- 子模块间用 `pub(super)` 暴露私有协作项（如 merge.rs 用 runner.rs 的 `Runner`/`run_with_timeout`），`mod.rs` 统一 `pub use` 对外
- 删除 `src/merger.rs` 单文件

---

## 任务四：workflow 接入锚点（记录 + resume 警告）

**文件**：`src/workflow.rs`

1. 合并成功路径（`execute` 内 worker 完成一次合并后）：读取输出文件 size，`state.record_anchor(&pair.stem, size)`。为保持并行安全，锚点记录通过现有 `state_mutex` 内完成（与 `mark_completed` 同一临界区）
2. `scan_and_filter` resume 分支：对已完成 stem，若 `state.anchor_for(stem)` 存在且当前输出文件 size ≠ 锚点值，`eprintln` 警告（**不阻断**，仅提示外部可能改动）

---

## 任务五：i18n 新增 key（中英成对）

**文件**：`src/i18n.rs`（追加到 `TRANSLATIONS`，自动被 `test_table_keys_paired_and_source_keys_exist` 校验）

- `output_too_small`：Cn「输出文件 {0} 大小 {1} 字节小于源视频 {2} 字节，可能合并不完整」 / En「Output {0} is {1} bytes, smaller than source video {2} bytes; merge may be incomplete」
- `output_modified_warning`：Cn「警告：{0} 输出文件大小与上次记录不一致，可能已被外部修改」 / En「Warning: {0} output size differs from the recorded value; it may have been modified externally」

---

## 任务六：文档对齐

- `AGENTS.md`：模块职责表中 `merger` 补充「拆分为 merger/ 子模块」；`state` 补充「schema v2 + 逐版本迁移链 + size 锚点」
- `CHANGELOG.md`：新增阶段二条目（schema v2、迁移链、size 锚点、输出校验升级、merger 拆分）

---

## 测试计划（遵守约束 8）

- **state**：`test_load_v1_migrates_to_v2`（version=1 与无 version 两态 → 迁移后 version=2、anchors 空）；`test_anchors_roundtrip`；`test_record_anchor_and_query`；`test_migrate_v1_to_v2`；更新 `test_load_old_format_gets_default_version` 断言为 v2；`test_reject_future_version` 保持不变
- **merge**：`test_validate_output_too_small_rejected_and_cleaned`、`test_validate_output_meets_min_passes`；现有 `runner_tests` 的 fake runner 输出 15B ≥ 源 5B，仍通过
- **workflow**：resume 输出锚点警告的单元测试（构造 size 不符场景断言不阻断 + 无警告的正常场景）

## 验证步骤
1. `cargo fmt --check` → `cargo clippy --all-targets`（零警告）
2. `cargo test`（全部通过，含 270 现有 + 新增）
3. `cargo build --release`（体积报告确认无回归）
4. 手动：旧 v1 state 文件（无 version / version=1）在 `--resume` 下正常加载并迁移，不报错

## 遵守的硬约束
- 原子写入与 `INTERRUPTED` 检查不变（约束 1）
- 不触碰 ffmpeg 命令，仍 `-c copy`（约束 2）
- 符号链接/路径遍历逻辑不动（约束 3/4）
- 删源顺序「先音频后视频、失败保留」不变（约束 5）
- 新增字符串中英成对（约束 6）
- 零新增运行时依赖（约束 7）
- 退出码映射不动（约束 9）
- 旧 schema 可解析并迁移，不破坏现有 `.mixbilibili_state.json`（约束 10）
