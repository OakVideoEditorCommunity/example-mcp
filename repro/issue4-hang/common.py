"""Shared MCP driver + timeline builder for the issue #4 repro."""
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
MEDIA = Path(os.environ.get("OAK_REPRO_MEDIA", HERE / "media"))
BIN = Path(os.environ.get("OAK_MCP_BIN", REPO / "target/debug/oak-mcp"))

# Narrative order used by the real cut.
ORDER = ["C0500", "C0501", "C0505", "C0506", "C0507", "C0509",
         "C0510", "C0503", "C0508", "C0512", "C0511", "C0502"]


class Client:
    def __init__(self):
        self.p = subprocess.Popen(
            [str(BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        self.i = 0

    @property
    def pid(self):
        return self.p.pid

    def req(self, method, params=None):
        self.i += 1
        self.p.stdin.write(json.dumps(
            {"jsonrpc": "2.0", "id": self.i, "method": method, "params": params or {}}
        ) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError("server exited: " + self.p.stderr.read())
        return json.loads(line)

    def call(self, name, args):
        parsed, _ = self.call_raw(name, args)
        return parsed

    def call_raw(self, name, args):
        result = self.req("tools/call", {"name": name, "arguments": args})["result"]
        text = [b["text"] for b in result["content"] if b["type"] == "text"][0]
        if result.get("isError"):
            raise RuntimeError(f"{name}: {text}")
        return json.loads(text), result

    def close(self):
        try:
            self.p.stdin.close()
        except Exception:
            pass
        try:
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def open_project():
    c = Client()
    c.req("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                         "clientInfo": {"name": "issue4-repro", "version": "1"}})
    print("server pid:", c.pid)
    c.call("project_new", {"name": "issue4", "width": 1280, "height": 720, "fps": 30})
    return c


def build_timeline(c, media=MEDIA):
    """12 linked clips, 11 dissolves, an opacity effect with keyframes."""
    clips = []
    t = 0.0
    for i, name in enumerate(ORDER):
        footage = c.call("media_import", {"path": str(media / f"{name}.MP4")})["footage"]
        seg = min(3.0, footage["duration"]["seconds"] - 1.0)
        media_in = 0.4 + (i % 3) * 0.3
        placed = c.call("clip_place_linked", {
            "footage": footage["id"], "start": t, "end": t + seg, "media_in": media_in,
        })
        clips.append(placed["clips"][0])
        t += seg
    for i in range(len(clips) - 1):
        c.call("transition_add", {"previous": clips[i], "next": clips[i + 1], "half": 0.4})
    effect = c.call("effect_add", {"host": clips[0],
                                   "type_id": "org.olivevideoeditor.Olive.opacity"})["effect"]
    c.call("effect_set_param", {"node": effect, "input_id": "opacity_in", "value": 0.0, "time": 0.0})
    c.call("effect_set_param", {"node": effect, "input_id": "opacity_in", "value": 1.0, "time": 0.8})
    length = c.call("timeline_get", {})["sequences"][0]["content_length"]["seconds"]
    print(f"timeline: {len(clips)} clips, {length:.3f}s")
    return clips, length


def poll(c, task, timeout):
    import time
    start = time.time()
    while time.time() - start < timeout:
        state = c.call("task_status", {"task": task})
        if state["state"] != "running":
            return state, time.time() - start
        time.sleep(3)
    return {"state": "running", "timeout": True}, time.time() - start
