# Delivery

## Build

```sh
./scripts/build.sh build --release
```

Windows (PowerShell, after `. .\scripts\fetch-ffmpeg.ps1`):

```powershell
cargo build --release
```

## Package

Ship a folder, not a bare executable:

```
<studio>-mcp/
├── <studio>-mcp          (or .exe on Windows)
├── libquercus_engine.so  (libquercus_engine.dylib / quercus_engine.dll)
├── LICENSE
├── README.md
└── STUDIO.md
```

Zip it, publish only if the user asks, and include the engine version
(`quercus-engine-rs 0.5.7`).

## Client config

```json
{
  "mcpServers": {
    "<studio>": {
      "command": "/absolute/path/to/<studio>-mcp/<studio>-mcp",
      "args": []
    }
  }
}
```

Tell the user to restart the client, then try one sample prompt.

## `STUDIO.md` template

```markdown
# <Studio> MCP

## Tools
- `import_folder` — import a camera-card folder
- `assemble_rushes` — lay imported clips on the timeline in order
- ...

## Try
1. "Import /Volumes/RUSHES/DAY01 and assemble it."
2. "Make a 30-second cut and export a 720p review to /Volumes/DELIVERY."

## Defaults
- 1920×1080, 25 fps, H.264 MP4
- Export to /Volumes/DELIVERY

## Limits
- Subtitle/audio workarounds from the README
- No source-media deletion
```

## License

GPL-3.0-or-later. Keep `LICENSE` and SPDX headers. If the studio distributes
the server outside the company, the GPL source/notice obligations apply.
