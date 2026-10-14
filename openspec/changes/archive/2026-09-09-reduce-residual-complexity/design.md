# Design: reduce-residual-complexity

## Context

- 上一轮 `simplify-overengineering` 已归档；ponytail-review 复查列出残余项，全部经源码核实。
- clap builder 对 source/output/format/retry 设了 `default_value`，对 sdel/progress 设了 `default_value("true")` + `default_missing_value`，因此 `parse_args` 中对应 `get_one(...).unwrap_or(..)` 的兜底分支在运行时不可达。
- `indicatif::ProgressBar` 内部即 `Arc`，`Clone` 共享底层状态且 `Send + Sync`，外层再包 `Arc` 是重复。
- 上一轮 design.md 已就 `StateTracker` 快照协议做出"保留"决定，本轮不重开。

## Goals / Non-Goals

- Goals：净删约 120 行；用户可见行为（CLI 解析、退出码、状态文件、stdout 文案内容）零变化；测试覆盖面不降（只删逐字/子集重复与空转断言）。
- Non-Goals：不动 `StateTracker`、i18n 查表结构；不修正确性/性能问题；不引入依赖。

## Decisions

1. **`std::env::consts::OS` 替代 `Os` 枚举**：`package_managers` 直接 `match OS { "windows" => ..., "macos" => ..., "linux" => ..., _ => vec![] }` 并立即 `.into_iter().find(|(name, _)| which::which(name).is_ok())` 返回 `Option<(&str, &str)>`。`get_manual_instructions` 收 `&str`。备选：保留枚举（否决：三 cfg 分支 + dead_code 豁免，纯重复 std）。
2. **`run_install(cmd: &str)`**：slice 参数与 `pms[0]` 索引、重复判空一并消失；`prompt_and_install` 与 `ensure_ffmpeg` 合并为 `ensure_ffmpeg -> Result<bool>` 主链 + 一个 `prompt(cmd: &str) -> bool`。备选：保留三段结构（否决：os 参数层层穿针，单调用点）。
3. **`parse_args` 信任 clap**：`get_one::<String>("source").expect("default set by builder")`。clap 的默认值即唯一事实源。bool 型 sdel/progress 统一 `!= "false"` 语义，收敛为一行闭包/小函数。
4. **`print_preview_pairs(pairs: &[FilePair], format, header)`**：估时打印移出函数体，由两个批量调用点在调用后各补一行 `format_time_estimate`（dry-run 与 interactive 两处都已在打印估时，行为不变）。`run_once` 传入 `std::slice::from_ref(&pair)`。
5. **`format_record` 去 `colored` 参数**：Bar 分支同样输出带 ANSI 的字符串。indicatif 的 `set_message` 渲染时经 console 的 `measure_text_width` 剥离 ANSI 计算宽度，条形对齐不受影响；彩色输出是增强非回退。备选：保留 bool（否决：四个分支的 green/normal 复制纯样板）。
6. **删测试的准则**：与上一轮相同——只删子集重复（保留的用例覆盖被删用例的全部断言）与空转断言（不构造任何失败可能）。`test_merge_summary_default` 保留；`timing_methods` 覆盖 avg/throughput 正路径；`avg_none` 与 `throughput_none` 保留边界。

## Risks / Trade-offs

- [删测试掩盖回归] → 每个被删用例的断言均可由保留用例推出；`cargo test` 全绿 + 集成测试兜底。
- [expect 在 clap 无默认值时 panic] → builder 与 parse 同文件相邻，属编程错误快速失败，符合 fail-fast。
- [进度条着色] → 无测试断言 bar 文案；CI 非 TTY 走 Text 分支本就彩色，行为统一反而消除了 Bar/Text 两条渲染差异。

## Migration Plan

纯删除/简化，单 commit 可 revert。无数据迁移。

## Open Questions

（无）
