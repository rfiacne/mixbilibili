# `mixbilibili` 命令行程序开发文档

## 1. 项目概述
* **项目名称**: `mixbilibili`
* **开发语言**: Rust
* **项目目标**: 开发一个跨平台的命令行工具，用于批量合并 Bilibili 下载的视频（`.mp4`）与音频（`.m4a`）文件。
* **核心依赖**: 程序依赖外部工具 `ffmpeg` 进行音视频混流。

## 2. 命令行参数设计 (CLI Specification)
程序应默认在**当前执行目录**下工作。建议使用 `clap` 库（Derive 模式）来解析命令行参数。

| 参数短名 | 参数长名 | 类型 | 默认值 | 描述 |
| :--- | :--- | :--- | :--- | :--- |
| `-s` | `--source` | `PathBuf` | `.` (当前目录) | 指定包含需要合并的 mp4 和 m4a 文件的输入目录。 |
| `-o` | `--output` | `PathBuf` | `.` (当前目录) | 指定合并后 mkv 文件的输出目录。如果不存在需自动创建。 |
| `-d` | `--sdel` | `bool` | `true` | 是否在成功合并后删除源文件（`.mp4` 和 `.m4a`）。 |

## 3. 核心工作流 (Core Workflow)

### 3.1 阶段一：环境检查与依赖安装
程序启动后，首先必须检查宿主机是否已安装 `ffmpeg`。
* **检查逻辑**: 在系统 `PATH` 中查找是否存在 `ffmpeg` 可执行文件（推荐使用 `which` crate）。
* **安装逻辑**: 如果未找到 `ffmpeg`，程序需识别当前操作系统（Windows, macOS, Linux）并尝试调用系统包管理器进行自动化安装。
  * **Windows**: 尝试使用 `winget install ffmpeg` 或 `choco install ffmpeg`。
  * **macOS**: 尝试使用 `brew install ffmpeg`。
  * **Linux**: 尝试使用 `sudo apt update && sudo apt install ffmpeg` (Debian/Ubuntu) 或相应的包管理器。
* **异常处理**: 如果自动安装失败，应输出清晰的错误提示，指导用户手动安装 `ffmpeg` 并退出程序。

### 3.2 阶段二：文件扫描与状态校验
扫描 `-s` (source) 目录中的文件，寻找成对的音视频文件。
1. **配对识别**: 寻找文件名相同但扩展名分别为 `.mp4` 和 `.m4a` 的文件组合（例如 `video1.mp4` 和 `video1.m4a`）。
2. **下载状态校验（aria2 拦截）**: 对于找到的每一对文件，检查同级目录下是否存在同名的 `.aria2` 控制文件（如 `video1.aria2`，或者 `video1.mp4.aria2` / `video1.m4a.aria2`）。
3. **跳过逻辑**: 如果存在对应的 `.aria2` 文件，说明该文件组正在下载中，**必须跳过**这对文件的合并操作。

### 3.3 阶段三：执行合并 (FFmpeg 调用)
对通过校验的文件对，调用系统的 `ffmpeg` 进程执行合并。
* **执行命令模板**:
  ```bash
  ffmpeg -hide_banner -loglevel error -i "{source_video}.mp4" -i "{source_audio}.m4a" -c:v copy -c:a copy -movflags +faststart -y "{output_dir}/{name}.mkv"
  ```
* **参数说明**: 
  * `-c:v copy -c:a copy`: 仅复制音视频流，不进行重新编码，保证速度和无损。
  * `-movflags +faststart`: 优化视频结构，便于网络流媒体播放。
  * `-y`: 自动覆盖输出目录中已存在的同名文件。

### 3.4 阶段四：清理工作
监控 `ffmpeg` 进程的执行状态：
* 如果 `ffmpeg` 退出状态码为 `0`（成功），且参数 `-sdel` 为 `true`，则删除源文件（`{name}.mp4` 和 `{name}.m4a`）。
* 如果 `ffmpeg` 执行失败，保留源文件，并在控制台输出错误日志。

## 4. 推荐的 Rust 依赖库 (Crates)
为了高效完成开发，建议 Claude Code 在 `Cargo.toml` 中引入以下依赖：
* `clap` (features = ["derive"]): 用于解析和构建命令行参数。
* `which`: 用于跨平台检测 `ffmpeg` 是否存在于系统 PATH 中。
* `walkdir` 或标准库 `std::fs`: 用于遍历目录和读取文件列表。
* `std::process::Command`: 用于衍生子进程调用 `ffmpeg` 和包管理器。

## 5. 开发注意事项
* **路径处理**: 跨平台路径分隔符可能会有差异，请严格使用 Rust 的 `std::path::PathBuf` 进行路径拼接和解析，避免使用硬编码的字符串拼接。
* **并发处理 (可选)**: 如果文件数量较多，可以考虑使用 `rayon` 或 `tokio` 进行并发合并以提升效率，但需注意控制并发数以防 I/O 阻塞或 CPU 满载。
* **容错性**: 目标文件夹 (`-o`) 如果不存在，程序应当在合并前自动创建该文件夹。