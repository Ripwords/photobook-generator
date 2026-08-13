#!/usr/bin/env bash
set -euo pipefail

TRIPLE="$(rustc --print host-tuple)"
OUT="src-tauri/binaries"

swift build --package-path sidecar -c release \
  -Xswiftc -target -Xswiftc arm64-apple-macos15.0

mkdir -p "$OUT"
cp "sidecar/.build/release/PhotobookEngine" "$OUT/photobook-engine-${TRIPLE}"
echo "built $OUT/photobook-engine-${TRIPLE}"
