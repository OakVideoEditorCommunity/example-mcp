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
    echo "Build it first:" >&2
    echo "  ./scripts/ffmpeg/build-ffmpeg.sh   # Linux/macOS" >&2
    echo "  . .\scripts\fetch-ffmpeg.ps1      # Windows (PowerShell)" >&2
    exit 1
  fi
fi

export OCIO_INSTALL_DIR="${OCIO_INSTALL_DIR:-/usr}"
export OCIO_RS_LINK="${OCIO_RS_LINK:-dynamic}"
exec cargo "$@"
