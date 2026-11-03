#!/usr/bin/env python3
"""End-to-end smoke test for oak-mcp.

Drives the server over its real stdio transport: initialize, import media,
place a clip, read the timeline, render a preview frame, export, and verify the
exported file with ffprobe.

Usage:
    python3 scripts/e2e_smoke.py [--bin target/debug/oak-mcp] [--media PATH]
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_MEDIA = Path(os.environ.get(
    "OAK_TEST_MEDIA", REPO.parent / "quercus-engine" / "testdata" / "demo.mp4"
))


class Client:
    def __init__(self, binary: Path) -> None:
        self.process = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.next_id = 0

    def _send(self, message: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def notify(self, method: str, params: dict | None = None) -> None:
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def request(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        request_id = self.next_id
        self._send(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}}
        )
        assert self.process.stdout is not None
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"server exited while handling {method}:\n{stderr}")
        response = json.loads(line)
        assert response.get("id") == request_id, response
        return response

    def call(self, name: str, arguments: dict) -> tuple[dict, dict]:
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        if "error" in response:
            raise RuntimeError(f"{name} failed: {response['error']}")
        result = response["result"]
        text = next(
            (block["text"] for block in result["content"] if block["type"] == "text"), "{}"
        )
        if result.get("isError"):
            raise RuntimeError(f"{name} returned an error: {text}")
        return json.loads(text), result

    def close(self) -> None:
        if self.process.stdin:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()


def ffprobe(path: Path) -> dict:
    output = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_name,width,height",
            "-of",
            "json",
            str(path),
        ]
    )
    return json.loads(output)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", default=str(REPO / "target/debug/oak-mcp"))
    parser.add_argument("--media", default=str(DEFAULT_MEDIA))
    parser.add_argument("--keep", action="store_true", help="keep exported files")
    args = parser.parse_args()

    binary = Path(args.bin)
    media = Path(args.media)
    if not binary.exists():
        print(f"binary not found: {binary} (run cargo build first)", file=sys.stderr)
        return 2
    if not media.exists():
        print(f"media not found: {media}", file=sys.stderr)
        return 2

    workdir = REPO / "target" / "e2e"
    workdir.mkdir(parents=True, exist_ok=True)
    export_path = workdir / "smoke_export.mp4"
    frame_path = workdir / "preview.png"

    client = Client(binary)
    checks = 0
    try:
        init = client.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "e2e", "version": "1"},
            },
        )
        assert "result" in init, init
        server = init["result"]["serverInfo"]
        print(f"initialized: {server['name']} {server['version']}")
        client.notify("notifications/initialized")

        tools = client.request("tools/list")["result"]["tools"]
        print(f"tools: {len(tools)}")
        assert len(tools) > 30, tools

        project, _ = client.call(
            "project_new",
            {"name": "Smoke", "width": 1280, "height": 720, "fps": 25},
        )
        session = project["session"]
        print(f"project: session={session} sequence={project['sequence']}")
        checks += 1

        imported, _ = client.call("media_import", {"session": session, "path": str(media)})
        footage = imported["footage"]
        print(
            f"imported: id={footage['id']} duration={footage['duration']['seconds']}s "
            f"video={footage['video_streams']} audio={footage['audio_streams']}"
        )
        assert footage["video_streams"] >= 1
        checks += 1

        placed, _ = client.call(
            "clip_place",
            {
                "session": session,
                "footage": footage["id"],
                "kind": "video",
                "track_index": 0,
                "start": 0,
                "end": 5,
            },
        )
        print(f"placed clip: {placed['clip']}")
        checks += 1

        timeline, _ = client.call("timeline_get", {"session": session})
        sequence = timeline["sequences"][0]
        clips = sequence["tracks"][0]["clips"]
        assert len(clips) == 1, timeline
        print(
            f"timeline: {sequence['name']} {sequence['width']}x{sequence['height']} "
            f"fps={sequence['frame_rate']['seconds']} clips={len(clips)}"
        )
        checks += 1

        preview, preview_result = client.call(
            "preview_frame", {"session": session, "time": 1.0, "width": 640, "height": 360}
        )
        image = next(
            block for block in preview_result["content"] if block["type"] == "image"
        )
        frame_path.write_bytes(base64.b64decode(image["data"]))
        print(f"preview: {preview['width']}x{preview['height']} -> {frame_path}")
        checks += 1

        export, _ = client.call(
            "export_start",
            {"session": session, "filename": str(export_path), "format": "mp4"},
        )
        print(f"export started: task={export['task']}")
        done, _ = client.call(
            "task_wait", {"task": export["task"], "timeout_ms": 180000}
        )
        print(f"export finished: {done['state']} ({done['elapsed_seconds']}s)")
        if done["state"] != "succeeded":
            raise RuntimeError(f"export failed: {done.get('error')}")
        checks += 1

        probe = ffprobe(export_path)
        video = next(item for item in probe["streams"] if item.get("width"))
        duration = float(probe["format"]["duration"])
        print(
            f"export verified: {export_path.name} {video['codec_name']} "
            f"{video['width']}x{video['height']} {duration:.2f}s"
        )
        assert video["width"] == 1280 and video["height"] == 720, probe
        assert 4.5 <= duration <= 5.5, probe
        checks += 1

        undo, _ = client.call("undo", {})
        print(f"undo: {undo['undone']} can_undo={undo['can_undo']}")
        checks += 1

        print(f"\nOK: {checks} checks passed (workdir {workdir})")
        return 0
    finally:
        if not args.keep:
            for path in (export_path, frame_path):
                path.unlink(missing_ok=True)
        client.close()


if __name__ == "__main__":
    sys.exit(main())
