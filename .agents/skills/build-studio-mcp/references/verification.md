# Verification

## Baseline

```sh
./scripts/build.sh test
./target/debug/<bin> --check
```

`--check` must show the engine, a sequence, the render manager, and the tool
count. Do not proceed on a failed check.

## Drive the real server

Use a short real clip; if none exists, generate one:

```sh
ffmpeg -f lavfi -i testsrc=size=1280x720:rate=25 -t 3 -pix_fmt yuv420p /tmp/studio-test.mp4
```

Minimal stdio driver (save as `/tmp/mcp_smoke.py`):

```python
import json, subprocess

p = subprocess.Popen(["./target/debug/<bin>"], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, text=True, bufsize=1)

def send(obj):
    p.stdin.write(json.dumps(obj) + "\n")
    p.stdin.flush()

def recv():
    return json.loads(p.stdout.readline())

send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {},
    "clientInfo": {"name": "smoke", "version": "0"}}})
recv()
send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
      "params": {"name": "project_new", "arguments": {"name": "smoke", "width": 1280, "height": 720, "fps": 25}}})
project = recv()
session = project["result"]["structuredContent"]["session"]

send({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
      "params": {"name": "import_folder", "arguments": {"session": session, "path": "/tmp/rushes"}}})
print(json.dumps(recv()["result"]["structuredContent"], ensure_ascii=False))
p.stdin.close()
p.wait()
```

Check:

- `tools/list` contains every new tool name.
- Each new tool returns `structuredContent` without `isError`.
- `preview_frame` returns an image content block.
- `export_start` → `task_wait` → `ffprobe` shows the expected codec, size,
  duration, and audio.
- A bad input (missing folder, wrong ID) returns `isError: true` with an
  actionable message.

## Protocol test

Copy the `Server` harness in `tests/protocol.rs` and add at least one test for
the main new tool: initialize, `tools/list`, call the tool, assert a field.
Run `./scripts/build.sh test`.
