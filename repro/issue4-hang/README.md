# Issue #4 repro: export hangs after a long `render_audio`

Observed against `quercus-engine-rs 0.5.7` / `quercus-engine 0.5.3` /
internal crates `0.5.2` through `oak-mcp 0.1.0`.

## Files

| Path | What |
|---|---|
| `repro.py` | The failing flow: build 33.77s timeline -> `render_audio` -> import WAV on A2 -> 5 previews -> `export_start`. |
| `control_prerendered_wav.py` | Same timeline, pre-made `sine33.wav` on A2, **no** `render_audio` in this process. Expected to succeed. |
| `control_short_6s.py` | 6s version of `render_audio` then export. Expected to succeed. |
| `common.py` | MCP driver + timeline builder shared by the scripts. |
| `media` | Symlink to `../../media/shuili` (12 Sony 4K50p clips + `sine33.wav`, ~3.0 GB). |
| `files.txt` | Required media files and sizes. |

The media were copied from the NAS share
`176XXXX0383/视频/20260825下午水利枢纽参观/视频/C05xx.MP4`; any media with the
same names works. To use another directory:

```sh
OAK_REPRO_MEDIA=/path/to/media python3 repro.py
```

## Prerequisites

- `cargo build` has produced `../../target/debug/oak-mcp`
  (override with `OAK_MCP_BIN=/path/to/oak-mcp`).
- ~4 GB free for the exported files.
- Linux x86-64 (the repro was run there).

## Run the failing case

```sh
cd repro/issue4-hang
REPRO_EXPORT_TIMEOUT=600 python3 repro.py
```

Expected output (abridged):

```
server pid: 12345
timeline: 12 clips, 33.766s
render_audio: peak ... dB, 1620800 frames -> .../repro_issue4_mix.wav
export started: t1 -> .../repro_issue4.mp4
export state after 600.0s: running

HANG REPRODUCED: task still running after 600s, file size ~8.9 MB (moov missing)
server pid 12345
attach with:
  sudo sysctl -w kernel.yama.ptrace_scope=0
  gdb -p 12345 -batch -ex 'thread apply all bt'
holding the process for 300s ...
```

While hung: the server is alive with many threads, CPU drops to ~0%, and the
output file stops growing (the MP4 `moov` atom is never written). The script
keeps the process alive for 300s so you can attach; set
`REPRO_HOLD_ON_HANG=0` to kill it immediately.

## Controls (should pass)

```sh
python3 control_short_6s.py            # ~7 s, succeeds
python3 control_prerendered_wav.py     # ~150-170 s, succeeds
```

- `control_short_6s.py` proves a short `render_audio` before export is fine.
- `control_prerendered_wav.py` proves the 33.77s timeline itself exports fine
  when the audio WAV is pre-made and no `render_audio` runs in the export
  process (this is also the workaround used for `oak_cut_audio.mp4`). It uses
  `repro_issue4_mix.wav` when present (the same clipped mix `repro.py` writes)
  so the only difference is the in-process `render_audio`; the clipped WAV
  alone exports fine in a fresh process.

## Exact parameters

- Sequence: 1280x720, 30 fps.
- 12 clips, `clip_place_linked` (video + audio), 3.0 s each, media-in 0.4-1.0 s.
- 11 `transition_add` at each seam, half = 0.4 s.
- `opacity` effect on the first clip with keyframes 0.0 @ 0.0 s and 1.0 @ 0.8 s.
- `render_audio(0..33.766s, gain_db=18)` writing a 48 kHz stereo WAV
  (`REPRO_GAIN_DB=0` disables the boost; +18 clips the peaks at 0 dBFS, which
  is what the original failing run used).
- WAV imported and placed on audio track index 1 (A2), 0..33.766 s.
- 5 `preview_frame` calls (1, 6, 12, 20, 28 s) before the export.
- `export_start` 1280x720, 30 fps, H.264 + AAC.

## Notes

- The export audio montage in `quercus-task 0.5.2`
  (`crates/quercus-task/src/render.rs:501`) hardcodes `stream_index: 0`, so the
  A1 clips' embedded audio is decoded as stream 0 (video). `render_audio` uses
  the correct preview-audio path (`preview_audio.rs:142`) which is why the WAV
  mix has sound.
- `render_audio` + export in the same process on this timeline hangs; the two
  controls isolate the trigger to the long session, not the timeline itself.
- If `kernel.yama.ptrace_scope` is 1 (default here), gdb cannot attach from a
  different process tree; either lower it (`sudo sysctl -w
  kernel.yama.ptrace_scope=0`) or start the repro under gdb.
