# mixbilibili CLI UI/UX 改进设计规格

**Date**: 2026-07-17
**Status**: 设计已批准
**Methodology**: TDD + SDD，6 阶段串联执行

---

## 1. 项目背景

mixbilibili 是 Rust CLI 工具，用于批量合并音视频文件。当前 UI 层基于 `indicatif` 进度条 + `colored` ANSI 颜色 + `println!` 文本输出。

**改进目标**: 极简专业风格，提升视觉层次和信息密度，保持功能不变。

---

## 2. 设计原则

- **极简专业**: 灰阶为主，蓝色点缀，无品牌色装饰
- **语义优先**: 颜色仅用于传达状态（成功/失败/警告），非装饰
- **向后兼容**: `--quiet` 模式、非 TTY 环境、现有 i18n key 含义不变
- **渐进增强**: Unicode 终端显示 box-drawing，非 Unicode 降级 ASCII

---

## 3. 配色方案

| 用途 | 颜色 | colored API |
|------|------|-------------|
| 主文本 | 默认/白色 | `Color::White` |
| 次要文本 | 亮黑 | `Color::BrightBlack` |
| 标题/强调 | 青色 | `Color::Cyan` |
| 成功 | 绿色 | `Color::Green` |
| 警告 | 黄色 | `Color::Yellow` |
| 错误 | 红色 | `Color::Red` |
| 进度条已完成 | 蓝色 | `Color::Blue` |
| 进度条未完成 | 亮黑 | `Color::BrightBlack` |
| 分隔线 | 亮黑 | `Color::BrightBlack` |

**原则**:
- 暗色/亮色终端均兼容（避免纯黑/纯白）
- 颜色仅用于语义，非装饰

---

## 4. 符号系统

| 用途 | Unicode | ASCII | 颜色 |
|------|---------|-------|------|
| 成功 | ✓ | [OK] | Green |
| 失败 | ✗ | [FAIL] | Red |
| 跳过 | ⊘ | [SKIP] | BrightBlack |
| 重试 | ↻ | [RETRY] | Yellow |
| 进行中 | ● | [*] | Cyan |
| 信息 | ℹ | [INFO] | Cyan |
| 警告 | ⚠ | [WARN] | Yellow |
| 水平线 | ─ | - | BrightBlack |
| 垂直线 | │ | \| | BrightBlack |
| 左上角 | ┌ | + | BrightBlack |
| 右上角 | ┐ | + | BrightBlack |
| 左下角 | └ | + | BrightBlack |
| 右下角 | ┘ | + | BrightBlack |

**Fallback 策略**:
- 检测 `LANG`/`LC_ALL` 环境变量判断 Unicode 支持
- 非 Unicode 终端自动降级到 ASCII
- 提供 `--ascii` 强制降级选项

---

## 5. 组件设计

### 5.1 报告头部

**当前**:
```
================================
       合并报告
================================
```

**改进后**:
```
┌──────────────────────────────────┐
│  合并报告                        │
├──────────────────────────────────┤
│  总文件: 12    成功: 10          │
│  跳过: 2       耗时: 1m 23s      │
└──────────────────────────────────┘
```

- 标题左对齐
- 分隔线用 `bright_black`
- 统计数据表格化对齐

### 5.2 进度条

**当前**:
```
[cyan ██████████=>         blue] 10/100
```

**改进后**:
```
● 合并中...    ████░░░░░░  10/100  ETA 1m
```

- spinner: `●`/`○` 交替
- 已完成部分 `Blue`，未完成 `BrightBlack`
- 模板: `{spinner} {msg:20} {bar:20} {pos}/{len} {eta}`

### 5.3 预览列表

**当前**:
```
  file1.mp4 -> file1_merged.mp4
  file2.mp4 -> file2_merged.mp4
```

**改进后**:
```
  ┌─ 预览列表 (4 项) ─────────────┐
  │ 1. src1.mp4  →  out1.mp4      │
  │ 2. src2.mp4  →  out2.mp4      │
  │ 3. src3.mp4  →  out3.mp4      │
  │ 4. src4.mp4  →  out4.mp4      │
  └────────────────────────────────┘
```

- 文件名两列对齐（计算最大宽度）
- 序号用 `bright_black`
- 箭头 `→` 用 `cyan`

### 5.4 交互确认

**当前**:
```
继续吗？(y/N)
```

**改进后**:
```
? 继续吗？ › (Y/n)
```

- 默认选项高亮（`Y` 绿色，`n` 灰色）
- 提示符 `?` 用 `cyan`

### 5.5 错误消息

**当前**:
```
Error: 文件不存在
```

**改进后**:
```
✗ 错误: 文件不存在
  ├─ 文件: /path/to/file.mp4
  └─ 原因: No such file or directory
```

- 错误头用红色 `✗`
- 上下文用 tree 结构缩进
- 次要信息用 `bright_black`

---

## 6. i18n 新增 Key

**命名规范**:
- `report_*` → 报告相关
- `progress_*` → 进度条相关
- `preview_*` → 预览列表
- `prompt_*` → 交互确认
- `error_*` → 错误消息

**新增约 20-25 个 key**:
```
report_header: "合并报告" / "Merge Report"
report_total_files: "总文件: {}" / "Total: {}"
report_success: "成功: {}" / "Success: {}"
report_skipped: "跳过: {}" / "Skipped: {}"
report_duration: "耗时: {}" / "Duration: {}"
preview_list_title: "预览列表 ({} 项)" / "Preview ({} items)"
prompt_continue: "继续吗？" / "Continue?"
prompt_default_yes: "Y" / "Y"
prompt_default_no: "n" / "n"
error_prefix: "错误" / "Error"
error_context_file: "文件" / "File"
error_context_reason: "原因" / "Reason"
```

---

## 7. TDD 测试策略

| 测试类型 | 覆盖范围 |
|---------|---------|
| 快照测试 | 完整报告输出、预览列表格式 |
| 单元测试 | 符号输出、颜色代码、对齐计算 |
| i18n 测试 | 新增 key 中英翻译完整性 |
| TTY 模拟测试 | bar/text 模式输出差异 |
| Fallback 测试 | Unicode/ASCII 降级正确性 |

**测试文件**:
- `src/progress.rs` → 进度条模板、颜色、符号
- `src/merger.rs` → 报告头部格式、box-drawing
- `src/main.rs` → 预览列表对齐、交互确认
- `src/i18n.rs` → 新增 key 翻译
- `tests/ui_snapshot.rs` → 完整输出快照（新文件）

**快照测试规范化**:
- 剥离 ANSI 颜色代码后比较纯文本
- 忽略行尾空白
- 支持 `UPDATE_SNAPSHOTS=1` 环境变量更新

---

## 8. 向后兼容约束

**不变**:
- `--quiet` 模式无额外输出
- 非 TTY 环境 text 模式格式
- `--dry-run` 输出语义
- 现有 i18n key 含义

**可改**:
- TTY 环境下的视觉渲染
- 进度条模板（不影响功能）
- 报告头部装饰（不影响数据）

---

## 9. 串联 Workflow

```
阶段 1 (UX 研究员) → ux-audit-report.md
  ↓
阶段 2 (UI 设计师) → terminal-design-system.md
  ↓
阶段 3 (高级项目经理) → ui-spec-v1.md (Given/When/Then)
  ↓
阶段 4 (前端开发者) → 失败测试集 (TDD Red)
  ↓
阶段 5 (前端开发者) → 实现通过测试 (TDD Green)
  ↓
阶段 6 (现实检验者) → acceptance-report.md
```

**预估工作量**: ~3.75h，~12 文件变更

---

## 10. 风险与缓解

| 风险 | 缓解 |
|------|------|
| Unicode 终端兼容性 | Fallback 到 ASCII，提供 `--ascii` 选项 |
| 快照测试脆弱 | 规范化比较（剥离 ANSI，忽略空白） |
| i18n key 膨胀 | 按模块分组，命名规范 |
| 进度条闪烁 | 调整 `steady_tick` 间隔 |

---

## 11. 验收标准

- `cargo test` 全部通过
- `cargo clippy -- -D warnings` 无警告
- `cargo build --release` 成功
- 手动运行验证视觉效果
- i18n 切换验证（`LANG=zh_CN` / `LANG=en_US`）
- 非 TTY 环境验证（pipe）
- `--quiet` 模式验证
