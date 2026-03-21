# mixbilibili 综合优化设计规格

**日期**: 2026-03-22
**状态**: 草案
**范围**: 错误处理、测试、CI/CD、性能、代码质量、依赖、文档、安全

---

## 概述

本设计将 `mixbilibili` 项目的优化分为三个阶段，按优先级逐步推进。每个阶段完成后可独立验证，降低风险。

| 阶段 | 优先级 | 内容 | 预计改动 |
|------|--------|------|----------|
| P1 | 高 | 错误处理 + 测试 + CI/CD | 5个源文件 + CI配置 |
| P2 | 中 | 性能 + 代码质量 + 输入验证 | 4个源文件 |
| P3 | 低 | 依赖 + 文档 + 安全细节 | 3个文件 + 文档 |

---

## 阶段一 (P1): 高优先级优化

### 1.1 错误处理重构

**目标**: 使用 `anyhow` 统一错误处理，提供更好的错误上下文。

**变更**:

1. 添加依赖到 `Cargo.toml`:
   ```toml
   [dependencies]
   anyhow = "1.0"
   ```

2. 替换所有 `Result<T, String>` 为 `Result<T, anyhow::Error>`:
   - `src/scanner.rs`: `ScanResult::scan()` 返回类型
   - `src/merger.rs`: `MergeResult`, `execute_merges()` 等
   - `src/ffmpeg.rs`: `FFmpeg` 相关函数

3. 使用 `anyhow::Context` 添加上下文:
   ```rust
   // Before
   .map_err(|e| format!("Failed to read directory: {}", e))?

   // After
   .context("Failed to read directory")?
   ```

4. 统一退出码:
   - `0`: 成功
   - `1`: 通用错误
   - `2`: ffmpeg 未找到
   - `3`: 合并失败

**文件改动**:
- `Cargo.toml`
- `src/main.rs`
- `src/scanner.rs`
- `src/merger.rs`
- `src/ffmpeg.rs`

### 1.2 核心功能测试

**目标**: 为关键路径添加测试覆盖。

**新增测试**:

1. `src/scanner.rs` (补充):
   - 空目录扫描
   - 只有视频文件的情况
   - 正在下载的文件过滤 (aria2)

2. `src/merger.rs` (新增):
   - `delete_source_files` 正常删除测试
   - `delete_source_files` 文件不存在时的处理
   - `run_with_timeout` 正常完成测试
   - `run_with_timeout` 超时终止测试

3. `src/main.rs` (集成测试模块):
   - 完整合并流程测试（创建临时视频/音频文件）

**文件改动**:
- `src/scanner.rs`
- `src/merger.rs`
- `src/main.rs`

### 1.3 GitHub Actions CI/CD

**目标**: 建立自动化测试和发布流程。

**配置文件**: `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [master]
  pull_request:
    branches: [master]
  release:
    types: [created]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test

  build:
    needs: test
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - uses: actions/upload-artifact@v4
        with:
          name: binary-${{ matrix.os }}
          path: target/release/mixbilibili*

  release:
    needs: build
    if: github.event_name == 'release'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
      - name: Upload Release Assets
        # 上传到 GitHub Release
```

**新增文件**:
- `.github/workflows/ci.yml`

---

## 阶段二 (P2): 中优先级优化

### 2.1 性能优化

**目标**: 减少不必要的内存分配和重复资源创建。

**变更**:

1. 移除 pairs 克隆 (`src/merger.rs:179`):
   ```rust
   // Before
   let pairs = scan_result.pairs.clone();

   // After
   let pairs = &scan_result.pairs;
   ```

2. 线程池复用 (`src/merger.rs:182-185`):
   - 选项 A: 使用 `rayon` 全局线程池（推荐，改动最小）
   - 选项 B: 接受 `ThreadPool` 参数

3. 优化忙等待 (`src/merger.rs:23-35`):
   ```rust
   // 使用 Condvar 替代轮询
   use std::sync::{Arc, Condvar, Mutex};

   fn wait_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
       let pair = Arc::new((Mutex::new(false), Condvar::new()));
       // 实现细节...
   }
   ```

**文件改动**:
- `src/merger.rs`

### 2.2 代码质量

**目标**: 清理技术债务，提高可维护性。

**变更**:

1. 清理未使用代码 (`src/ffmpeg.rs`):
   - `Os` enum: 若无跨平台安装计划，移除
   - `InstallResult`: 同上
   - `ffmpeg_path()`: 若不使用，移除；或标记为未来功能

2. 消除魔法数字:
   ```rust
   // src/merger.rs 顶部
   const DEFAULT_TIMEOUT_SECS: u64 = 300;
   const POLL_INTERVAL_MILLIS: u64 = 100;
   ```

3. 可选: 将超时时间暴露为 CLI 参数:
   ```rust
   // src/cli.rs
   #[arg(long, default_value = "300")]
   pub timeout: u64,
   ```

**文件改动**:
- `src/ffmpeg.rs`
- `src/merger.rs`
- `src/cli.rs` (可选)

### 2.3 输入验证增强

**目标**: 提前发现无效输入，提供清晰错误信息。

**变更** (`src/cli.rs`):

```rust
impl Cli {
    fn validate(&self) -> Result<()> {
        // 现有: jobs 范围检查

        // 新增: 源目录检查
        if !self.source.exists() {
            bail!("Source directory does not exist: {}", self.source.display());
        }
        if !self.source.is_dir() {
            bail!("Source path is not a directory: {}", self.source.display());
        }

        // 新增: 输出目录检查
        if let Some(output) = &self.output {
            if !output.exists() {
                bail!("Output directory does not exist: {}", output.display());
            }
            if output.canonicalize()? == self.source.canonicalize()? {
                bail!("Source and output directories cannot be the same");
            }
        }

        Ok(())
    }
}
```

**文件改动**:
- `src/cli.rs`

---

## 阶段三 (P3): 低优先级优化

### 3.1 依赖管理

**目标**: 精确版本控制，减少不必要的依赖。

**变更**:

1. 精确版本锁定 (`Cargo.toml`):
   ```toml
   [dependencies]
   clap = { version = "4.5.0", features = ["derive"] }
   colored = "2.1.0"
   rayon = "1.10.0"
   tempfile = "3.10.0"
   which = "6.0.0"
   anyhow = "1.0.82"
   ```

2. 移除 `num_cpus`:
   ```rust
   // Before
   let default_jobs = num_cpus::get();

   // After
   let default_jobs = std::thread::available_parallelism()
       .map(|p| p.get())
       .unwrap_or(1);
   ```

**文件改动**:
- `Cargo.toml`
- `src/cli.rs`

### 3.2 文档完善

**目标**: 提高项目可维护性和用户友好度。

**新增文件**:

1. `CHANGELOG.md`:
   ```markdown
   # Changelog

   ## [Unreleased]

   ### Added
   - Comprehensive error handling with anyhow
   - GitHub Actions CI/CD pipeline
   - Core functionality tests

   ### Changed
   - Improved error messages with context
   - Exit codes now differentiate error types
   ```

2. `CONTRIBUTING.md`:
   ```markdown
   # Contributing

   ## Development Setup
   1. Install Rust via rustup
   2. Run `cargo test` to verify setup

   ## Code Style
   - Run `cargo fmt` before committing
   - Run `cargo clippy` and fix all warnings

   ## Pull Requests
   - Ensure CI passes
   - Add tests for new functionality
   ```

3. Rustdoc 注释到公开函数:
   ```rust
   /// Scans a directory for video/audio file pairs.
   ///
   /// # Arguments
   /// * `source` - Directory to scan
   ///
   /// # Returns
   /// A `ScanResult` containing matched pairs and any errors.
   ///
   /// # Errors
   /// Returns an error if the directory cannot be read.
   pub fn scan(source: &Path) -> Result<ScanResult> { ... }
   ```

**文件改动**:
- `src/scanner.rs`
- `src/merger.rs`
- `src/ffmpeg.rs`
- `src/cli.rs`

### 3.3 安全细节

**目标**: 修复潜在竞态条件。

**变更** (`src/main.rs:57-64`):

```rust
// Before: 先测试写权限再删除 (TOCTOU)
let test_file = source.join(".mixbilibili_test");
File::create(&test_file)?;
fs::remove_file(&test_file)?;

// After: 原子创建
use std::fs::OpenOptions;
let test_file = source.join(".mixbilibili_test");
match OpenOptions::new()
    .write(true)
    .create_new(true)  // 原子创建，失败则文件已存在
    .open(&test_file)
{
    Ok(_) => fs::remove_file(&test_file)?,
    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
        // 文件已存在，说明可写
        fs::remove_file(&test_file)?;
    }
    Err(e) => return Err(e).context("Cannot write to source directory"),
}
```

**文件改动**:
- `src/main.rs`

---

## 实施顺序

```
P1.1 错误处理重构
  ↓
P1.2 核心功能测试
  ↓
P1.3 CI/CD 配置
  ↓
P2.1 性能优化
  ↓
P2.2 代码质量
  ↓
P2.3 输入验证增强
  ↓
P3.1 依赖管理
  ↓
P3.2 文档完善
  ↓
P3.3 安全细节
```

---

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 错误处理改动影响现有逻辑 | 先写测试，再重构 |
| 性能优化引入新 bug | 保持简单实现，避免过度优化 |
| CI 配置复杂度 | 使用成熟模板，逐步扩展 |

---

## 成功标准

- [ ] 所有现有功能正常工作
- [ ] 测试覆盖核心路径
- [ ] CI 在 PR 时自动运行
- [ ] 错误信息清晰有上下文
- [ ] 无 clippy 警告
- [ ] 文档完整