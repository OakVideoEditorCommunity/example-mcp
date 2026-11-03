# oak-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes the
[Quercus](https://github.com/OakVideoEditorCommunity/quercus-engine) nonlinear
video-editing engine to LLM agents. It depends on the published
`quercus-engine-rs` crate from crates.io and speaks MCP over stdio.

An agent can create a project, import footage, place/trim/split clips, build
effect chains and keyframes, render preview frames (returned as PNG images),
export deliverables asynchronously, and round-trip OpenTimelineIO — all through
MCP tool calls.

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

## Requirements

- Rust 1.85+ (edition 2024).
- The **project FFmpeg 8.1 build** that Quercus pins: a static install prefix
  with `lib/pkgconfig`. A distro/system FFmpeg is not supported. Build it once
  in a checkout of `quercus-engine` with
  `tooling/install-deps.sh` + `tooling/ffmpeg/build-ffmpeg.sh` and point
  `FFMPEG_DIR` at `.cache/ffmpeg`.
- **OpenColorIO 2.x** development files (`libOpenColorIO.so` + headers).
  `quercus-core 0.5.2` and `quercus-render 0.5.2` enable ocio-rs 0.2's
  `bundled` feature, whose vendored yaml-cpp does not compile with GCC >= 13
  (missing `#include <cstdint>`). This repository therefore links the system
  OCIO: `.cargo/config.toml` sets `OCIO_INSTALL_DIR=/usr` and
  `OCIO_RS_LINK=dynamic` (Linux defaults). Override them for other prefixes,
  e.g. `OCIO_INSTALL_DIR=/opt/homebrew/opt/open-colorio` on macOS.
  If you must build the bundled copy instead, provide the missing includes, e.g.
  `CXXFLAGS="-include cstdint"`, and unset `OCIO_INSTALL_DIR`.

`FFMPEG_DIR` is not checked in (Cargo's `[env]` cannot expand shell variables);
export it before building, or use `scripts/build.sh`.

## Build

```sh
# Quercus' pinned static FFmpeg (see above).
export FFMPEG_DIR=/path/to/quercus-engine/.cache/ffmpeg

# Linux defaults for the system-OCIO workaround (override on other prefixes).
export OCIO_INSTALL_DIR=/usr
export OCIO_RS_LINK=dynamic

cargo build            # debug, as used by the tests below
cargo build --release  # production binary
./target/debug/oak-mcp --check   # engine loads, sequence + render manager work
```

`scripts/build.sh` wraps those exports: `./scripts/build.sh build --release`.

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
| `src/main.rs` | CLI, stdio loop, `--check` |
| `src/rpc.rs` | JSON-RPC 2.0 model + line framing |
| `src/server.rs` | MCP lifecycle, tools/resources/prompts, logging |
| `src/state.rs` | Sessions, async task registry, render init |
| `src/tools/` | Tool definitions and handlers, grouped by domain |
| `src/convert.rs` | DTO → JSON, seconds ↔ frame timestamps |
| `src/image.rs` | F32 RGBA → PNG |
| `src/sidecar.rs` | Format/marker persistence the `.ove` serializer drops |

The process is single-threaded: requests are handled in order, engine tasks run
on Quercus' own workers, and `task_wait` blocks only the calling request.

## Testing

```sh
cargo test                                   # unit + protocol integration tests
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
