#!/usr/bin/env python3
"""Issue #4: export hangs after a long render_audio in the same process.

Expected: the export task stays `running` forever (CPU drops to idle, output
file is truncated / has no moov atom). The same timeline exported in a fresh
process (or without render_audio) succeeds, see control_*.py.

Env:
  OAK_REPRO_MEDIA        media dir (default ./media -> ../../media/shuili)
  OAK_MCP_BIN            oak-mcp binary (default ../../target/debug/oak-mcp)
  REPRO_EXPORT_TIMEOUT   seconds to wait for the export (default 600)
  REPRO_HOLD_ON_HANG     "1" keeps the hung server alive for gdb (default 1)
"""
import base64
import os
import time

from common import MEDIA, build_timeline, open_project, poll

OUT = MEDIA / "repro_issue4.mp4"
MIX = MEDIA / "repro_issue4_mix.wav"
TIMEOUT = int(os.environ.get("REPRO_EXPORT_TIMEOUT", "600"))
GAIN_DB = float(os.environ.get("REPRO_GAIN_DB", "18"))
HOLD = os.environ.get("REPRO_HOLD_ON_HANG", "1") != "0"

c = open_project()
try:
    clips, length = build_timeline(c)

    # 1) Correct audio path: preview_audio picks the real audio streams.
    mix = c.call("render_audio", {
        "start": 0, "end": length, "gain_db": GAIN_DB, "filename": str(MIX),
    })
    print(f"render_audio: peak {mix['peak_db']} dB, {mix['frames']} frames -> {MIX}")

    # 2) Lay the mix back on A2 as a WAV (the export path can decode WAV).
    wav = c.call("media_import", {"path": str(MIX)})["footage"]
    c.call("clip_place", {"footage": wav["id"], "kind": "audio",
                          "track_index": 1, "start": 0, "end": length})

    # 3) Preview frames (also part of the original failing session).
    for at in (1.0, 6.0, 12.0, 20.0, 28.0):
        _, result = c.call_raw("preview_frame", {"time": at, "width": 640, "height": 360})
        image = next(b for b in result["content"] if b["type"] == "image")
        (MEDIA / f"repro_preview_{at:.0f}.png").write_bytes(base64.b64decode(image["data"]))

    # 4) Export: this is the step that hangs.
    export = c.call("export_start", {"filename": str(OUT), "format": "mp4"})
    print("export started:", export["task"], "->", OUT)
    state, elapsed = poll(c, export["task"], TIMEOUT)
    print(f"export state after {elapsed:.1f}s:", state["state"])
    if state["state"] == "running":
        size = OUT.stat().st_size if OUT.exists() else 0
        print(f"\nHANG REPRODUCED: task still running after {elapsed:.0f}s, "
              f"file size {size} bytes (moov missing)")
        print(f"server pid {c.pid}")
        print("attach with:")
        print("  sudo sysctl -w kernel.yama.ptrace_scope=0")
        print(f"  gdb -p {c.pid} -batch -ex 'thread apply all bt'")
        if HOLD:
            print("holding the process for 300s ...")
            time.sleep(300)
    else:
        print("NOT REPRODUCED: export finished:", state.get("error"))
finally:
    c.close()
