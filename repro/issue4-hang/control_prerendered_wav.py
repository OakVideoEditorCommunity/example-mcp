#!/usr/bin/env python3
"""Control for issue #4: same 33.77s timeline, pre-made WAV on A2, no
render_audio in this process. Expected: export succeeds (~150s)."""
import os
from common import MEDIA, build_timeline, open_project, poll

OUT = MEDIA / "repro_control_prerendered.mp4"
TIMEOUT = int(os.environ.get("REPRO_EXPORT_TIMEOUT", "600"))

c = open_project()
try:
    _, length = build_timeline(c)
    # Prefer the exact clipped mix produced by repro.py: exporting it in a
    # fresh process succeeds, which proves the hang is the in-process
    # render_audio state, not the WAV content.
    mix = MEDIA / "repro_issue4_mix.wav"
    source = mix if mix.exists() else MEDIA / "sine33.wav"
    print("control wav:", source)
    wav = c.call("media_import", {"path": str(source)})["footage"]
    c.call("clip_place", {"footage": wav["id"], "kind": "audio",
                          "track_index": 1, "start": 0,
                          "end": min(wav["duration"]["seconds"], length)})
    export = c.call("export_start", {"filename": str(OUT), "format": "mp4"})
    state, elapsed = poll(c, export["task"], TIMEOUT)
    print(f"export state after {elapsed:.1f}s:", state["state"], state.get("error"))
finally:
    c.close()
