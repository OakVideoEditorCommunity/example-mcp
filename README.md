[English](README.md) | [简体中文](README.zh-CN.md)

# oak-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes the
[Quercus](https://github.com/OakVideoEditorCommunity/quercus-engine) nonlinear
video-editing engine to LLM agents. It depends on the published
`quercus-engine-rs` crate from crates.io and speaks MCP over stdio.

An agent can create a project, import footage, place/trim/split clips, build
effect chains and keyframes, render preview frames (returned as PNG images),
export deliverables asynchronously, and round-trip OpenTimelineIO — all through
MCP tool calls.

The MCP layer uses the official [`rmcp`](https://crates.io/crates/rmcp) SDK;
`src/tools/` contains only engine-facing tools, so adding a tool does not touch
protocol code.

## Tools

50 tools in seven groups (each has a JSON schema and read-only/destructive
annotations):

| Group | Tools |
|---|---|
| Project & session | `project_new`, `project_open`, `project_save`, `project_info`, `session_list`, `session_use`, `session_close` |
| Sequence & timeline | `sequence_create`, `sequence_use`, `sequence_set_format`, `timeline_get`, `playhead_set`, `track_add`, `track_set`, `marker_add`, `markers_list`, `workarea_set` |
| Media | `media_import`, `media_list` |
| Clips | `clip_place`, `clip_place_linked`, `clip_move`, `clip_trim`, `clip_split`, `clip_delete`, `clip_speed`, `clip_slide`, `clip_slip`, `clip_set`, `transition_add` |
| Effects | `effect_types`, `effect_add`, `effect_remove`, `effect_enable`, `effect_chain`, `effect_params`, `effect_set_param`, `effect_keyframe` |
| Render & export | `preview_frame`, `render_audio`, `export_start`, `task_status`, `task_wait`, `task_cancel`, `task_list`, `otio_export`, `otio_import` |
| History | `undo`, `redo`, `history` |

Resources expose `quercus://server/info`, `quercus://session/{id}/timeline`, and
`quercus://session/{id}/project`. Two prompts (`rough_cut`, `deliver_export`)
bundle the intended tool sequence.

### Conventions

- **Times are seconds** (`f64`). The server converts to the engine's rational
  timebase and per-sequence frame timestamps.
- **Node ids are opaque decimal strings.** Always take them from tool results;
  ids are generated per session and are not stable across save/reopen.
- Every session-scoped tool accepts `session`; omitted, it uses the active
  session.
- Tool failures are returned as `isError: true` content (not JSON-RPC errors),
  so the model can read the message and retry. Unknown tools and malformed
  requests are proper JSON-RPC errors (`-32602`, `-32601`, ...).
- Export is asynchronous: `export_start` returns a task id, then poll
  `task_status` or block on `task_wait`.

## Build it with DeepSeek Harness

This repository ships a DeepSeek Harness skill at
`.agents/skills/build-studio-mcp`. Open the repo in DSH and describe the
studio's workflow in plain language: the agent sets up FFmpeg, writes the
tools, builds, tests on real media, and hands back a packaged MCP server. It
asks only about brand, OS/client, repetitive workflows, and delivery defaults.

For a developer-oriented code walkthrough, see
[docs/tutorial-customize-example-mcp.md](docs/tutorial-customize-example-mcp.md).

## Requirements

- Rust 1.88+ (edition 2024; `rmcp` requires 1.88).
- **FFmpeg 8.1**, the project build Quercus pins (static prefix + `lib/pkgconfig`).
  A distro/system FFmpeg is not supported. The Quercus build scripts are
  vendored under `scripts/`, so you do **not** need a Quercus checkout:
  - **Linux/macOS**: run `./scripts/install-deps.sh` once (build tools + system
    libraries; may prompt for sudo/Homebrew), then
    `./scripts/ffmpeg/build-ffmpeg.sh`. The prefix is `.cache/ffmpeg`.
  - **Windows**: run `. .\scripts\fetch-ffmpeg.ps1` (note the leading dot).
    It downloads the same BtbN/FFmpeg-Builds archive that Quercus CI uses,
    verifies its SHA-256 checksum, and unpacks it into `.cache\ffmpeg`.
- **OpenColorIO 2.x** development files (`libOpenColorIO.so` + headers).
  `quercus-core 0.5.2` and `quercus-render 0.5.2` enable ocio-rs 0.2's
  `bundled` feature, whose vendored yaml-cpp does not compile with GCC >= 13
  (missing `#include <cstdint>`). This repository links the system OCIO:
  `scripts/build.sh` sets `OCIO_INSTALL_DIR=/usr` and `OCIO_RS_LINK=dynamic`
  on Linux; the Windows script sets the static/real path used by CI.
  On macOS export `OCIO_INSTALL_DIR="$(brew --prefix opencolorio)"`.
  If you must build the bundled copy instead, provide the missing includes, e.g.
  `CXXFLAGS="-include cstdint"`, and unset `OCIO_INSTALL_DIR`.

`FFMPEG_DIR` is not checked in. `scripts/build.sh` uses `.cache/ffmpeg`
automatically; on Windows the PowerShell script sets it for you.

## Build

### Linux / macOS

```sh
./scripts/install-deps.sh           # once; build tools + system libs (sudo/brew)
./scripts/ffmpeg/build-ffmpeg.sh    # once; pinned FFmpeg 8.1 -> .cache/ffmpeg
./scripts/build.sh build --release
./target/release/oak-mcp --check    # engine loads, sequence + render manager work
```

`scripts/build.sh` sets the OCIO environment, uses `.cache/ffmpeg`, and forwards
the cargo arguments (`build`, `test`, ...). Override `FFMPEG_DIR` or
`OCIO_INSTALL_DIR` in the environment if your prefix differs.

### Windows (PowerShell)

Requirements: MSVC C++ Build Tools, Rust
`stable-x86_64-pc-windows-msvc`, and Git for Windows (for `link.exe` and the
usual toolchain).

```powershell
. .\scripts\fetch-ffmpeg.ps1       # the leading dot keeps the env vars
cargo build --release
.\target\release\oak-mcp.exe --check
```

The script downloads
`ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip` from
[BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest),
verifies `checksums.sha256`, extracts it to `.cache\ffmpeg`, and sets
`FFMPEG_DIR`, `PKG_CONFIG_PATH`, `PATH`, `OCIO_RS_ENABLE_REAL=1`,
`OCIO_RS_LINK=static`, `CC=cl`, `CXX=cl`. This mirrors Quercus' Windows CI.

### Manual FFmpeg setup (without the scripts)

**Windows.** Download the archive and checksums yourself:

```powershell
$base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
$asset = "ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip"
Invoke-WebRequest "$base/checksums.sha256" -OutFile "$env:TEMP\checksums.sha256"
# Find the line for $asset in checksums.sha256 and note its SHA-256.
Invoke-WebRequest "$base/$asset" -OutFile "$env:TEMP\$asset"
(Get-FileHash "$env:TEMP\$asset" -Algorithm SHA256).Hash  # compare
Expand-Archive "$env:TEMP\$asset" -DestinationPath .cache\ffmpeg-extract -Force
# Move the single top-level folder out of ffmpeg-extract:
Move-Item .cache\ffmpeg-extract\ffmpeg-n8.1-latest-win64-gpl-shared-8.1 .cache\ffmpeg
$env:FFMPEG_DIR = "$PWD\.cache\ffmpeg"
$env:PKG_CONFIG_PATH = "$PWD\.cache\ffmpeg\lib\pkgconfig"
$env:PATH = "$PWD\.cache\ffmpeg\bin;$env:PATH"
$env:OCIO_RS_ENABLE_REAL = "1"
$env:OCIO_RS_LINK = "static"
$env:CC = "cl"
$env:CXX = "cl"
cargo build --release
```

If the top-level folder inside the archive has a different name, use that name
in `Move-Item`.

**Linux/macOS.** Install the build tools (`cmake`, `nasm`, `pkg-config`,
`freetype`, `fontconfig`, plus a C/C++ toolchain), then reproduce the pinned
build with the same steps as `scripts/ffmpeg/build-ffmpeg.sh`: build the codec
libraries into `.cache/ffmpeg`, configure FFmpeg 8.1 with
`--prefix=$PWD/.cache/ffmpeg --pkg-config-flags=--static`, and install it there.
Using the vendored scripts is strongly recommended because the codec set and
flags are pinned.

If `$CARGO_HOME` is not writable (sandboxed CI), use a project-local one:

```sh
CARGO_HOME=$PWD/.cargo-home cargo build --release
```

The binary links the engine shared library (`libquercus_engine.so`) at runtime.
`build.rs` adds an `$ORIGIN` rpath, and the SDK stages a copy next to the built
binary, so `target/{debug,release}/oak-mcp` runs in place. For `cargo install`,
ship the dylib next to the executable (or on the loader path).

## Use with an MCP client

Stdio transport, no arguments:

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

Typical agent session:

```
project_new {width: 1920, height: 1080, fps: 30}
media_import {path: "/media/a.mov"}
clip_place {footage: "10", start: 0, end: 5}
preview_frame {time: 1.0}          -> PNG image
export_start {filename: "/tmp/out.mp4"}
task_wait {task: "t1", timeout_ms: 120000}
```

## Source layout

| Path | Purpose |
|---|---|
| `src/main.rs` | CLI, `rmcp` stdio server bootstrap, `--check` |
| `src/server.rs` | `rmcp` `ServerHandler`: tools/resources/prompts mapping |
| `src/state.rs` | Sessions, async task registry, render init |
| `src/tools/` | Tool definitions and handlers, grouped by domain |
| `src/convert.rs` | DTO → JSON, seconds ↔ frame timestamps |
| `src/image.rs` | F32 RGBA → PNG |
| `src/sidecar.rs` | Format/marker persistence the `.ove` serializer drops |
| `scripts/ffmpeg/` | Vendored Quercus FFmpeg build scripts (Linux/macOS) |
| `scripts/fetch-ffmpeg.ps1` | Windows BtbN FFmpeg downloader (checksum-verified) |
| `scripts/install-deps.sh` | Build tools / system libraries (Linux/macOS) |
| `.agents/skills/build-studio-mcp/` | DeepSeek Harness skill that builds a studio MCP from a plain-language brief |

The process is single-threaded: requests are handled in order, engine tasks run
on Quercus' own workers, and `task_wait` blocks only the calling request.

## Adding a tool

Tools are plain declarations: add a `ToolDef` to the relevant `src/tools/*.rs`
module (name, description, JSON Schema, handler), append it to that module's
`tools()` list, and `rmcp` publishes it — no protocol code is involved.
Handlers parse arguments through `Args` and return
`ToolReply::{json,image,error}`.

## Testing

```sh
./scripts/build.sh test                      # unit + protocol integration tests
python3 scripts/e2e_smoke.py                 # new -> import -> place -> preview -> export
python3 scripts/e2e_edit.py                  # effects, keyframes, split/trim, save/reopen,
                                             # markers, range export, resources, prompts
```

Both scripts drive the real binary over stdio and verify the exported file with
`ffprobe`. They need a small test clip: point `OAK_TEST_MEDIA` at one (for
example `testdata/demo.mp4` from a Quercus checkout), or pass `--media`.

## Engine limitations handled here

- **Sequence format and markers are not serialized by the engine's `.ove`
  writer.** `project_save` additionally writes `<project>.oak-mcp.json`
  (format per sequence + current-sequence markers) and `project_open` re-applies
  it, reporting `sidecar_restored` and any warnings. Delete the sidecar to open
  the project with engine defaults.
- **`SequenceSnapshot::length` is a stale cached value** for SDK-only
  producers (the app refreshes it). Export lengths and the reported
  `content_length` are derived from the clips instead.
- **The export encoder wants a frame duration, not a frame rate**, so the
  server inverts the sequence rate for `EncodingParams`.
- **Undo is process-global** in the engine; `undo`/`redo` affect the most recent
  edit across sessions. This is upstream behaviour, not a server choice.
- Preview frames are clamped to 8-bit RGBA PNG; HDR/wide-gamut output is not
  represented.

## Known upstream engine defects

These were reproduced against the published crates (`quercus-engine-rs 0.5.7`,
`quercus-engine 0.5.3`, internal crates `0.5.2`); `oak-mcp` works around the
ones it can:

- **Export audio from video containers is silent** (`quercus-task 0.5.2`
  hardcodes `stream_index: 0` in its export audio montage; stream 0 of a video
  file is video). `render_audio` uses the correct preview-audio path. Workaround:
  render the timeline mix to WAV, then lay that WAV on a second audio track and
  export.
- **Keyframed effect parameters animate in preview but not in export**: the
  export montage serializes static parameter values only.
- **Muted audio tracks are still mixed by the export path.**
- **Audio-only exports (WAV) fail** with `Failed to write frame` because the
  export still renders and writes video frames.
- **A long `render_audio` followed by an export in the same process can hang**
  (a 33.7s timeline stayed `running` with the CPU idle). Render audio in a
  separate process, then export in a fresh one.
- **`.ove` save/reopen loses sequence format, markers, and work area**
  (`SequenceBehavior::save_custom` writes only track lists) — see the sidecar
  workaround above.
- **`SequenceSnapshot::length` is stale** for SDK producers; export lengths are
  derived from clips instead.

## License

GPL-3.0-or-later, matching `quercus-engine-rs` and the Quercus engine.
Customized servers that you distribute must remain GPL-3.0-or-later; the
source files carry SPDX headers.
