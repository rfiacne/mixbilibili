# mixbilibili CLI UI/UX 改进 Workflow

**Date**: 2026-07-17
**Status**: 编排方案
**Methodology**: TDD + SDD 混合，串联执行

---

## 项目现状分析

### 技术栈
- **语言**: Rust CLI 工具（无 Web/GUI）
- **UI 层**: `indicatif` 进度条 + `colored` ANSI 颜色 + `println!` 文本输出
- **i18n**: 自建中英双语系统 (`src/i18n.rs`)，自动检测系统语言
- **模块结构**: `progress.rs`（渲染）→ `merger.rs`（报告）→ `main.rs`（流程）→ `cli.rs`（参数）

### 当前 UI 元素
| 元素 | 现状 | 问题 |
|------|------|------|
| 进度条 | 30字符 cyan/blue `[████=>-----]` | 模板单调，缺少视觉层次 |
| 状态符号 | `✓` 绿色, `✗` 红色, `↻` 黄色 | 可用但缺乏品牌感 |
| 报告头部 | `================================` + cyan bold | ASCII 分隔线原始 |
| 预览列表 | 缩进文本列表 | 无表格对齐，无颜色区分 |
| 错误输出 | `Error:` 前缀 + red | 缺少视觉层级 |
| 时间格式 | `123ms` / `1.23s` / `2m 30s` | 合理但可更紧凑 |
| 交互确认 | `(y/N)` 纯文本 | 无高亮，不够醒目 |

### 已有 UX 特性（v0.7.0）
- 交互式预览模式 (`--interactive`)
- Dry-run 时间估算
- TTY 自动检测（bar vs text）
- 中断恢复 (`--resume`)
- 静默模式 (`--quiet`)

---

## Workflow 总览（串联 6 阶段）

```
阶段 1          阶段 2          阶段 3          阶段 4          阶段 5          阶段 6
UX 研究员  →  UI 设计师  →  SDD 规格编写  →  TDD 测试先行  →  前端开发者  →  现实检验者
(审计现状)    (视觉方案)    (设计规格文档)   (写失败测试)     (实现+通过测试)  (最终验收)
```

每个阶段 **严格串联**，输出作为下一阶段输入。

---

## 阶段 1：UX 研究员 — 现状审计与用户痛点分析

**Agent**: `UX 研究员`
**输入**: 项目源码（`progress.rs`, `merger.rs`, `main.rs`, `cli.rs`, `i18n.rs`）+ 现有设计文档
**方法**: 运行工具，捕获实际输出，分析视觉层次和信息密度

### 任务
1. 运行 `cargo build --release && ./target/release/mixbilibili --help` 捕获帮助输出
2. 运行 `./target/release/mixbilibili --dry-run -s <test_dir>` 捕获预览输出
3. 运行正常合并流程，截图/记录进度条和报告输出
4. 分析 `print_full_report()` 的信息层次和视觉权重
5. 对比同类工具（yt-dlp, ffmpeg-progress, aria2c）的终端美学

### 输出文件
`docs/ux-audit/ux-audit-report.md`
- 当前输出的截图/记录
- 视觉层次评分（1-5）
- 信息密度分析
- 用户痛点列表（按优先级排序）
- 竞品视觉对比

### 串联逻辑
阶段 2 的 UI 设计师 **必须** 基于审计报告中的痛点列表设计方案，不可凭空设计。

---

## 阶段 2：UI 设计师 — 终端视觉设计系统

**Agent**: `UI 设计师`
**输入**: `docs/ux-audit/ux-audit-report.md` + `src/progress.rs` + `src/merger.rs`
**方法**: 设计终端设计系统（配色、符号、排版、动效）

### 任务
1. **配色方案**: 基于 Bilibili 品牌色（#FB7299 粉）设计终端配色
   - 主色、辅色、成功/失败/警告/信息色
   - 暗色/亮色终端适配
   - 使用 `colored` crate 的可用颜色映射

2. **符号系统**: 设计统一的 Unicode 符号语言
   - 状态符号（成功/失败/跳过/重试/进行中）
   - 分隔符和装饰符
   - 考虑 Nerd Font / 普通终端兼容

3. **排版规范**:
   - 标题层级（H1/H2/H3 在终端的映射）
   - 缩进和对齐规则
   - 表格/列表格式
   - 空白和间距节奏

4. **动效设计**（进度条相关）:
   - 进度条样式和动画字符
   - Spinner 样式选择
   - 完成/失败的过渡效果

5. **组件设计**:
   - 进度条组件（重新设计模板）
   - 报告卡片组件
   - 预览列表组件
   - 错误消息组件
   - 交互确认组件

### 输出文件
`docs/ux-audit/terminal-design-system.md`
- 配色方案表（含 `colored` crate 代码映射）
- 符号字符表
- 排版规范
- ASCII mock-up 每个组件的前后对比
- 动效 GIF/描述

### 串联逻辑
阶段 3 的规格编写 **必须** 将设计系统的每个元素转化为可验证的规格条目。

---

## 阶段 3：产品经理 — SDD 规格编写

**Agent**: `高级项目经理`
**输入**: `docs/ux-audit/terminal-design-system.md` + `docs/ux-audit/ux-audit-report.md`
**方法**: 将视觉设计转化为精确的技术规格，每条可测试

### 任务
1. 将设计系统每个元素转化为规格条目
2. 为每个 UI 组件编写 **验收标准**（Given/When/Then 格式）
3. 定义 i18n 新增 key 列表（中英双语）
4. 定义向后兼容约束（`--quiet` 模式不变，text 模式不变）
5. 优先级排序（P0 必须 / P1 应该 / P2 可选）

### 规格条目示例

```markdown
### SPEC-UI-001: 报告头部视觉升级
**优先级**: P0
**组件**: MergeSummary::print_full_report()
**验收标准**:
- Given: 合并完成，生成完整报告
- When: 报告头部渲染
- Then: 使用 box-drawing 字符替代 `===` 分隔线
- And: 标题使用 cyan + bold + 左侧装饰符号
- And: 中文环境使用 `╔══ 合并报告 ══╗` 样式
- And: 英文环境使用 `╔══ Merge Report ══╗` 样式

### SPEC-UI-002: 进度条品牌化
**优先级**: P1
**组件**: MergeProgress::new()
**验收标准**:
- Given: TTY 环境执行批量合并
- When: 进度条渲染
- Then: 进度条使用 `█▓░` 三阶字符替代 `=>-`
- And: 已完成部分使用 magenta 色（Bilibili 品牌色近似）
- And: 未完成部分使用 bright_black
- And: 模板格式: `{spinner} {msg:30} {bar:25} {pos}/{len} {eta}`
```

### 输出文件
`docs/ux-audit/ui-spec-v1.md`
- 完整规格条目列表（SPEC-UI-001 ~ SPEC-UI-N）
- 每条含验收标准
- i18n key 新增列表
- 向后兼容矩阵
- 优先级排序

### 串联逻辑
阶段 4 的 TDD **必须** 为每条规格条目编写至少一个失败测试。

---

## 阶段 4：TDD 测试先行 — 编写失败测试

**Agent**: `前端开发者`（Rust CLI 上下文）
**输入**: `docs/ux-audit/ui-spec-v1.md` + 现有测试文件
**方法**: Red-Green-Refactor 的 Red 阶段

### 任务
1. 为每个 SPEC-UI 条目编写测试用例
2. 测试类型：
   - **单元测试**: `format_record()` 输出格式验证
   - **快照测试**: 完整报告输出的字符串匹配
   - **i18n 测试**: 新增 key 的中英翻译验证
   - **TTY 模拟测试**: bar/text 模式的输出差异验证
3. 所有测试 **必须初始失败**（Red）
4. 使用 `#[ignore]` 标记新 UI 测试，避免阻塞 CI

### 测试文件变更
| 文件 | 新增测试 |
|------|---------|
| `src/progress.rs` | 进度条模板验证、符号输出、颜色代码 |
| `src/merger.rs` | 报告头部格式、box-drawing 字符、对齐 |
| `src/i18n.rs` | 新增 key 翻译完整性 |
| `tests/ui_snapshot_test.rs` | 新文件：完整输出快照测试 |

### 输出
- `cargo test` 输出显示 N 个新测试 failing
- 测试代码已提交（`git commit -m "test: add failing UI tests for spec-v1"`）

### 串联逻辑
阶段 5 的开发者 **必须** 逐个让失败测试通过，不可跳过或修改测试来适配旧代码。

---

## 阶段 5：前端开发者 — 实现 + 通过测试

**Agent**: `前端开发者`
**输入**: `docs/ux-audit/ui-spec-v1.md` + 失败测试 + `docs/ux-audit/terminal-design-system.md`
**方法**: Red-Green-Refactor 的 Green 阶段

### 任务（按优先级执行）

#### P0: 报告头部视觉升级
- 修改 `src/merger.rs` 的 `print_full_report()`
- 用 box-drawing 字符替代 `===` 分隔线
- 新增 i18n key: `report_border_*`, `report_icon_*`

#### P1: 进度条品牌化
- 修改 `src/progress.rs` 的 `MergeProgress::new()`
- 更新 `ProgressStyle` 模板和 `progress_chars`
- 添加 spinner 样式

#### P1: 预览列表表格化
- 修改 `src/main.rs` 的 `print_preview_pairs()`
- 对齐文件名，添加序号和颜色区分

#### P2: 交互确认美化
- 修改 `src/main.rs` 的交互确认输出
- 高亮 `(y/N)` 选项

#### P2: 错误消息层级
- 修改错误前缀格式
- 添加错误分类图标

### 文件变更矩阵
| 文件 | 变更类型 | 影响范围 |
|------|---------|---------|
| `src/progress.rs` | 修改 | 进度条模板、颜色、符号 |
| `src/merger.rs` | 修改 | 报告格式、分隔线 |
| `src/main.rs` | 修改 | 预览列表、交互确认 |
| `src/i18n.rs` | 修改 | 新增 ~20 个翻译 key |
| `src/cli.rs` | 无变更 | — |
| `Cargo.toml` | 可能修改 | 若需新依赖（如 `console`） |

### 质量门禁
- `cargo test` 全部通过（包括阶段 4 的新测试）
- `cargo clippy -- -D warnings` 无警告
- `cargo build --release` 成功
- 手动运行验证视觉效果

### 串联逻辑
所有测试通过后，进入阶段 6 最终验收。

---

## 阶段 6：现实检验者 — 最终验收

**Agent**: `现实检验者`
**输入**: 所有阶段产出 + 编译产物
**方法**: 独立验证，默认为 "NEEDS WORK"

### 验收清单
1. **视觉验证**: 运行工具，对比设计稿 mock-up
   - [ ] 进度条使用新模板和品牌色
   - [ ] 报告头部使用 box-drawing 字符
   - [ ] 预览列表对齐且有色标
   - [ ] 符号系统一致（无混用旧/新符号）

2. **i18n 验证**: 切换 LANG 环境变量
   - [ ] `LANG=zh_CN` 所有新 key 显示中文
   - [ ] `LANG=en_US` 所有新 key 显示英文
   - [ ] 无 fallback 到 key name 的情况

3. **兼容性验证**:
   - [ ] `--quiet` 模式无额外输出
   - [ ] 非 TTY 环境（pipe）text 模式正常
   - [ ] `--dry-run` 输出格式正确
   - [ ] `--interactive` 确认提示美观

4. **测试验证**:
   - [ ] `cargo test` 全部通过
   - [ ] 无 `#[ignore]` 遗留
   - [ ] 快照测试覆盖所有新 UI 组件

5. **性能验证**:
   - [ ] 进度条渲染无闪烁
   - [ ] 大量文件（100+）进度条流畅

### 输出
`docs/ux-audit/acceptance-report.md`
- 每项验收的 PASS/FAIL
- 截图/终端输出记录
- 遗留问题列表（如有）
- 最终裁决: `PRODUCTION READY` / `NEEDS WORK`

---

## 串联依赖图

```
阶段 1 (UX 研究员)
  │
  ├── 输出: ux-audit-report.md (痛点列表)
  │
  ▼
阶段 2 (UI 设计师)
  │
  ├── 输入: 痛点列表
  ├── 输出: terminal-design-system.md (设计系统)
  │
  ▼
阶段 3 (高级项目经理)
  │
  ├── 输入: 设计系统 + 痛点列表
  ├── 输出: ui-spec-v1.md (可测试规格)
  │
  ▼
阶段 4 (前端开发者 - TDD)
  │
  ├── 输入: 规格条目
  ├── 输出: 失败测试集 (Red)
  │
  ▼
阶段 5 (前端开发者 - 实现)
  │
  ├── 输入: 失败测试 + 设计系统
  ├── 输出: 通过测试的实现 (Green)
  │
  ▼
阶段 6 (现实检验者)
  │
  ├── 输入: 编译产物 + 全部文档
  ├── 输出: acceptance-report.md (验收裁决)
```

---

## 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| `colored` crate 颜色有限 | 无法精确匹配 Bilibili 粉 | 用最接近的 `magenta` 或引入 `crossterm` |
| 终端兼容性 | box-drawing 在旧终端乱码 | 添加 fallback 检测，非 Unicode 终端用 ASCII |
| 快照测试脆弱 | 微小改动导致大量测试失败 | 使用规范化比较（忽略空白/颜色代码） |
| i18n 膨胀 | 翻译 key 数量翻倍 | 按模块分组，添加 key 命名规范 |
| 进度条闪烁 | 高频更新导致视觉噪音 | 调整 `steady_tick` 间隔，使用 double-buffer |

---

## 预估工作量

| 阶段 | Agent | 预估时间 | 文件变更数 |
|------|-------|---------|-----------|
| 1. UX 审计 | UX 研究员 | 30 min | 1 (报告) |
| 2. 视觉设计 | UI 设计师 | 45 min | 1 (设计系统) |
| 3. 规格编写 | 高级项目经理 | 30 min | 1 (规格) |
| 4. TDD 测试 | 前端开发者 | 40 min | 3-4 (测试文件) |
| 5. 实现 | 前端开发者 | 60 min | 4-5 (源码) |
| 6. 验收 | 现实检验者 | 20 min | 1 (报告) |
| **总计** | | **~3.75h** | **~12 文件** |
