#!/usr/bin/env python3
"""Control for issue #4: a 6s version of the same flow (render_audio then
export). Expected: succeeds in a few seconds."""
import os
from common import MEDIA, Client, open_project, poll

OUT = MEDIA / "repro_control_short.mp4"
TIMEOUT = int(os.environ.get("REPRO_EXPORT_TIMEOUT", "120"))

c = open_project()
try:
    for i, name in enumerate(("C0500", "C0501")):
        footage = c.call("media_import", {"path": str(MEDIA / f"{name}.MP4")})["footage"]
        c.call("clip_place_linked", {"footage": footage["id"], "start": i * 3, "end": i * 3 + 3,
                                     "media_in": 0.4})
    c.call("render_audio", {"start": 0, "end": 6, "filename": str(MEDIA / "repro_short_mix.wav")})
    export = c.call("export_start", {"filename": str(OUT), "format": "mp4"})
    state, elapsed = poll(c, export["task"], TIMEOUT)
    print(f"export state after {elapsed:.1f}s:", state["state"], state.get("error"))
finally:
    c.close()
