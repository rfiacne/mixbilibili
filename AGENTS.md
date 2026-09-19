# AGENTS.md

## 项目概述

mixbilibili 是一个 Rust CLI 工具，批量合并 Bilibili 下载的 .mp4（视频）和 .m4a（音频）文件为 MKV/MP4/MOV 格式。使用 ffmpeg `-c copy` 无转码模式。

## 模块职责

| 模块 | 职责 | 边界 |
|------|------|------|
| `cli` | 参数定义、解析（clap）、验证 | 不执行业务逻辑，仅产出 `Args` 结构体 |
| `ffmpeg` | ffmpeg 检测、安装引导、命令构建、耗时估算 | 不执行合并流程，只构造 `Command` |
| `scanner` | 递归/非递归扫描目录，配对 .mp4/.m4a，跳过 aria2 下载中的文件 | 不修改文件系统 |
| `merger` | 单对合并执行（含重试、超时、中断检测）、输出验证、源文件删除；物理拆分为 `merger/` 子模块（mod/merge/runner/summary/delete） | 核心 I/O 操作集中此处 |
| `progress` | 进度渲染（TTY 进度条 / 纯文本回退）、时间格式化 | 只负责输出，不影响合并逻辑 |
| `state` | 增量状态持久化（`.mixbilibili_state.json`），支持中断恢复；schema v2 + 逐版本迁移链 + per-stem 输出 size 锚点 | 原子写入（tmp+rename），不含业务判断 |
| `i18n` | 零依赖国际化（中/英），运行时自动检测系统语言 | 静态翻译表，`t(key)` / `tf(key, args)` |
| `ui` | 终端符号（Unicode/ASCII 双模）、语义化颜色、预览列表渲染 | 纯展示层，无业务逻辑 |
| `workflow` | 4 阶段工作流编排（init→scan→交互确认/dry-run→execute→finalize/report）、`--once` 模式、Ctrl+C 注册、rayon 并行执行 | 只协调，不含可复用逻辑 |
| `error` | `AppError` 定义（thiserror）、错误翻译、退出码映射（1/2/3 稳定） | 不含业务逻辑 |
| `main` | 进程入口：调 `workflow::run()`、错误输出与退出、全局 `INTERRUPTED` 标志 | 仅入口，无业务逻辑 |

## 代码约定

### 错误处理
- 统一使用 `anyhow::Result` 传播错误
- 应用层错误使用 `thiserror` 派生 `AppError`，映射到退出码
- 所有用户可见消息必须经过 `i18n` 翻译（`t()` / `tf()`）

### 并行
- 使用 rayon 全局线程池（`main` 中配置一次 `num_threads`）
- 共享状态通过 `Arc<Mutex<MergeState>>` 保护
- 每 5 对文件保存一次增量状态

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

1. **不破坏中断恢复**：修改 `state` 或 `merger` 时必须保持原子写入和 `INTERRUPTED` 标志检查
2. **不引入转码**：ffmpeg 命令必须使用 `-c:v copy -c:a copy`，禁止添加编解码参数
3. **符号链接安全**：`scanner` 必须跳过符号链接，防止无限循环和路径逃逸
4. **路径遍历防护**：`--once` 模式的 stem 参数必须经过 `validate_stem()` 验证
5. **源文件删除原子性**：先删音频，音频删除失败则保留视频，绝不中断时删除源文件
6. **翻译完整性**：新增用户可见字符串必须同时提供中英文翻译
7. **不添加运行时依赖**：除非有明确理由，优先使用标准库和现有依赖
8. **测试覆盖**：修改核心逻辑（scanner/merger/state）时必须更新或新增对应单元测试
9. **退出码稳定**：不修改已有的退出码映射（1=通用错误, 2=ffmpeg缺失, 3=合并失败）
10. **状态文件兼容**：不破坏 `.mixbilibili_state.json` 的现有 schema；schema 升级必须走 `state` 的逐版本迁移链（`migrations()`），并同步更新 `CURRENT_SCHEMA_VERSION`
