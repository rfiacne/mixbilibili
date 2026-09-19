# mixbilibili 系统优化实施计划(阶段一 + 阶段二)

基线:codebase-memory 知识图谱文档(docs/2026-08-13-architecture-and-feature-roadmap.md)已与源码逐条验证(6648 行,论断全部属实),另发现文档未提的「递归 a/b 与平铺 a_b 输出同名静默覆盖」数据丢失隐患。全程零新依赖,不破坏 AGENTS.md 十条硬约束。

## 阶段一:架构还债

### 1. CHANGELOG 补 0.8.0
- 从 `git log v0.7.0..HEAD` 归纳条目(CJK 显示修复、递归扫描加固、源覆盖保护、SIGCHLD 复位等),在 CHANGELOG.md 顶部新增 `## [0.8.0]` 段(release workflow 依赖此段提取 tag notes)。

### 2. main.rs 拆分(1137 行 → 约 60 行)
- 新建 `src/error.rs`:`exit_codes` 常量、`AppError`、`get_exit_code`、`translate_error`(原 main.rs:19-73)。退出码 1/2/3 映射保持稳定(硬约束 9)。
- 新建 `src/workflow.rs`:`run()`(含 SIGCHLD 复位、Ctrl+C 注册、4 阶段编排)、`init`、`ScanContext` + `format_time_estimate`、`scan_and_filter`、`execute`、`finalize`、`run_once`、`parse_prompt_response`、`STATE_SAVE_INTERVAL`。
- `validate_stem` → `cli.rs`(参数验证,符合 cli 模块边界;不弱化路径遍历防护)。
- `print_preview_pairs`、`file_name_width` → `ui.rs`(纯展示);`print_preview_pairs` 改为接收普通参数(pairs/stats/时间),不依赖 workflow 的 ScanContext,保持 ui 边界纯净。
- `main.rs` 只留:`main()`、`pub static INTERRUPTED`(merger 引用它,位置不变)、mod 声明。
- 原 25 个测试随代码整体迁移(exit/AppError → error.rs;run_once/finalize/prompt → workflow.rs;width/preview → ui.rs),断言内容不变。
- 同步更新 AGENTS.md 模块职责表(新增 workflow、error 行)。

### 3. stem 规范化统一 + 输出名碰撞修复(数据丢失修复)
- `scanner.rs`:提取闭包(scanner.rs:69-84)为 `pub(crate) fn make_key(recursive, relative_parent, stem) -> String`,新增 `pub(crate) fn safe_stem(key) -> String`(replace '/'→'_'),键规则全项目仅此一份。
- `merger.rs:280` 与 run_once 清理路径(原 main.rs:485,当前漏做 replace)统一改用 `scanner::safe_stem`。
- 碰撞修复:`scan_directory` 配对后对 safe_stem 查重,冲突即 bail(列出两个原始 key + 提示改名,新增 2 条翻译),杜绝静默覆盖。

### 4. i18n 完整性测试期强制(零依赖)
- `i18n.rs` 新增测试:(a) TRANSLATIONS 中每个 key 中英成对;(b) 扫描 src/*.rs 中 `t("…")`/`tf("…")` 字面量 key,断言全部存在(借助 `t_for` 绕过运行时语言检测)。约 30 行,CI 的 cargo test 自动强制翻译完整性,key 拼错从"运行时静默回退"变为"测试失败"。

## 阶段二:核心健壮性

### 5. merger 执行路径可注入 + ffmpeg 路径修复(解决 2.3)
- `merger.rs`:`merge_pair`/`do_merge` 增加 `runner: Option<&dyn Fn(&mut Command, Duration) -> Result<ExitStatus>>` 参数,None 用现有 `run_with_timeout`;现有调用点(execute、run_once)不改。
- 新增 fake-runner 单测:成功、失败重试后成功、超时错误映射——核心合并逻辑从此不依赖真实 ffmpeg。
- `ffmpeg.rs` `build_merge_command`:`Command::new(which::which("ffmpeg").unwrap_or("ffmpeg"))`,消除 PATH 精简场景(aria2 hook)下定位不一致;调整 `test_command_program_is_ffmpeg` 断言。
- ponytail:不做 merger 物理拆分子模块——当前单一职责尚清晰,拆分是纯 churn;待 merger 增加新职责时再拆。

### 6. state 版本化入口
- `state.rs`:引入 `pub const CURRENT_SCHEMA_VERSION: u32 = 1`,`load()` 检查改为 `> CURRENT_SCHEMA_VERSION`(行为与报错不变,硬约束 10 满足),`new()` 用常量。
- ponytail:不做逐版本迁移链和 per-stem size/hash 锚点——尚无 v2 schema 也无消费方,现在写是死代码;首个真实 schema 变更落地时随迁移一起实现。

### 7. 输出校验升级——明确跳过
- ponytail:保持"存在 + 非零"校验。`-c copy` 下 ffmpeg 退出码是权威信号;size/hash 启发式在容器开销差异(MP4 moov vs MKV)下会误报。待出现"exit 0 但产物损坏"的真实案例再引入。

## 验证(全部完成后)
- `cargo fmt --check` && `cargo clippy -- -D warnings` && `cargo test`(与 CI 一致)
- 手动回归:递归扫描含子目录目录、`--once` 合法/非法 stem、Ctrl+C 中断恢复、`--resume` 读旧状态文件(schema 兼容)
- 确认退出码映射与 `.mixbilibili_state.json` schema 未变(硬约束 9/10)

## 明确不做
转码、字幕/封面嵌入、watch 模式、配置化、merger 物理拆分、state v2 schema。