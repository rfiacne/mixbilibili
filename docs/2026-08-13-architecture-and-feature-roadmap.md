# mixbilibili 架构评估与功能路线图

> 日期：2026-08-13
> 版本基线：v0.8.0（`Cargo.toml` 已升到 0.8.0，`CHANGELOG.md` 仍停在 0.7.0，文档滞后待补）
> 分析依据：codebase-memory 知识图谱（1209 节点 / 3257 边）+ 源码阅读 + 测试/CI 现状

---

## 1. 现状架构概览

单一 Rust 二进制（6648 行源码 + 437 行集成测试），无运行时重依赖，核心链路为：

```
cli → main → scanner → merger → state / progress / ui
```

知识图谱的依赖分层（`boundaries` 聚合 CALLS 边）给出如下结论：

| 层 | 模块 | 特征 |
|----|------|------|
| entry（只发出调用） | `main`、`scanner`、`ffmpeg`、`integration_test` | 编排与入口 |
| internal（扇入 7 / 扇出 19） | `merger` | 唯一汇聚业务逻辑的中间层 |
| core（高扇入 / 0 出边） | `state`（38/0）、`ui`（38/0）、`progress`（18/5） | 公共底座，改动波及面最大 |

调用热点（fan-in 排序）：`MergeState::new`(33)、`Symbols::new`(32)、`cli::build_cli`(22)、`scanner::scan_directory`(22)、`cli::parse_args`(21)、`MergeState::load`(18)、`MergeState::save`(13)、`i18n::t`(12)。

**模块体量**（不含测试）：

| 文件 | 行数 | 职责 |
|------|------|------|
| `main.rs` | 1137 | 入口 + 4 阶段工作流编排 + 交互确认 + Ctrl+C |
| `merger.rs` | 1217 | 合并执行、重试、超时、输出验证、删源 |
| `scanner.rs` | 796 | 目录扫描、配对、aria2 检测 |
| `cli.rs` | 742 | 参数定义/解析/验证 |
| `state.rs` | 702 | 增量状态持久化 |
| `ffmpeg.rs` | 565 | ffmpeg 检测、安装引导、命令构建 |
| `i18n.rs` | 506 | 静态翻译表 |
| `progress.rs` | 413 | 进度渲染 |
| `ui.rs` | 133 | 终端符号 + 颜色 |

---

## 2. 已识别的架构问题与信号

以下按"影响面 / 修复成本"排序，均来自源码证据，非臆测。

### 2.1 模块已偏大，边界与实际职责开始漂移

- **`main.rs`（1137 行）承担了过多编排**：`run()` 内联了 4 个阶段（init → scan → dry-run/交互确认 → execute → finalize/report），还包含 `parse_prompt_response`、`validate_stem`、`print_preview_pairs`、`translate_error`、`get_exit_code` 等**本不该属于"入口"的可复用逻辑**。AGENTS.md 明确 `main` 应"协调所有模块，不含可复用逻辑"，现状已有偏差。
- **`merger.rs`（1217 行）是核心 I/O 集中地**，但重试/超时/中断检测/输出验证/源文件删除五类职责耦合在一个文件里，单测与产品代码交织（38 个测试与函数体混排）。
- **`cli.rs` 742 行** 同时包含 clap 定义、`Args` 结构体、`validate()` 与 44 个测试，职责尚可但已到拆分阈值。

### 2.2 `--once` 与 `scan_directory` 存在配对逻辑重复

`--once` 模式（aria2 hook 触发）跳过扫描，但 `merger::merge_pair` 里用 `pair.stem.replace('/', "_")` 处理复合键，而 `scanner` 里用 `make_key` 构造同一复合键——**两条路径各自维护"stem 规范化"规则**，未来改规则（如处理更深的目录层级、同名冲突）需要同步两处。

### 2.3 `ffmpeg` 命令构建硬编码 `"ffmpeg"`

`build_merge_command` 用 `Command::new("ffmpeg")`，而 `ffmpeg_path()` 已经能定位到具体路径。在 PATH 中未安装但 `--once`/aria2 hook 场景下（如 hook 环境 PATH 精简），存在定位不一致的风险。

### 2.4 `state` 无版本迁移机制

`MergeState` 的 `version` 字段只有"拒绝更高版本"的前向检查（`version > 1` 即 bail），**没有向后迁移 / 字段兼容策略**。AGENTS.md 约束"不破坏 `.mixbilibili_state.json` schema"，但目前只有一道硬门槛，未来新增字段（如每个 stem 的源文件 hash）会直接触发版本冲突。

### 2.5 `i18n` 静态表随功能线性膨胀

506 行 match 表是刻意选择的零依赖方案，但**新增每个用户可见字符串需同时手改两处**（`Lang::Cn` / `Lang::En`），且 `t()` 的 key 是运行时字符串，编译器不校验 key 是否存在于表中（拼写错误延迟到运行时表现为 fallback）。这是"零依赖"与"可维护性"的明确权衡点。

### 2.6 文档/版本滞后

- `Cargo.toml` 0.8.0，`CHANGELOG.md` 止于 0.7.0。
- 知识图谱中 Module/File 节点间**未建立 `IMPORTS`/`CALLS` 边**（Rust 的 `mod` 声明未建模），模块依赖关系只能靠 `boundaries` 聚合推断——这是索引工具的局限，也说明当前"模块关系图"主要依赖人工维护的 AGENTS.md。

---

## 3. 架构优化方向

> 原则：YAGNI，逐项可独立落地，且不触犯 AGENTS.md 的 10 条硬约束（中断恢复、不转码、符号链接安全、路径遍历防护、删源原子性、翻译完整、零运行时依赖、测试覆盖、退出码稳定、状态 schema 兼容）。

### A. 拆分 `main.rs`，回收"编排"边界（高价值 / 中成本）

把 `run()` 里可复用的逻辑下沉到独立模块：

- 新建 `workflow.rs`（或 `orchestrator.rs`）：承载 4 阶段编排与交互确认流程，`main` 只保留入口、错误到退出码的映射、Ctrl+C 注册。
- `validate_stem` / `parse_prompt_response` / `translate_error` / `get_exit_code` 分别归位（stem 校验 → `scanner` 或 `cli`；prompt → `ui`；错误映射 → 独立 `error` 模块）。

**收益**：`main.rs` 降到 300 行以内；`AppError` 与退出码映射集中到一个 `error` 模块，与 AGENTS.md 的"退出码稳定"约束对齐。

### B. 收敛 `merger` 的职责，引入可注入的 ffmpeg 后端（高价值 / 中高成本）

- 把 `merge_pair` 里的重试循环、超时（`wait_with_timeout`）、中断检测、输出验证、删源逻辑拆为 `merger/` 下的子模块，或至少清晰分块。
- 引入 `trait FfmpegRunner`（或 `CommandRunner`）抽象 ffmpeg 调用，生产实现走 `Command`，测试用 mock 避免依赖真实 ffmpeg 的脆弱断言（当前 `merger` 38 个测试中部分依赖真实 ffmpeg）。

**收益**：核心 I/O 可单元测试化；同时顺带解决 2.3 的 `ffmpeg_path()` 硬编码问题。

### C. `state` 版本化迁移框架（中价值 / 中成本）

- 在 `load()` 中引入"逐版本迁移"表（`migrate_v1_to_v2` 等），而非一刀切 bail。
- 为每个 stem 记录源文件 size（或 hash）作为**校验锚点**，为未来"源文件是否被外部改动"的判断铺路——注意这本身是一次 schema 升级，需走迁移路径。

### D. `i18n` 可维护性增强（中价值 / 低成本）

- 用 `const` 替代字符串字面量作为 key（如 `pub const ERR_MERGE: &str = "err_merge"`），让编译器在引用处做拼写校验。
- 或用 `macro_rules!` 生成 `t!()` 并配一个**编译期测试**遍历所有 key 断言中英文条目成对存在——**保持零依赖**的前提下把"翻译完整性"从人肉约定变为 CI 可强制。

### E. 补充知识图谱建模 / 文档对齐（低成本）

- 修正 `CHANGELOG.md` 至 0.8.0。
- 在 CI 中增加"翻译完整性"断言（配合 D），以及 release 构建的二进制体积报告（0.5.0 已提过）。

---

## 4. 功能进步方向

结合 Bilibili 下载场景与现有能力，按"用户价值 / 实现成本"分级：

### 高价值

1. **字幕/弹幕合并**：Bilibili 下载常见 `.xml`（弹幕）或 `.srt`/`.json`（字幕）伴随文件，`ffmpeg -c copy` 可嵌入字幕流（`-c:s mov_text` 或 `-c:s srt`），让成品 MKV 携带字幕。**注意**：这涉及"是否引入转码"的边界——纯 copy 嵌入字幕需评估 `-c:s copy` 的兼容性，与 AGENTS.md "不转码"约束需明确取舍。
2. **封面/元数据写入**：读取 `.jpg` 封面与视频标题，用 `-map_metadata` / `-attach` 写入 MKV 头，不触碰音视频流。
3. **watch 模式（inotify/fswatch）**：以目录监听替代 aria2 hook 的"下载完成回调"，对非 aria2 下载器（yt-dlp、BBDown、bilili）通用化——**这是当前 `--once` + aria2 hook 架构的通用化升级**。
4. **配置化（`.mixbilibili.toml` / `config` 模块）**：把默认输出目录、格式、并发、删除策略等沉淀为项目级/用户级配置文件，减少每次命令行重复。

### 中价值

5. **输出命名模板**：支持 `{stem}`、`{title}`、`{date}` 等占位符，替代硬编码的 `safe_stem + 扩展名`。
6. **内容校验与增量 hash**：合并后比对源/目标 size 与 hash，替代当前仅 `validate_output`（非零大小）的弱校验。
7. **失败清单导出**：`--retry` 之外，把失败 stem 导出为清单文件，供 `--once` 循环重跑或下次 `--resume` 精准重试。
8. **更多容器格式**：`webm`、`ts`（Bilibili 旧版下载常见 `.flv`/`.ts`），需评估 `-c copy` 流兼容性。

### 低价值 / 暂缓

9. GUI / TUI 全量重做——当前 TTY 进度条已足够，收益低。
10. 转码/压缩——与项目定位（`-c copy` 无损合并）直接冲突，明确不做。
11. 云端/网络功能——超出 CLI 工具定位。

---

## 5. 路线图（建议分三阶段）

> 每阶段可独立合入，均不破坏现有退出码、状态 schema 与 `-c copy` 约束。

### 阶段一：架构还债（低成本、低风险，先做）

- [ ] 修正 `CHANGELOG.md` 到 0.8.0，补 0.8.0 的 changelog 条目（对应 `git log` 中的 UI/UX 改进、`--ascii`、递归扫描加固等）。
- [ ] `main.rs` 拆分：抽出 `workflow.rs` + `error` 模块（回收 `validate_stem`/`parse_prompt_response`/`translate_error`/`get_exit_code`）。
- [ ] 统一 `--once` 与 `scan_directory` 的 stem 规范化规则为单一函数（消除 2.2 的重复）。
- [ ] i18n key 改 `const` + 编译期"中英成对"测试（配合 CI 强制）。

### 阶段二：核心健壮性（中成本）

- [ ] `merger` 职责拆分 + 引入可注入 ffmpeg 后端（解决 2.3 硬编码 + 测试脆弱性）。
- [ ] `state` 版本化迁移框架 + 为 stem 记录 size/hash 校验锚点。
- [ ] 输出校验从"非零大小"升级为"size 一致性 + 可选 hash"。

### 阶段三：功能扩展（按用户优先级挑选）

- [ ] 字幕/封面/元数据嵌入（先做封面与元数据，字幕需单独论证转码边界）。
- [ ] watch 模式（通用化 aria2 hook，扩展到 yt-dlp/BBDown）。
- [ ] 配置化 + 输出命名模板。
- [ ] 失败清单导出与精准重试。

---

## 6. 风险与约束（不可破坏项）

对齐 AGENTS.md 十条约束，路线图中的每项都需满足：

1. **中断恢复**：`state`/`merger` 改动必须保持原子写入（tmp+rename）与 `INTERRUPTED` 检查。
2. **不转码**：ffmpeg 命令必须保持 `-c:v copy -c:a copy`；字幕嵌入若涉及 `-c:s` 转码需单独立项论证。
3. **符号链接安全**：watch 模式 / 递归扫描扩展需继续跳过符号链接。
4. **路径遍历防护**：`--once` 的 stem 校验下沉后不得弱化 `validate_stem`。
5. **删源原子性**：先删音频、失败保留视频的次序不可变。
6. **翻译完整**：新增字符串中英成对，配合 CI 断言强制。
7. **零运行时依赖**：优先标准库与现有依赖（`inotify` 等需评估，若引入需明确理由）。
8. **测试覆盖**：改 `scanner`/`merger`/`state` 必须同步更新/新增单测。
9. **退出码稳定**：1=通用错误、2=ffmpeg 缺失、3=合并失败，不新增/重排。
10. **状态 schema 兼容**：任何 schema 变更走版本化迁移，不破坏旧文件解析。

---

## 7. 建议的下一步

建议优先落地**阶段一**（纯架构还债，零功能风险），其中 `main.rs` 拆分 + i18n const 化是性价比最高、且为后续所有改动铺路的项。若要推进，可从"阶段一"中挑选 1–2 项进入正式设计（brainstorm → spec → plan → 实现）。

> 本文档为分析建议，供讨论；未进入实现前不修改任何代码。
