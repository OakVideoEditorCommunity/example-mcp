#!/usr/bin/env bash
# Bundled OCIO's vendored yaml-cpp is missing <cstdint> with GCC >= 13.
# Quercus default is bundled OCIO, so add the include to the cargo registry
# copy after `cargo fetch` (cargo does not re-verify extracted sources).
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="${CARGO_HOME:-$HOME/.cargo}"
patched=0
for f in "$ROOT"/registry/src/*/ocio-sys-0.2.1/vendor/OpenColorIO/ext/ocio-rs-deps/yaml-cpp/src/emitterutils.cpp; do
  [ -f "$f" ] || continue
  if ! grep -q '^#include <cstdint>' "$f"; then
    sed -i '1i #include <cstdint>' "$f"
    echo "patched $f"
    patched=1
  fi
done
if [ "$patched" = 0 ]; then
  echo "ocio-sys yaml-cpp already patched (or not fetched yet)"
fi
