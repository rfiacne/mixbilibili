# AGENTS.md

## 项目概述

mixbilibili 是一个 Rust CLI 工具，批量合并 Bilibili 下载的 .mp4（视频）和 .m4a（音频）文件为 MKV/MP4/MOV 格式。使用 ffmpeg `-c copy` 无转码模式。

## 模块职责

| 模块 | 职责 | 边界 |
|------|------|------|
| `cli` | 参数定义、解析（clap）、验证 | 不执行业务逻辑，仅产出 `Args` 结构体 |
| `ffmpeg` | ffmpeg 检测、安装引导、命令构建、耗时估算 | 不执行合并流程，只构造 `Command` |
| `scanner` | 递归/非递归扫描目录，配对 .mp4/.m4a，跳过 aria2 下载中的文件 | 不修改文件系统 |
| `merger` | 单对合并策略（重试、超时、中断检测）、输出验证、源文件删除、汇总报告。通过 `MergePlan` 传入运行参数，通过 `ffmpeg::MergeRunner` 接缝调用 ffmpeg | 核心 I/O 操作集中此处 |
| `pipeline` | 一个批次的完整生命周期（`process`）：逐对合并、记录结果、每 5 对保存状态、清理部分输出、`--sdel` 删源、构建报告。`--once` 是单对批次，不走独立代码路径 | 只编排，合并策略在 `merger` |
| `interrupt` | 协作式取消令牌（`Interrupt`），Ctrl+C 处理程序只是它的一个适配器 | 替代进程级全局标志 |
| `progress` | 进度渲染（TTY 进度条 / 纯文本回退）、时间格式化 | 只负责输出，不影响合并逻辑 |
| `state` | 增量状态持久化（`.mixbilibili_state.json`），支持中断恢复；schema v2 + 逐版本迁移链 + per-stem 输出 size 锚点 | 原子写入（tmp+rename），不含业务判断 |
| `i18n` | 零依赖国际化（中/英），运行时自动检测系统语言 | 静态翻译表，`t(key)` / `tf(key, args)` |
| `ui` | 终端符号（Unicode/ASCII 双模）、语义化颜色 | 纯展示层，无业务逻辑 |
| `lib` | `run()` 是完整接口（读参数→合并→报告），`AppError`/`Failure` 与退出码映射，预览渲染 | 库接口层 |
| `main` | 进程适配器：调 `mixbilibili::run()`、输出错误、退出 | 仅入口，无业务逻辑 |

## 代码约定

### 错误处理
- 统一使用 `anyhow::Result` 传播错误
- 应用层错误使用 `thiserror` 派生 `AppError`，映射到退出码
- 所有用户可见消息必须经过 `i18n` 翻译（`t()` / `tf()`）

### 并行
- 使用 rayon 全局线程池
- 每 5 对文件保存一次增量状态（`pipeline::STATE_SAVE_INTERVAL`）

### 国际化
- 不引入外部 i18n 框架，使用内置静态翻译表
- 新增用户可见文本时必须同时添加 `Lang::Cn` 和 `Lang::En` 条目

### 测试
- 单元测试使用 `#[cfg(test)]` 条件编译
- 使用 `tempfile` crate 创建临时目录
- 覆盖正常路径、边界条件和错误情况

### 构建
- Release profile 优化体积：`opt-level="z"`, LTO=fat, strip=true, panic=abort
- MSRV: Rust 2021 edition

## Agent 必须遵守的约束

1. **不破坏中断恢复**：修改 `state` 或 `merger` 时必须保持原子写入和 `interrupt::Interrupt` 令牌检查；被中断的 pair 属于「未完成」而非「失败」（`was_interrupted`），`all_success()`——决定是否删除状态文件——只有全部 pair 到达终态才为真；`RunOutcome::Cancelled` 与 `RunOutcome::TimedOut` 必须保持区分，勿把两者合并为同一返回值
2. **不引入转码**：ffmpeg 命令必须使用 `-c:v copy -c:a copy`，禁止添加编解码参数
3. **符号链接安全**：`scanner` 必须跳过符号链接，防止无限循环和路径逃逸
4. **路径遍历防护**：`--once` 模式的 stem 参数必须经过 `validate_stem()` 验证
5. **源文件删除原子性**：先删音频，音频删除失败则保留视频，绝不中断时删除源文件
6. **翻译完整性**：新增用户可见字符串必须同时提供中英文翻译
7. **不添加运行时依赖**：除非有明确理由，优先使用标准库和现有依赖
8. **测试覆盖**：修改核心逻辑（scanner/merger/state）时必须更新或新增对应单元测试
9. **退出码稳定**：不修改已有的退出码映射（1=通用错误, 2=ffmpeg缺失, 3=合并失败）
10. **状态文件兼容**：不破坏 `.mixbilibili_state.json` 的现有 schema；schema 升级必须走 `state` 的逐版本迁移链（`migrations()`），并同步更新 `CURRENT_SCHEMA_VERSION`
