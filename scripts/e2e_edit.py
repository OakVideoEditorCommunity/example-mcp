#!/usr/bin/env python3
"""Broader end-to-end test: effects, keyframes, markers, split/trim, save/reopen,
range export, resources and prompts.

Run after `cargo build`:
    python3 scripts/e2e_edit.py
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = REPO / "target/debug/oak-mcp"
MEDIA = Path(os.environ.get(
    "OAK_TEST_MEDIA", REPO.parent / "quercus-engine" / "testdata" / "demo.mp4"
))
WORK = REPO / "target" / "e2e-edit"


class Client:
    def __init__(self, binary: Path) -> None:
        self.process = subprocess.Popen(
            [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        self.next_id = 0

    def notify(self, method: str, params: dict | None = None) -> None:
        self.process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": method, "params": params or {}}) + "\n"
        )
        self.process.stdin.flush()

    def request(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        self.process.stdin.write(
            json.dumps(
                {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}
            )
            + "\n"
        )
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError(f"server exited during {method}: {self.process.stderr.read()}")
        response = json.loads(line)
        assert response.get("id") == self.next_id, response
        return response

    def call(self, name: str, arguments: dict) -> dict:
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        if "error" in response:
            raise RuntimeError(f"{name}: JSON-RPC error {response['error']}")
        result = response["result"]
        text = next(
            (block["text"] for block in result["content"] if block["type"] == "text"), "{}"
        )
        if result.get("isError"):
            raise RuntimeError(f"{name} failed: {text}")
        return json.loads(text)

    def close(self) -> None:
        self.process.stdin.close()
        self.process.wait(timeout=10)


def main() -> int:
    WORK.mkdir(parents=True, exist_ok=True)
    saved = WORK / "roundtrip.ove"
    ranged = WORK / "range.mp4"
    client = Client(BIN)
    try:
        client.request(
            "initialize",
            {"protocolVersion": "2025-06-18", "capabilities": {},
             "clientInfo": {"name": "e2e-edit", "version": "1"}},
        )
        client.notify("notifications/initialized")

        project = client.call("project_new", {"name": "Edit", "width": 960, "height": 540, "fps": 25})
        session = project["session"]
        imported = client.call("media_import", {"path": str(MEDIA)})
        footage = imported["footage"]["id"]
        clip = client.call(
            "clip_place",
            {"footage": footage, "start": 0, "end": 6, "media_in": 2},
        )["clip"]
        print(f"placed clip {clip}")

        track = client.call("track_add", {"kind": "video"})
        print(f"track_add -> {track}")
        assert track["track"], track

        marker = client.call("marker_add", {"time": 1.5, "name": "intro"})
        markers = client.call("markers_list", {})
        print(f"markers: {markers}")
        assert markers["count"] >= 1, markers

        split = client.call("clip_split", {"clip": clip, "time": 3.0})
        print(f"split -> {split}")
        timeline = client.call("timeline_get", {})
        clips = timeline["sequences"][0]["tracks"][0]["clips"]
        print(f"clips after split: {[(c['in']['seconds'], c['out']['seconds']) for c in clips]}")
        assert len(clips) == 2, clips

        trimmed = client.call(
            "clip_trim", {"clip": clips[0]["id"], "start": 0, "end": 2}
        )
        print(f"trimmed -> {trimmed}")

        effects = client.call("effect_types", {})
        assert effects["count"] > 0
        effect = client.call(
            "effect_add",
            {"host": clip, "type_id": "org.olivevideoeditor.Olive.opacity"},
        )
        print(f"effect_add -> {effect}")
        effect_id = effect["effect"]
        params = client.call("effect_params", {"node": effect_id})
        print(f"effect params: {[p['input_id'] for p in params['parameters']][:10]}")
        assert params["parameters"], params

        first = params["parameters"][0]
        static = client.call(
            "effect_set_param",
            {"node": effect_id, "input_id": first["input_id"], "value": 1.0},
        )
        print(f"set param -> {static}")
        keyed = client.call(
            "effect_set_param",
            {
                "node": effect_id,
                "input_id": first["input_id"],
                "value": 0.5,
                "time": 1.0,
                "interpolation": "linear",
            },
        )
        print(f"set keyframe -> {keyed}")

        history = client.call("history", {})
        print(f"history: {history}")
        undone = client.call("undo", {})
        print(f"undo: {undone}")
        redone = client.call("redo", {})
        print(f"redo: {redone}")

        save = client.call("project_save", {"path": str(saved)})
        print(f"saved -> {save}")
        assert saved.exists() and saved.stat().st_size > 0

        reopened = client.call("project_open", {"path": str(saved)})
        new_session = reopened["session"]
        print(f"reopened -> {reopened}")
        new_timeline = client.call("timeline_get", {"session": new_session})
        new_clips = new_timeline["sequences"][0]["tracks"][0]["clips"]
        print(f"reopened clips: {len(new_clips)}")
        assert len(new_clips) == len(clips), (new_clips, clips)

        export = client.call(
            "export_start",
            {"session": new_session, "filename": str(ranged), "format": "mp4", "start": 0, "end": 2},
        )
        done = client.call("task_wait", {"task": export["task"], "timeout_ms": 180000})
        print(f"range export: {done['state']} {done.get('error')}")
        assert done["state"] == "succeeded", done
        probe = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "csv=p=0", str(ranged)],
            text=True,
        ).strip()
        print(f"range export duration: {probe}")
        assert 1.5 <= float(probe) <= 2.5, probe

        resources = client.request("resources/list")["result"]["resources"]
        print(f"resources: {[item['uri'] for item in resources]}")
        uri = f"quercus://session/{new_session}/timeline"
        body = client.request("resources/read", {"uri": uri})["result"]["contents"][0]
        payload = json.loads(body["text"])
        assert payload["sequences"], payload
        print(f"resource read: {uri} ({len(body['text'])} bytes)")

        prompts = client.request("prompts/list")["result"]["prompts"]
        print(f"prompts: {[item['name'] for item in prompts]}")
        prompt = client.request(
            "prompts/get", {"name": "rough_cut", "arguments": {"media": str(MEDIA)}}
        )["result"]["messages"][0]["content"]["text"]
        assert "media_import" in prompt
        print(f"prompt text: {len(prompt)} chars")

        print("\nOK: edit workflow checks passed")
        return 0
    finally:
        client.close()


if __name__ == "__main__":
    sys.exit(main())
