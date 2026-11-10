[English](README.md) | [简体中文](README.zh-CN.md)

# oak-mcp

一个 [Model Context Protocol](https://modelcontextprotocol.io) 服务器，把
[Quercus](https://github.com/OakVideoEditorCommunity/quercus-engine) 非线性视频剪辑引擎
暴露给 LLM Agent。它依赖 crates.io 上已发布的 `quercus-engine-rs`，通过 stdio 讲 MCP。

Agent 可以通过 MCP 工具调用完成：新建工程、导入素材、放置/裁剪/切割片段、构建效果链和关键帧、
渲染预览帧（以 PNG 返回）、异步导出成片，以及 OpenTimelineIO 往返。

MCP 协议层使用官方 [`rmcp`](https://crates.io/crates/rmcp) SDK；`src/tools/` 里只有面向引擎的
工具，所以新增工具不需要改协议代码。

## 工具

共 50 个工具，分七组（每个工具都有 JSON Schema 和只读/破坏性标注）：

| 组 | 工具 |
|---|---|
| 工程与会话 | `project_new`、`project_open`、`project_save`、`project_info`、`session_list`、`session_use`、`session_close` |
| 序列与时间线 | `sequence_create`、`sequence_use`、`sequence_set_format`、`timeline_get`、`playhead_set`、`track_add`、`track_set`、`marker_add`、`markers_list`、`workarea_set` |
| 媒体 | `media_import`、`media_list` |
| 片段 | `clip_place`、`clip_place_linked`、`clip_move`、`clip_trim`、`clip_split`、`clip_delete`、`clip_speed`、`clip_slide`、`clip_slip`、`clip_set`、`transition_add` |
| 效果 | `effect_types`、`effect_add`、`effect_remove`、`effect_enable`、`effect_chain`、`effect_params`、`effect_set_param`、`effect_keyframe` |
| 渲染与导出 | `preview_frame`、`render_audio`、`export_start`、`task_status`、`task_wait`、`task_cancel`、`task_list`、`otio_export`、`otio_import` |
| 历史 | `undo`、`redo`、`history` |

资源：`quercus://server/info`、`quercus://session/{id}/timeline`、`quercus://session/{id}/project`。
提示词：`rough_cut`、`deliver_export`，用于把推荐的工具调用顺序打包给模型。

### 约定

- **时间单位是秒**（`f64`）。服务器负责转换成引擎的有理数时间基和按序列帧率的时间戳。
- **节点 ID 是不透明的十进制字符串**。始终使用工具返回的 ID；ID 按会话生成，保存/重开后不保证稳定。
- 所有会话级工具都接受 `session`；省略时使用活动会话。
- 工具失败返回 `isError: true` 的内容（不是 JSON-RPC 错误），模型能读到并重试。未知工具和格式错误的请求才是真正的 JSON-RPC 错误（`-32602`、`-32601` 等）。
- 导出是异步的：`export_start` 返回任务 ID，然后用 `task_status` 轮询或 `task_wait` 阻塞等待。

## 用 DeepSeek Harness 直接生成

工作室 Skill 在独立仓库：
[oak-and-quercus/mcp-skill](https://atomgit.com/oak-and-quercus/mcp-skill)
（`git@atomgit.com:oak-and-quercus/mcp-skill.git`）。装到 DSH 的 Skill 目录后，
在 DSH 里打开这个仓库，用自然语言描述工作室的流程即可：Agent 会负责 FFmpeg 环境、
写工具、构建、用真实素材测试，最后交付打包好的 MCP 服务。它只会问品牌、操作系统/客户端、
重复性工作流和交付默认值，不会让工作室的人配环境或写代码。

想直接看代码的开发者，可以读
[docs/tutorial-customize-example-mcp.md](docs/tutorial-customize-example-mcp.md)。

## 环境要求

- Rust 1.88+（edition 2024；`rmcp` 要求 1.88）。
- **FFmpeg 9.0**：Quercus 固定的工程构建（静态前缀 + `lib/pkgconfig`）。**不支持**发行版/系统自带的 FFmpeg。
  Quercus 的构建脚本已经拷贝到本仓库 `scripts/` 下，**不需要**再准备 Quercus checkout：
  - **Linux/macOS**：先运行一次 `./scripts/install-deps.sh`（构建工具 + 系统库；可能要求 sudo/Homebrew），
    再运行 `./scripts/ffmpeg/build-ffmpeg.sh`，产物在 `.cache/ffmpeg`。
  - **Windows**：运行 `. .\scripts\fetch-ffmpeg.ps1`（注意前面的点）。它会下载 Quercus CI 使用的同一个
    BtbN/FFmpeg-Builds 预构建包，校验 SHA-256，解压到 `.cache\ffmpeg`。
- **OpenColorIO 2.x** 开发文件（`libOpenColorIO.so` + 头文件）。
  `quercus-core 0.5.2` / `quercus-render 0.5.2` 会启用 ocio-rs 0.2 的 `bundled` 特性，其内置 yaml-cpp
  在 GCC >= 13 下编译失败（缺少 `#include <cstdint>`）。本仓库改为链接系统 OCIO：
  `scripts/build.sh` 在 Linux 上设置 `OCIO_INSTALL_DIR=/usr`、`OCIO_RS_LINK=dynamic`；
  Windows 脚本设置 CI 使用的 `OCIO_RS_ENABLE_REAL=1`、`OCIO_RS_LINK=static`、`CC=cl`、`CXX=cl`。
  macOS 请 `export OCIO_INSTALL_DIR="$(brew --prefix opencolorio)"`。
  如果一定要用 bundled 版本，请补上缺失的 include（例如 `CXXFLAGS="-include cstdint"`）并取消 `OCIO_INSTALL_DIR`。

`FFMPEG_DIR` 不写进仓库。`scripts/build.sh` 会自动使用 `.cache/ffmpeg`；Windows 脚本会自动设置环境变量。

### 镜像

GitHub 克隆或 BtbN 下载慢时，可以用镜像（自动化流程里先询问再切换）：

- example-mcp：`https://atomgit.com/oak-and-quercus/example-mcp.git`
- FFmpeg 9.0 Windows 包：镜像暂未发布（AtomGit 目前只有 8.1 归档）。在 v9.0 镜像上线前，
  请使用 BtbN 主源，或在 Linux/macOS 上用 `./scripts/ffmpeg/build-ffmpeg.sh` 从源码构建。

FFmpeg 镜像只用于 Windows。AtomGit/Cloudflare 可能没有上游 checksum；
`fetch-ffmpeg.ps1` 此时要求加 `-AllowUnverified`，并打印 SHA-256。

## 构建

### Linux / macOS

```sh
./scripts/install-deps.sh           # 只跑一次；构建工具 + 系统库（sudo/brew）
./scripts/ffmpeg/build-ffmpeg.sh    # 只跑一次；固定 FFmpeg 9.0 -> .cache/ffmpeg
./scripts/build.sh build --release
./target/release/oak-mcp --check    # 引擎加载、序列和渲染管理器可用
```

`scripts/build.sh` 会设置 OCIO 环境、自动使用 `.cache/ffmpeg`，并把参数转发给 cargo
（`build`、`test` 等）。如果你的前缀不同，覆盖 `FFMPEG_DIR` 或 `OCIO_INSTALL_DIR`。

### Windows（PowerShell）

需要：MSVC C++ Build Tools、Rust `stable-x86_64-pc-windows-msvc`、Git for Windows（提供
`link.exe` 和常用工具链）。

```powershell
. .\scripts\fetch-ffmpeg.ps1       # 前面的点很重要：保留环境变量
cargo build --release
.\target\release\oak-mcp.exe --check
```

脚本会从 [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest)
下载 `ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip`，校验 `checksums.sha256`，解压到
`.cache\ffmpeg`，并设置 `FFMPEG_DIR`、`PKG_CONFIG_PATH`、`PATH`、`OCIO_RS_ENABLE_REAL=1`、
`OCIO_RS_LINK=static`、`CC=cl`、`CXX=cl`。逻辑与 Quercus 的 Windows CI 一致。

GitHub 慢或被墙时，先询问用户，再切换：

```powershell
. .\scripts\fetch-ffmpeg.ps1 -Source AtomGit -AllowUnverified
# AtomGit 也不稳定时
. .\scripts\fetch-ffmpeg.ps1 -Source Cloudflare -AllowUnverified
```

### 不用脚本时的手工步骤

**Windows。** 自己下载并解压：

```powershell
$base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
$asset = "ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"
# 9.0 的镜像暂未发布，先用主源。
Invoke-WebRequest "$base/checksums.sha256" -OutFile "$env:TEMP\checksums.sha256"
# 在 checksums.sha256 里找到 $asset 那一行，记下 SHA-256
Invoke-WebRequest "$base/$asset" -OutFile "$env:TEMP\$asset"
(Get-FileHash "$env:TEMP\$asset" -Algorithm SHA256).Hash  # 对比
Expand-Archive "$env:TEMP\$asset" -DestinationPath .cache\ffmpeg-extract -Force
# 把压缩包里的顶层目录移出来：
Move-Item .cache\ffmpeg-extract\ffmpeg-n9.0-latest-win64-gpl-shared-9.0 .cache\ffmpeg
$env:FFMPEG_DIR = "$PWD\.cache\ffmpeg"
$env:PKG_CONFIG_PATH = "$PWD\.cache\ffmpeg\lib\pkgconfig"
$env:PATH = "$PWD\.cache\ffmpeg\bin;$env:PATH"
$env:OCIO_RS_ENABLE_REAL = "1"
$env:OCIO_RS_LINK = "static"
$env:CC = "cl"
$env:CXX = "cl"
cargo build --release
```

如果压缩包里的顶层目录名不同，把 `Move-Item` 里的名字换成实际的。

**Linux/macOS。** 安装构建工具（`cmake`、`nasm`、`pkg-config`、`freetype`、`fontconfig`、
C/C++ 工具链），然后按 `scripts/ffmpeg/build-ffmpeg.sh` 的同样步骤：把所有编解码库装进
`.cache/ffmpeg`，用 `--prefix=$PWD/.cache/ffmpeg --pkg-config-flags=--static` 配置并安装
FFmpeg 9.0。**强烈建议直接用仓库里的脚本**，因为编解码器集合和编译参数是固定的，手工很容易漏。

如果 `$CARGO_HOME` 不可写（沙盒 CI），用项目内目录：

```sh
CARGO_HOME=$PWD/.cargo-home cargo build --release
```

二进制在运行时会链接引擎共享库（`libquercus_engine.so`）。`build.rs` 会加 `$ORIGIN` rpath，
SDK 也会把库复制到二进制旁边，所以 `target/{debug,release}/oak-mcp` 可以直接运行。
用 `cargo install` 时，需要把动态库放在可执行文件旁或 loader 路径上。

## 接入 MCP 客户端

stdio 传输，无参数：

```json
{
  "mcpServers": {
    "oak": {
      "command": "/path/to/oak-mcp/target/release/oak-mcp",
      "args": []
    }
  }
}
```

典型 Agent 会话：

```
project_new {width: 1920, height: 1080, fps: 30}
media_import {path: "/media/a.mov"}
clip_place {footage: "10", start: 0, end: 5}
preview_frame {time: 1.0}          -> PNG image
export_start {filename: "/tmp/out.mp4"}
task_wait {task: "t1", timeout_ms: 120000}
```

## 源码结构

| 路径 | 作用 |
|---|---|
| `src/main.rs` | CLI、`rmcp` stdio 启动、`--check` |
| `src/server.rs` | `rmcp` `ServerHandler`：工具/资源/提示词映射 |
| `src/state.rs` | 会话、异步任务表、渲染初始化 |
| `src/tools/` | 按领域分组的工具定义和 handler |
| `src/convert.rs` | DTO → JSON、秒 ↔ 帧时间戳 |
| `src/image.rs` | F32 RGBA → PNG |
| `src/sidecar.rs` | `.ove` 序列化器丢失的格式/标记持久化 |
| `scripts/ffmpeg/` | 从 Quercus 拷贝的 FFmpeg 构建脚本（Linux/macOS） |
| `scripts/fetch-ffmpeg.ps1` | Windows BtbN FFmpeg 下载脚本（校验 SHA-256） |
| `scripts/install-deps.sh` | 构建工具/系统库（Linux/macOS） |

进程是单线程处理请求：请求按顺序处理，引擎任务跑在 Quercus 自己的 worker 上，
`task_wait` 只阻塞当前请求。

## 新增工具

工具就是声明：在对应的 `src/tools/*.rs` 里加一个 `ToolDef`（名字、描述、JSON Schema、handler），
再把它加到该模块的 `tools()` 列表，`rmcp` 就会发布它——不需要碰协议代码。
handler 通过 `Args` 解析参数，返回 `ToolReply::{json,image,error}`。

## 测试

```sh
./scripts/build.sh test                      # 单元 + 协议集成测试
python3 scripts/e2e_smoke.py                 # 新建 -> 导入 -> 放置 -> 预览 -> 导出
python3 scripts/e2e_edit.py                  # 效果、关键帧、切割/裁剪、保存重开、标记、范围导出、资源、提示词
```

两个 e2e 脚本都通过 stdio 驱动真实二进制，并用 `ffprobe` 校验导出文件。它们需要一个小测试素材：
把 `OAK_TEST_MEDIA` 指向一个文件（例如 Quercus checkout 里的 `testdata/demo.mp4`），或用 `--media` 传入。

## 这里处理的引擎限制

- **引擎的 `.ove` writer 不保存序列格式和标记。** `project_save` 额外写
  `<project>.oak-mcp.json`（每个序列的格式 + 当前序列的标记），`project_open` 再恢复，
  并报告 `sidecar_restored` 和警告。删除 sidecar 就用引擎默认值打开。
- **`SequenceSnapshot::length` 对 SDK 生产者是过期的缓存值**（App 会刷新它）。
  导出长度和报告的 `content_length` 都从片段推导。
- **导出编码器要的是帧时长，不是帧率**，所以服务器会反转序列帧率再填 `EncodingParams`。
- **撤销是进程级的**；`undo`/`redo` 影响最近一次编辑，跨会话共享。这是上游行为，不是服务器选择。
- 预览帧被钳制为 8-bit RGBA PNG；不表示 HDR/宽色域输出。

## 已知上游引擎缺陷

这些是在已发布 crate（`quercus-engine-rs 0.5.7`、`quercus-engine 0.5.3`、内部 crate `0.5.2`）上复现的；
`oak-mcp` 会尽量绕开：

- **从视频容器导出音频是静音的**（`quercus-task 0.5.2` 在导出音频 montage 里硬编码
  `stream_index: 0`，而视频文件的 stream 0 是视频）。`render_audio` 走的是正确的预览音频路径。
  workaround：先把时间线混音渲染成 WAV，再把 WAV 放到第二条音轨导出。
- **关键帧效果在预览里动，导出不动**：导出 montage 只序列化静态参数值。
- **静音的音频轨在导出时仍会被混入。**
- **纯音频导出（WAV）失败**：报 `Failed to write frame`，因为导出仍然渲染并写视频帧。
- **长时间 `render_audio` 后在同一进程里导出可能挂起**（33.7s 时间线保持 `running`，CPU 空闲）。
  在独立进程里渲染音频，再在新进程里导出。
- **`.ove` 保存/重开丢失序列格式、标记和工作区**（`SequenceBehavior::save_custom` 只写轨道列表）——
  见上面的 sidecar workaround。
- **`SequenceSnapshot::length` 对 SDK 生产者是过期的**；导出长度已改为从片段推导。

## 许可证

GPL-3.0-or-later，与 `quercus-engine-rs` 和 Quercus 引擎一致。
对外分发定制版服务器也必须保持 GPL-3.0-or-later；源码文件带 SPDX 头。
