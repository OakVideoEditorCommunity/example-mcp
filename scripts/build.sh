#!/usr/bin/env bash
# Build/run cargo with the environment Quercus needs.
#   ./scripts/build.sh build --release
#   ./scripts/build.sh test
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z "${FFMPEG_DIR:-}" ]]; then
  echo "FFMPEG_DIR is not set. Point it at Quercus' pinned FFmpeg 8.1 prefix:" >&2
  echo "  export FFMPEG_DIR=/path/to/quercus-engine/.cache/ffmpeg" >&2
  exit 1
fi

export OCIO_INSTALL_DIR="${OCIO_INSTALL_DIR:-/usr}"
export OCIO_RS_LINK="${OCIO_RS_LINK:-dynamic}"
exec cargo "$@"
