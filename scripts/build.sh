#!/usr/bin/env bash
# Build/run cargo with the environment Quercus needs.
#   ./scripts/build.sh build --release
#   ./scripts/build.sh test
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z "${FFMPEG_DIR:-}" ]]; then
  if [[ -d "$PWD/.cache/ffmpeg" ]]; then
    export FFMPEG_DIR="$PWD/.cache/ffmpeg"
  else
    echo "FFMPEG_DIR is not set and .cache/ffmpeg does not exist." >&2
    echo "Fetch/build FFmpeg first:" >&2
    echo "  ./scripts/fetch-ffmpeg.sh          # Linux (BtbN shared)" >&2
    echo "  ./scripts/ffmpeg/build-ffmpeg.sh --shared   # macOS" >&2
    echo "  . .\scripts\fetch-ffmpeg.ps1       # Windows (PowerShell)" >&2
    exit 1
  fi
fi

# Bundled OCIO's yaml-cpp needs <cstdint> with GCC >= 13 (Quercus default is
# bundled OCIO; no system-OCIO override).
cargo fetch
bash scripts/fix-ocio-bundled.sh
exec cargo "$@"
