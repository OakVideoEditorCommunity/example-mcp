---
name: build-studio-mcp
description: Use when a film, TV, or video studio wants its own MCP server for video editing built from example-mcp/Quercus, or wants to add studio-specific workflow tools, prompts, branding, packaging, or client setup. The studio user is non-technical; the agent owns environment setup, code, builds, tests, and delivery.
---

# Build a Studio MCP Server

## Outcome

A working, branded MCP server that the studio can connect to Claude, Cursor,
VS Code, or its own agent. It keeps example-mcp's 50 engine tools and adds 3–6
high-level tools for the studio's repetitive workflows, verified on real media,
packaged with the engine library, and handed over with a one-page usage guide.

## Operating rules

- **The user is an editor/producer, not an engineer.** Do not ask them to run
  commands, edit files, install Rust/FFmpeg, or understand MCP, JSON-RPC, or
  Rust. Do all of that yourself and report progress in plain language.
- **Ask only about the work**, not the implementation: brand, OS/client,
  3 repetitive workflows, media/delivery conventions, and things the server
  must never do. Use sensible defaults for everything else.
- **Ask for approval only** before sudo/package installs, network downloads,
  overwriting or deleting existing files, and publishing to GitHub.
- **Do not teach the protocol.** `rmcp` handles MCP; the studio's value is in
  the workflow tools. Do not hand the user a tutorial — hand them a working
  server and a short usage note.
- **Preserve source media.** Work on copies, never delete camera originals, and
  keep media/secrets out of git. The server must expose destructive actions
  honestly (`destructive: true`).
- **License:** the result is GPL-3.0-or-later. Keep `LICENSE` and SPDX headers;
  mention the obligation only if the studio plans to distribute the server.

## Workflow

### 1. Intake (at most five questions)

Ask in the user's language, conversationally. Do not ask all at once if the
answers are already in the request.

1. Studio/brand name and where deliverables should go.
2. Operating system and MCP client (Claude Desktop, Cursor, VS Code, custom).
3. The three most repetitive editing tasks (e.g. camera-card import, LUT,
   review export, social cut, QC).
4. Media locations and delivery specs (resolution, fps, codec, naming).
5. Anything the server must never do (delete originals, upload footage, ...).

Defaults when unanswered: 1920×1080, 25 or 30 fps, H.264 MP4, export next to
the project, no deletions, no network uploads.

### 2. Create the studio workspace

Detect whether the current directory is already example-mcp (`src/tools/media.rs`
and a git remote containing `example-mcp`).

- **If it is:** work in place on a branch, or clone a clean copy with
  `git clone . ../<studio-slug>-mcp` (tracked files only; no `target/`, `.cache/`,
  or media).
- **If it is not:** clone
  `https://github.com/OakVideoEditorCommunity/example-mcp.git <studio-slug>-mcp`
  into the directory the user chooses.

Then rename the server (ask for the slug only if the brand is ambiguous):

| Where | Change |
|---|---|
| `Cargo.toml` | `package.name`, `[[bin]].name`, description, repository |
| `src/server.rs` | `Implementation::new(...)`, title, `INSTRUCTIONS` |
| `src/main.rs` | `--help` / `--version` text |
| `tests/protocol.rs` | `env!("CARGO_BIN_EXE_<old>")` → new binary name |
| `scripts/e2e_smoke.py` | `--bin` default |
| `scripts/e2e_edit.py` | `BIN` constant |
| `README.md`, `README.zh-CN.md` | title and client examples |

Do not rename the package to `oak-mcp` or another taken name. Keep `scripts/`
with the copy; it contains the FFmpeg build/download scripts and e2e tests.

### 3. Set up the environment (agent-owned)

Check `FFMPEG_DIR` and `.cache/ffmpeg` first and reuse them.

- **Linux/macOS:**
  - `./scripts/install-deps.sh` (may need sudo; ask once).
  - `./scripts/ffmpeg/build-ffmpeg.sh` in a **background job**; it builds the
    pinned FFmpeg 8.1 into `.cache/ffmpeg` and can take tens of minutes.
    Continue scaffolding tools while it runs; do not busy-poll.
  - Then `./scripts/build.sh build` and `./target/debug/<bin> --check`.
  - If the build complains about OCIO/yaml-cpp, use the system OCIO:
    `OCIO_INSTALL_DIR=/usr OCIO_RS_LINK=dynamic`.
  - macOS: `export OCIO_INSTALL_DIR="$(brew --prefix opencolorio)"` before
    `./scripts/build.sh`.
- **Windows (PowerShell):**
  - `. .\scripts\fetch-ffmpeg.ps1` downloads the BtbN prebuilt package,
    verifies SHA-256, unpacks `.cache\ffmpeg`, and sets FFMPEG_DIR,
    PKG_CONFIG_PATH, OCIO, CC/CXX. Use the leading dot.
  - `cargo build --release`; `.\target\release\<bin>.exe --check`.
  - Requires MSVC Build Tools, Rust `stable-x86_64-pc-windows-msvc`, and Git
    for Windows. Do not send the user to install these manually; detect and
    explain what is missing only if it blocks.
- **Gate:** baseline `--check` must pass before adding tools. If the environment
  cannot be fixed, stop and report the exact blocker; do not fake tool tests.

### 4. Design the studio tools

Read `src/tools/mod.rs`, `media.rs`, `clips.rs`, and `render.rs` before writing.

- Add **3–6 workflow tools**, not wrappers around every engine command. One
  tool should complete one studio step in a single call.
- Good candidates: `import_folder`, `assemble_rushes`, `apply_show_lut`,
  `export_review`, `make_social_cut`, `qc_report`.
- Put them in `src/tools/studio.rs`, register with `pub mod studio;` and
  `tools.extend(studio::tools());` in `src/tools/mod.rs`.
- Use `ToolDef`, `Args`, `ToolReply`; return node IDs as strings, times in
  seconds, actionable errors; set `read_only`/`destructive` truthfully.
- Add or update a prompt and `INSTRUCTIONS` so the model knows the studio's
  defaults and tool order.
- See [recipes](references/recipes.md) for compiled, tested examples.

### 5. Verify on real media

- `./scripts/build.sh test` (Windows: `cargo test` with the env from the
  PowerShell script).
- `./target/debug/<bin> --check`.
- Start the server over stdio and call: `project_new`, each new tool,
  `preview_frame`, `export_start`, `task_wait`. Use a short real clip.
- Verify the export with `ffprobe` (duration, codec, resolution, audio).
- Add at least one protocol test in `tests/protocol.rs` for the main new tool.
- Check `tools/list` contains the new names; failures must be `isError` content,
  not protocol errors.
- Do not report success until these pass. See [verification](references/verification.md).

### 6. Package and hand off

- `./scripts/build.sh build --release` (Windows: `cargo build --release`).
- Ship the binary with the engine library (`libquercus_engine.so` / `.dylib` /
  `.dll`) and `LICENSE`; zip the folder.
- Write `STUDIO.md`: tool list, sample prompts, media/delivery paths, how to
  add the server to the client, known limits.
- Give the user the client config with an **absolute** path, and tell them to
  restart the client and try one sample prompt.
- Only create/push a GitHub repo if the user asks; the result is
  GPL-3.0-or-later.
- See [delivery](references/delivery.md).

## Failure handling

| Symptom | Action |
|---|---|
| `quercus-ffmpeg-link` panic / FFMPEG_DIR missing | Build/reuse `.cache/ffmpeg`; rerun `scripts/build.sh` |
| OCIO/yaml-cpp / `cstdint` build error | Use system OCIO (`OCIO_INSTALL_DIR=/usr`, `OCIO_RS_LINK=dynamic`) |
| `libquercus_engine.so` not found at runtime | Run from `target/debug` or keep the dylib next to the binary |
| Client hangs or shows garbage | Something prints to stdout; move logs to `eprintln!` |
| `invalid node id` | Use IDs returned by tools; never invent them |
| Export silent / keyframes static | Upstream engine defect; upgrade or use the documented WAV workaround |
| `task_wait` timeout | Poll `task_status`; exports can take minutes |
| New tool compiles but fails at call | Check `isError` text, sample media path, and session/sequence presence |

## Do not

- Add `stdin`/`stdout`/JSON-RPC code; `rmcp` owns the protocol.
- Print to stdout anywhere in the server.
- Invent engine APIs or node IDs; mirror `src/tools/*.rs`.
- Delete or overwrite source media without explicit approval.
- Commit `target/`, `.cache/`, `media/`, binaries, or credentials.
- Claim the server works without `--check`, a real tool call, and an export check.
