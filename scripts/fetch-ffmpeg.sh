#!/usr/bin/env bash
# Fetch the pinned BtbN FFmpeg 9.0 GPL shared build into .cache/ffmpeg.
# Linux only; macOS has no BtbN build (use scripts/ffmpeg/build-ffmpeg.sh --shared).
#
#   ./scripts/fetch-ffmpeg.sh
#   export FFMPEG_DIR="$PWD/.cache/ffmpeg"
#   export PKG_CONFIG_PATH="$FFMPEG_DIR/lib/pkgconfig"
#   export LD_LIBRARY_PATH="$FFMPEG_DIR/lib"
set -euo pipefail
cd "$(dirname "$0")/.."

[ "$(uname -s)" = "Linux" ] || {
  echo "BtbN has no macOS build; use scripts/ffmpeg/build-ffmpeg.sh --shared" >&2
  exit 1
}

URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-11-10-12-39/ffmpeg-n9.0.2-48-gd4070a14ce-linux64-gpl-shared-9.0.tar.xz"
SHA256="19bc462dc230253465326c4600ca24a72475c6945b4f0c5d2bca522fdc2afc3f"
CACHE=".cache"
ARCHIVE="$CACHE/ffmpeg-linux64-gpl-shared-9.0.tar.xz"
PREFIX="$CACHE/ffmpeg"

mkdir -p "$CACHE"
if [[ ! -f "$PREFIX/lib/libavcodec.so" ]]; then
  echo "==> downloading BtbN FFmpeg 9.0 shared build"
  curl -fL --retry 3 --output "$ARCHIVE" "$URL"
  echo "$SHA256  $ARCHIVE" | sha256sum -c -
  rm -rf "$CACHE/ffmpeg-extract"
  mkdir -p "$CACHE/ffmpeg-extract"
  tar -xJf "$ARCHIVE" -C "$CACHE/ffmpeg-extract"
  rm -rf "$PREFIX"
  mv "$CACHE/ffmpeg-extract"/ffmpeg-* "$PREFIX"
  rm -rf "$CACHE/ffmpeg-extract" "$ARCHIVE"
fi

echo "FFMPEG_DIR=$PWD/$PREFIX"
echo "PKG_CONFIG_PATH=$PWD/$PREFIX/lib/pkgconfig"
echo "LD_LIBRARY_PATH=$PWD/$PREFIX/lib"
