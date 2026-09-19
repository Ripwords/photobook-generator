#!/usr/bin/env bash
# CI wrapper for `swift test`: fails after a deadline instead of hanging, and
# dumps the stacks of every test process first so a hang is diagnosable from
# the log. The Swift suite takes ~15s on a dev Mac; on a GitHub macos-15
# runner it once hung silently for 25 minutes (Vision has deadlocked this
# suite before, see sidecar/Sources/PhotobookEngine/VisionGate.swift).
set -uo pipefail

DEADLINE="${SWIFT_TEST_DEADLINE:-300}"

# Build first so compile time does not count against the test deadline.
swift build --package-path sidecar --build-tests || exit $?

swift test --package-path sidecar --skip-build &
pid=$!

for ((i = 0; i < DEADLINE; i++)); do
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"
    exit $?
  fi
  sleep 1
done

echo "::error::swift test still running after ${DEADLINE}s; sampling stacks"
for p in $(pgrep -f 'swiftpm-testing-helper|PhotobookEnginePackageTests|xctest'); do
  echo "===== sample of pid $p: $(ps -o command= -p "$p") ====="
  sample "$p" 5 2>&1
done
pkill -P "$pid" 2>/dev/null
kill "$pid" 2>/dev/null
exit 1
