# Design: simplify-overengineering

## Context

动机见 proposal.md - Why。当前约束：

- 项目为单一 Rust 二进制 CLI，代码 ~1600 行 src + 等量测试，CI 在 Linux/Windows 双平台跑 fmt/clippy/test。
- 状态文件 `.mixbilibili_state.json` 已有旧格式兼容先例（`#[serde(default)]` 忽略已废弃的 `pending` 键），本次沿用同一机制。
- indicatif 0.17.11 已在依赖中，内置 `{per_sec}`、`{eta}` 模板键（已对照 registry 源码 style.rs:311/332 确认）。
- `StateTracker` 并行 worker 写状态文件，带 seq/save_lock 快照协议。

## Goals / Non-Goals

- Goals：净删 ~250 行 src 与测试；所有用户可见行为（CLI 解析、输出文案、状态文件对外兼容性）不变；CI 配置去重。
- Non-Goals：不修正确性/性能问题（aria2-hook.sh 路径修复除外，经用户确认纳入）；不引入新依赖；不改 `StateTracker` 的快照写协议（该协议保护"锁外写文件时旧快照覆盖新快照"，删除它换来的 ~25 行不值回滚风险）；不动 `format_record` 的四元组 match（各分支输出不同，非冗余）。

## Decisions

1. **indicatif 模板键**：`{per_sec}` 内置渲染为 `N/s`（HumanFloatCount），`{eta}` 为人类可读倒计时。与现自定义的 "0.0 files/s" / `--` 文案略有差异，接受——文案微调不构成行为契约（README/测试均未断言进度条内容）。备选：保留自定义闭包（否决，30 行重复造轮子）。
2. **`Renderer::Text` 的 `completed` 计数器删除**：它只被 `finish()` store、无人 load，属死灵活性。备选：保留供"未来统计"（YAGNI）。
3. **`i18n` 去掉 HashMap**：260 条 `(Lang,&str)` 静态表线性 `find`，调用点在每行打印处，O(260) 次元组比较 << 一次 I/O；省 OnceLock+HashMap 及潜在的初始化分歧。
4. **`ChildExt` → 自由函数**：trait 无第二实现，扩展点没人用；函数签名不变，测试改动仅限调用写法（`wait_with_timeout(&mut child, ...)`）。
5. **`ScanStats.pairs` / `MergeState.output_dir` 删除**：生产代码均不读取，仅测试断言或装饰性写盘。旧状态文件里的 `output_dir` 键由 serde 默认忽略（与 `pending` 同机制），新文件不再写该键。
6. **CI 合并方向**：保留 `ci.yml`（含 build/release job），在 `on:` 增加 `pull_request: branches: [master]`；删除 `ci-check.yml`。PR 上也会跑 fmt/clippy/test（原 ci-check 的职责），release 仍仅 tag 触发。备选：保留两个文件（否决，逐行重复）。
7. **aria2-hook.sh**：`MIXBILIBILI_BIN="${MIXBILIBILI_BIN:-mixbilibili}"` 后用 `$MIXBILIBILI_BIN` 调用；PATH 查找失败时 bash 自然报错退出，不额外加检查。
8. **docs/ 删除**：git 历史保留全文；CHANGELOG 已沉淀结果性内容，过程文档归档只会误导（评审结论，经用户确认）。

## Risks / Trade-offs

- [删测试可能掩盖回归] → 只删同义反复（断言常量等于字面量、构造后即断言自带字段）与逐字重复的用例；每个被删的重复项至少保留一份原始用例；最后 `cargo test` 全绿 + 集成测试覆盖 CLI 行为。
- [indicatif 内置键文案变化] → 无测试依赖该文案；若用户不满可单行改回 with_key。
- [状态文件少写 `output_dir` 后旧版二进制读新版文件] → 旧版 `MergeState` 结构里 `output_dir` 无 `#[serde(default)]`…需确认。已确认：现结构带 `#[serde(default)]`（state.rs:30，整结构体级），旧文件同样能加载缺省字段，双向兼容成立。
- [CI 触发合并导致 push+PR 双触发重复跑] → 接受：ci.yml 原本 push 即跑，ci-check 的增量仅 PR-from-fork 场景，重复运行成本低。

## Migration Plan

纯删除/合并，无数据迁移。回滚 = revert 单个 commit。docs 删除后如需找回：`git log --diff-filter=D -- docs/`。

## Open Questions

（无）
