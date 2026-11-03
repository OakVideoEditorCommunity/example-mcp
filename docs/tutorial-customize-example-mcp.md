# 从 example-mcp 定制你自己的 MCP 服务

> 面向影视工作室的实战教程。假设你会用终端、装过软件，但不需要懂 MCP 协议或 Rust 高级特性。
>
> 读完你会得到一个可以分发的 `my-studio-mcp`：保留 example-mcp 的全部剪辑工具，加上你自己的工作室工具、提示词和品牌。
>
> 代码与 example-mcp 相同，许可为 **GPL-3.0-or-later**。
>
> 如果你只是想让 DeepSeek Harness 直接生成，不需要读这篇教程：安装独立仓库
> [oak-and-quercus/mcp-skill](https://atomgit.com/oak-and-quercus/mcp-skill) 的
> `build-studio-mcp` Skill，用自然语言描述工作室流程即可。

---

## 0. 最终结构

```
MCP 客户端（Claude / Cursor / 自研 Agent）
        │  stdio + MCP
        ▼
src/server.rs        ← rmcp 负责协议、生命周期、序列化（不要改）
        │
        ▼
src/tools/*.rs       ← 你 99% 的时间花在这里：ToolDef + handler
        │
        ▼
quercus-engine-rs    ← 剪辑、调色、预览、导出
```

三条规则：

1. **不要写协议代码**。`rmcp` 已经处理 initialize、tools/list、tools/call、resources、prompts、错误码。
2. **一个工具只做一件事**，但可以是“工作室的一步”（例如“导入整个素材文件夹”），而不是“一个引擎 API”。
3. **先读一个现成工具再写新的**。`src/tools/media.rs` 是最小、最干净的模板。

---

## 1. 准备（约 30 分钟）

### 1.1 Fork / 克隆

```bash
git clone https://github.com/OakVideoEditorCommunity/example-mcp.git my-studio-mcp
cd my-studio-mcp
```

如果你在 GitHub 上 fork，把上面的 URL 换成你自己的仓库。**整个仓库（尤其 `scripts/`）要一起拷走**：
构建 FFmpeg、Windows 预构建下载和 e2e 测试都依赖它。

### 1.2 依赖

- **Rust 1.88+**（`rmcp 3.8` 的最低要求）。
- **Quercus 固定的 FFmpeg 8.1 静态前缀**（系统 FFmpeg 不行）。
- **OpenColorIO 2.x** 开发文件。

仓库已经带上 Quercus 的 FFmpeg 构建脚本，不需要再准备 Quercus checkout。

**Linux / macOS**（只做一次）：

```bash
./scripts/install-deps.sh           # 构建工具 + 系统库，可能要求 sudo/brew
./scripts/ffmpeg/build-ffmpeg.sh    # 固定 FFmpeg 8.1 -> .cache/ffmpeg
```

**Windows**（只做一次，PowerShell）：

```powershell
. .\scripts\fetch-ffmpeg.ps1       # 从 BtbN 下载预构建包并校验 SHA-256
```

它会把 FFmpeg 解压到 `.cache\ffmpeg`，并设置 `FFMPEG_DIR`、`PKG_CONFIG_PATH`、
`OCIO_RS_ENABLE_REAL`、`OCIO_RS_LINK=static`、`CC/CXX=cl`。细节和手工步骤见 README 的
“Windows（PowerShell）”和“不用脚本时的手工步骤”。

### 1.3 第一次构建

**Linux / macOS：**

```bash
./scripts/build.sh build          # 自动用 .cache/ffmpeg，并带 OCIO 环境变量
./target/debug/oak-mcp --check
```

**Windows：**

```powershell
. .\scripts\fetch-ffmpeg.ps1
cargo build --release
.\target\release\oak-mcp.exe --check
```

`--check` 会打印 JSON：引擎能创建 session、创建 sequence、初始化渲染管理器、加载 50 个工具。失败时它返回非 0，先解决再往下走。

> Linux 上 `scripts/build.sh` 默认 `OCIO_INSTALL_DIR=/usr`、`OCIO_RS_LINK=dynamic`。macOS/Homebrew 用：
> `export OCIO_INSTALL_DIR=$(brew --prefix opencolorio)`。

### 1.4 接到 MCP 客户端

Claude Desktop / Cursor / VS Code 的配置都是同一段 JSON：

```json
{
  "mcpServers": {
    "my-studio": {
      "command": "/absolute/path/to/my-studio-mcp/target/debug/oak-mcp",
      "args": []
    }
  }
}
```

用绝对路径；`command` 指向二进制，不是 `cargo run`。

### 1.5 冒烟测试

最直观的是 MCP Inspector：

```bash
npx @modelcontextprotocol/inspector ./target/debug/oak-mcp
```

在界面里：

1. `tools/list` 应该看到 50 个工具；
2. 调 `project_new {width: 1280, height: 720, fps: 30}`；
3. 调 `media_import {path: "/绝对路径/某素材.mp4"}`；
4. 调 `preview_frame {time: 1.0}`，应该返回一张 PNG；
5. 调 `export_start {filename: "/tmp/test.mp4"}`，再 `task_wait`。

没有 Inspector 也可以直接跑仓库自带测试：

```bash
./scripts/build.sh test
python3 scripts/e2e_smoke.py
```

---

## 2. 目录导览：只改两个地方

| 路径 | 作用 | 你要改吗 |
|---|---|---|
| `src/main.rs` | CLI、rmcp stdio 启动、`--check` | 改名时改帮助文本 |
| `src/server.rs` | rmcp `ServerHandler`：工具/资源/提示词映射、品牌、instructions | 只改品牌和 prompts |
| `src/tools/mod.rs` | 工具注册表、`ToolDef`、`Args`、`ToolReply` | 加模块时改一行 |
| `src/tools/project.rs` | 工程/序列/标记/工作区工具 | 参考 |
| `src/tools/media.rs` | 导入、工程箱、代理、多机位 | **先读这个** |
| `src/tools/clips.rs` | 放置/裁剪/转场/字幕 | 参考 |
| `src/tools/effects.rs` | 效果链/参数/关键帧 | 参考 |
| `src/tools/render.rs` | 预览、音频、导出、任务 | 参考 |
| `src/tools/history.rs` | 撤销/重做 | 参考 |
| `src/state.rs` | 会话、异步任务表 | 需要新状态时改 |
| `src/convert.rs` | 时间/ID/快照 JSON | 一般不改 |
| `tests/protocol.rs` | 真实 stdio 的集成测试 | 加测试时改 |
| `scripts/` | 构建 + e2e | 改名时改二进制路径 |

**不要**在 `src/tools/` 里手写 JSON-RPC、读 stdin、拼协议消息。工具只需要：

```rust
fn my_tool(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String>
```

---

## 3. 改名与品牌

### 3.1 Cargo.toml

```toml
[package]
name = "my-studio-mcp"
version = "0.1.0"
description = "My Studio's MCP server for Quercus"
license = "GPL-3.0-or-later"
repository = "https://github.com/your-org/my-studio-mcp"

[[bin]]
name = "my-studio-mcp"
path = "src/main.rs"
```

### 3.2 改名检查清单

| 位置 | 改什么 |
|---|---|
| `Cargo.toml` | `package.name`、`[[bin]].name`、description、repository |
| `src/server.rs` | `Implementation::new("my-studio-mcp", ...)`、`title`、`INSTRUCTIONS` |
| `src/main.rs` | `--help` 文本、`--version` 前缀 |
| `tests/protocol.rs` | `env!("CARGO_BIN_EXE_oak-mcp")` 改成新二进制名（见下） |
| `scripts/e2e_smoke.py` | `--bin` 默认值 |
| `scripts/e2e_edit.py` | `BIN` 常量 |
| `README.md` | 标题、安装/配置示例 |
| MCP 客户端配置 | `command` 路径 |

`tests/protocol.rs` 里是：

```rust
Command::new(env!("CARGO_BIN_EXE_oak-mcp"))
```

把 `oak-mcp` 换成 `[[bin]].name` 的值。Cargo 会为每个二进制生成这个环境变量；改完跑 `cargo test` 确认没有 “environment variable not defined”。

### 3.3 改 instructions

`src/server.rs` 的 `INSTRUCTIONS` 是给模型的“工作室使用说明”。建议写：

- 你们的素材命名/目录约定；
- 默认分辨率、帧率、输出目录；
- 哪些工具必须按顺序调用；
- 不允许模型做的事（例如不要删除原始素材）。

示例：

```text
You edit video for Studio X. Defaults: 1920x1080, 25 fps, H.264 MP4.
Always import from /mnt/rushes and export to /mnt/delivery.
Before export, call timeline_get and preview_frame at the first and last clip.
Never delete media from the bin; ask the user instead.
```

---

## 4. 第一个工具：`studio_info`（不碰引擎，20 行）

目的：走通“新建文件 → 注册 → 编译 → 调用”。

### 4.1 新建 `src/tools/studio.rs`

```rust
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply};
use crate::state::ServerState;

pub fn tools() -> Vec<ToolDef> {
	vec![ToolDef {
		name: "studio_info",
		title: "Studio information",
		description: "Return this MCP server's studio name and version.",
		read_only: true,
		destructive: false,
		input_schema: || {
			json!({
				"type": "object",
				"properties": {
					"studio": {
						"type": "string",
						"description": "Studio name (optional; defaults to My Studio)"
					}
				}
			})
		},
		handler: studio_info,
	}]
}

fn studio_info(_state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let studio = args
		.opt_str("studio")
		.unwrap_or_else(|| "My Studio".to_string());
	Ok(ToolReply::json(json!({
		"studio": studio,
		"server": "my-studio-mcp",
		"engine": "quercus-engine-rs",
		"version": env!("CARGO_PKG_VERSION"),
	})))
}
```

注意 `opt_str` 返回 `Option<String>`，不是 `Result`；必填参数才用 `args.str(...)`（返回 `Result`）。

### 4.2 注册

`src/tools/mod.rs`：

```rust
pub mod studio;
// ...
tools.extend(history::tools());
tools.extend(studio::tools());
```

### 4.3 编译并调用

```bash
./scripts/build.sh build
./target/debug/oak-mcp --check
```

在 Inspector 里刷新 `tools/list`，应该多出 `studio_info`；调用：

```json
{"studio": "光影工作室"}
```

返回：

```json
{"studio":"光影工作室","server":"my-studio-mcp","engine":"quercus-engine-rs","version":"0.1.0"}
```

### 4.4 `ToolDef` 逐字段

| 字段 | 含义 |
|---|---|
| `name` | 工具名，模型看到的名字；保持 snake_case，全服务器唯一 |
| `title` | 人类可读标题 |
| `description` | **模型决定是否调用它的主要依据**；写清“什么时候用/什么时候不用” |
| `read_only` | 只读工具；会出现在客户端“只读”提示里 |
| `destructive` | 可能删除/覆盖用户数据；批量删除类工具必须设为 true |
| `input_schema` | 惰性 JSON Schema；用 `json!({...})` 描述参数 |
| `handler` | 普通 Rust 函数，签名固定为 `fn(&mut ServerState, Value) -> Result<ToolReply, String>` |

`ToolReply` 三种返回：

- `ToolReply::json(value)`：结构化结果 + 文本；
- `ToolReply::image(png, metadata)`：PNG + JSON（预览帧用）；
- `ToolReply::error(message)`：工具执行失败，客户端会显示 `isError: true`，模型能读到并重试。

参数解析用 `Args`：`str` / `opt_str` / `f64` / `opt_f64` / `bool` / `opt_bool` / `opt_usize` / `opt_i64` / `id` / `opt_id` / `ids`。

---

## 5. 第二个工具：`import_folder`（真实引擎，约 80 行）

需求：工作室把相机卡拷成一个文件夹，希望一条命令全部导入工程箱。

### 5.1 完整代码

`src/tools/studio.rs` 末尾追加（或替换成这个文件）：

```rust
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use quercus_engine_rs::edit::EditCommand;
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, resolve_session};
use crate::convert;
use crate::state::ServerState;

pub fn tools() -> Vec<ToolDef> {
	vec![ToolDef {
		name: "import_folder",
		title: "Import a folder of media",
		description: "Import every video/audio file in a folder into the project bin. \
			Useful for rushes copied off a camera card.",
		read_only: false,
		destructive: false,
		input_schema: || {
			json!({
				"type": "object",
				"properties": {
					"session": {"type": "string", "description": "Session id (defaults to active)"},
					"path": {"type": "string", "description": "Folder to scan"},
					"recursive": {"type": "boolean", "description": "Also scan subfolders (default false)"}
				},
				"required": ["path"]
			})
		},
		handler: import_folder,
	}]
}

fn import_folder(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let folder = args.str("path")?;
	let recursive = args.opt_bool("recursive")?.unwrap_or(false);
	let folder = std::fs::canonicalize(&folder)
		.map_err(|error| format!("cannot open folder `{folder}`: {error}"))?;
	let mut files = Vec::new();
	collect_media(&folder, recursive, &mut files)?;
	files.sort();
	if files.is_empty() {
		return Err(format!("no media files found in `{}`", folder.display()));
	}

	let entry = state.entry(&session_id)?;
	let mut imported = Vec::new();
	for file in &files {
		let result = entry
			.session
			.execute(EditCommand::ImportFootage { path: file.clone() })
			.map_err(|error| format!("import `{file}` failed: {error}"))?;
		if let Some(footage) = result.primary {
			imported.push(footage);
		}
	}
	let snapshot = entry.session.snapshot();
	let footages: Vec<Value> = imported
		.iter()
		.filter_map(|id| snapshot.footages.iter().find(|footage| footage.id == *id))
		.map(convert::footage_json)
		.collect();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"count": footages.len(),
		"footages": footages,
	})))
}

fn collect_media(dir: &Path, recursive: bool, out: &mut Vec<String>) -> Result<(), String> {
	let entries =
		std::fs::read_dir(dir).map_err(|error| format!("cannot read `{}`: {error}", dir.display()))?;
	for entry in entries {
		let entry = entry.map_err(|error| error.to_string())?;
		let path = entry.path();
		if path.is_dir() {
			if recursive {
				collect_media(&path, true, out)?;
			}
		} else if is_media(&path) {
			let path = path.canonicalize().map_err(|error| error.to_string())?;
			out.push(path.to_string_lossy().into_owned());
		}
	}
	Ok(())
}

fn is_media(path: &Path) -> bool {
	let extension = path
		.extension()
		.and_then(|value| value.to_str())
		.map(str::to_ascii_lowercase)
		.unwrap_or_default();
	matches!(
		extension.as_str(),
		"mp4" | "mov" | "mkv" | "mxf" | "avi" | "m4v" | "webm" | "wav" | "mp3" | "flac" | "aif"
			| "aiff"
	)
}
```

别忘了在 `src/tools/mod.rs` 注册 `studio`（同 4.2）。

### 5.2 这段代码在做什么

1. `resolve_session`：取 `session` 参数，没有就用当前活动 session；返回可读错误。
2. `canonicalize`：把 `~`、相对路径、符号链接变成绝对路径；引擎需要真实路径。
3. `collect_media`：只收常见视频/音频扩展名；`recursive` 控制是否进子目录。
4. `EditCommand::ImportFootage`：引擎的导入命令；每次导入是一个 undo 步骤。
5. `snapshot().footages`：从快照里取导入后的素材元数据，转成 JSON 返回。
6. 出错时返回 `Err(String)`，`tools::call` 会把它变成 `isError` 内容，模型能读到。

### 5.3 测试

准备一个文件夹（两个小视频即可）：

```bash
mkdir -p /tmp/rushes
cp /path/to/a.mp4 /tmp/rushes/
cp /path/to/b.mp4 /tmp/rushes/
```

Inspector 顺序：

1. `project_new` → 拿到 `session: "s1"`；
2. `import_folder {session: "s1", path: "/tmp/rushes"}`；
3. 期望 `{"count": 2, "footages": [...]}`，每个 footage 有 `id`、`duration`、`video`。

之后就可以用 `clip_place` 把这些 `id` 放到时间线。

### 5.4 练习：`assemble_rushes`

把“导入 + 顺序摆放”合成一个工作室工具：

- 先调用 `import_folder` 的逻辑（或复用 `tools::call`？不要——工具之间直接调 engine 更清晰）；
- 取当前 sequence，找到最后一个 clip 的 `out`；
- 对每个 footage 用 `EditCommand::PlaceFootageClip` 依次放到末尾；
- 返回总时长和 clip 数。

提示：`src/tools/clips.rs` 里的 `clip_place` 就是最小示例；时间用 `crate::convert::seconds_to_ts` 转帧。

---

## 6. 工作室工具的设计建议

不要给每个引擎命令都包一个 MCP 工具——example-mcp 已经有 50 个底层工具。你要加的是**工作室的工作流**：

| 好工具 | 为什么 |
|---|---|
| `import_folder` | 一条命令完成相机卡导入 |
| `assemble_rushes` | 导入 + 按文件名顺序排到时间线 |
| `apply_show_lut` | 对某条轨道/所有 clip 套统一 LUT |
| `export_review` | 720p 代理 + 水印/烧字 + 固定输出目录 |
| `make_social_cut` | 从主时间线裁 9:16、加字幕、导出 |
| `qc_report` | 检查黑帧/静音/时长/分辨率并返回报告 |

设计检查表：

- 名字是**动词_名词**，模型一看就知道什么时候用；
- 参数不超过 5 个，路径/ID 用字符串，时间用秒；
- 返回值包含 `id`、人可读摘要、下一步建议；
- 失败信息要能指导修复（“找不到文件夹 / 不支持的扩展名 / 没有活动序列”）；
- 只读工具设 `read_only: true`；会覆盖/删除的设 `destructive: true`；
- 长任务不要阻塞：返回 task id，让模型用 `task_wait`（参考 `src/tools/render.rs`）。

---

## 7. 改提示词和资源

### 7.1 加一个 prompt

`src/server.rs` 的 `prompts()` 里加：

```rust
Prompt::new(
	"studio_brief",
	Some("Turn a one-line client brief into a rough cut"),
	Some(vec![{
		let mut argument = PromptArgument::new("brief");
		argument.description = Some("What the client wants".into());
		argument.required = Some(true);
		argument
	}]),
)
```

`PromptArgument` 是 `#[non_exhaustive]`，不能直接写结构体字面量；用 `PromptArgument::new(...)` 再改字段。

然后在 `prompt_text()` 里加分支：

```rust
"studio_brief" => {
	let brief = arguments
		.get("brief")
		.and_then(Value::as_str)
		.ok_or_else(|| McpError::invalid_params("studio_brief requires `brief`", None))?;
	format!(
		"Create a rough cut for this brief: {brief}\n\
		 1. project_new 1920x1080 25fps\n\
		 2. import_folder from the rushes path\n\
		 3. assemble_rushes\n\
		 4. preview_frame at the first and last clip\n\
		 5. export_review"
	)
}
```

### 7.2 自定义资源（可选）

`src/server.rs` 的 `list_resources` / `resource_body` 可以暴露工作室配置、当前项目摘要、渲染队列等。最简单的做法是加一个 `quercus://studio/config` 资源，返回 JSON：

```rust
if uri == "quercus://studio/config" {
	return Ok(json!({
		"name": "Studio X",
		"defaults": {"width": 1920, "height": 1080, "fps": 25},
		"delivery": "/mnt/delivery",
	}));
}
```

资源适合“模型需要知道但不需要执行”的只读信息；需要执行/改状态的用工具。

---

## 8. 测试与调试

### 8.1 每次改完跑

```bash
./scripts/build.sh build
./scripts/build.sh test
python3 scripts/e2e_smoke.py
python3 scripts/e2e_edit.py
```

### 8.2 给新工具加协议测试

`tests/protocol.rs` 里已有 `Server` 测试脚手架（启动真实二进制、发 JSON-RPC、读响应）。仿照现有测试：

```rust
#[test]
fn studio_info_is_listed_and_callable() {
	let mut server = Server::start();
	server.request(
		"initialize",
		json!({
			"protocolVersion": "2025-06-18",
			"capabilities": {},
			"clientInfo": {"name": "test", "version": "0"}
		}),
	);
	let listed = server.request("tools/list", json!({}));
	assert!(
		listed["result"]["tools"]
			.as_array()
			.expect("tools")
			.iter()
			.any(|tool| tool["name"] == "studio_info")
	);
	let result = server.call("studio_info", json!({"studio": "Test"}));
	assert_eq!(result["structuredContent"]["studio"], "Test");
}
```

`Server` 是 `tests/protocol.rs` 里的测试脚手架，已有 `request` / `call` / `text_of`；复制一个现有测试改参数最快。

### 8.3 常见错误

| 现象 | 原因 | 处理 |
|---|---|---|
| 构建时 `quercus-ffmpeg-link` panic | 没设 `FFMPEG_DIR` | 先跑 `./scripts/ffmpeg/build-ffmpeg.sh`（Windows 用 `fetch-ffmpeg.ps1`），再 `./scripts/build.sh build` |
| `yaml-cpp` / `cstdint` 编译错 | ocio-sys bundled 与 GCC 不兼容 | 用系统 OCIO：`OCIO_INSTALL_DIR=/usr OCIO_RS_LINK=dynamic` |
| 运行时报 `libquercus_engine.so: cannot open` | dylib 不在 loader 路径 | 在 `target/debug` 下运行；打包时把 dylib 放在二进制旁 |
| 客户端连上就卡住/乱码 | 有代码往 stdout 打印 | 所有日志用 `eprintln!`；stdout 只给 rmcp |
| `invalid node id` | 自己编了 ID | ID 一律用工具返回的字符串，不要手写 |
| 导出 0 帧/时长不对 | 时间基用反了 | 用 `scripts`/现有工具；自定义导出代码看 `src/tools/render.rs` |
| 导出没声音/关键帧不动 | 旧引擎的已知问题 | 升级引擎；或先 `render_audio` 出 WAV 再放回时间线 |
| `task_wait` 超时 | 导出比预期长 | 先 `task_status` 看进度；`task_wait` 最长 600 秒 |
| `cargo test` 找不到二进制 | 改名后没改 `CARGO_BIN_EXE_*` | 见 3.2 检查清单 |

### 8.4 调试

- `oak-mcp -v` 会把生命周期日志打到 stderr；
- 工具里临时 `eprintln!` 不会破坏协议；
- 用 `--check` 快速确认引擎/渲染是否正常；
- 先用手工 JSON 调通，再写进测试。

---

## 9. 打包与分发

### 9.1 Release 构建

```bash
./scripts/build.sh build --release
./target/release/my-studio-mcp --check
```

### 9.2 二进制 + dylib

引擎是共享库。`build.rs` 给二进制加了 `$ORIGIN`/`@loader_path` rpath，SDK 会把 `libquercus_engine.so`（macOS 为 `.dylib`）放到 `target/release/` 旁边，所以**整个目录一起分发**：

```
my-studio-mcp/
├── my-studio-mcp
└── libquercus_engine.so
```

不要只拷二进制。用 `cargo install` 时也要把 dylib 放到 loader 路径或可执行文件旁。

### 9.3 客户端配置

```json
{
  "mcpServers": {
    "my-studio": {
      "command": "/opt/my-studio-mcp/my-studio-mcp",
      "args": []
    }
  }
}
```

### 9.4 发布

- GitHub Releases 上传 `tar.gz`（二进制 + dylib + LICENSE + README）；
- 写清版本、支持的平台、引擎版本（`quercus-engine-rs 0.5.7`）；
- 加 sha256；
- 如果做安装器，检查 `--check` 是否通过。

### 9.5 许可证

example-mcp 和本教程代码都是 **GPL-3.0-or-later**。这意味着：

- 工作室**内部使用**通常没问题；
- 如果你**对外分发**修改后的 MCP（二进制或源码），需要按 GPL 提供对应源码和许可证；
- 保留 `LICENSE` 和源码里的 SPDX 头；
- 具体合规问题请咨询法务，本文不构成法律意见。

---

## 10. Vibe Coding 工作流（给非计算机专业）

### 10.1 在仓库根放一个 `AGENTS.md`

```markdown
# AGENTS.md

## Build & test
- Build: `./scripts/build.sh build`
- Test: `./scripts/build.sh test`
- Smoke: `python3 scripts/e2e_smoke.py`

## Rules
- MCP protocol is handled by rmcp. Never add stdin/stdout or JSON-RPC code.
- Tools live in `src/tools/<domain>.rs`; use `ToolDef`, `Args`, `ToolReply`.
- Read `src/tools/media.rs` before writing a new tool.
- Node ids are strings from tool results; never invent them.
- Times are seconds. Use existing helpers in `src/tools/clips.rs`.
- Logs go to stderr (`eprintln!`); stdout is protocol only.
- Run `cargo fmt`, build, and tests before saying done.
- Do not change `Cargo.toml` dependencies without asking.
```

### 10.2 有效的提示词

- “Read `src/tools/media.rs` and `src/tools/mod.rs`. Add a read-only tool `list_rushes` that scans a folder and returns filenames, without importing. Follow the existing ToolDef style.”
- “The build fails with <error>. Fix only the new tool; don't touch `src/server.rs`.”
- “Add a protocol test for `studio_info` next to the existing tests; run `cargo test`.”
- “Review my new tool for: stdout prints, invented node ids, missing error messages, wrong read_only/destructive flags.”

### 10.3 护栏

- **小步**：一次只加一个工具，编译 + 测试通过再加下一个；
- **先读后写**：让模型先读一个同类工具，避免发明不存在的 API；
- **测试兜底**：每个工具至少一个手动调用；关键工具加协议测试；
- **不要让它改协议层**：`src/server.rs` 只允许改品牌/提示词；
- **看 diff**：确认没有把 `FFMPEG_DIR`、密钥、绝对路径写进仓库。

---

## 附录 A：新增工具检查清单

- [ ] 文件名/工具名是 snake_case，全服务器唯一
- [ ] `description` 写清何时用、何时不用
- [ ] `input_schema` 有 `type: object` 和 `required`
- [ ] 用 `Args` 解析，不手写 `arguments["x"].as_str()`
- [ ] 错误信息包含“哪一步、哪个路径、怎么修”
- [ ] 只读/破坏性标记正确
- [ ] 返回结构化 JSON，ID 用字符串
- [ ] 没有 `println!`（只用 `eprintln!`）
- [ ] `cargo fmt` + build + test 通过
- [ ] 在 Inspector 里真实调用过

## 附录 B：引擎侧容易踩的坑

- `SequenceSnapshot::length` 是缓存值，导出长度要用 clip 的 `out` 推导（example-mcp 已处理）。
- 导出参数要的是**帧时长**（1/fps），不是 fps。
- 一个进程里长时间 `render_audio` 后再导出可能挂起；先渲染 WAV，再在新进程导出。
- 导出音频/关键帧/静音轨在旧版引擎有已知缺陷；先升级，或按 example-mcp 的 workaround 做。
- undo 是进程级的，多 session 共享。
- 预览帧是工作色彩空间，显示前要 `utils::working_to_display_target`（example-mcp 已处理）。

## 附录 C：相关文件

- `src/tools/mod.rs`：`ToolDef`、`Args`、`ToolReply` 定义
- `src/tools/media.rs`：最小工具模板
- `src/tools/render.rs`：异步任务/导出模板
- `tests/protocol.rs`：真实 stdio 测试脚手架
- `README.md`：构建、工具列表、已知限制
- `docs/quercus-capabilities.md`：引擎能力总表

---

**下一步**：把 `studio_info` 和 `import_folder` 跑通，然后写下你们工作室最痛的那一步（通常是“导入 + 套 LUT + 出审片”），把它做成一个工具。工具不在多，在于让模型一次调用完成一个真实工作流。
