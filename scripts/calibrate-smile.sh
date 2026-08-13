#!/usr/bin/env bash
# Runs the sidecar's `calibrate` NDJSON request over every supported image in
# a folder and prints a per-face table of SmileProxy's raw geometry, plus a
# summary of the `lift` (and candidate `midpointLift`) distribution.
#
# This exists because SmileProxy.confidence(for:)'s calibration constants
# ([-0.15, 0.35] mapped onto 0...1) were never measured -- see the doc
# comment on SmileProxy.swift and docs/PROJECT-STATUS.md. Run this against a
# folder of clearly smiling faces and a folder of clearly neutral ones; the
# two `lift` distributions are what should set those constants, not another
# guess.
#
# Usage:
#   scripts/calibrate-smile.sh <folder> [--recursive] [--limit N] [--json OUT.json]
#
# Examples:
#   scripts/calibrate-smile.sh ~/Pictures/SmilingFaces
#   scripts/calibrate-smile.sh ~/Pictures/NeutralFaces
#   scripts/calibrate-smile.sh sidecar/Fixtures   # no faces -- zero rows, no crash
set -euo pipefail

SUPPORTED_EXT_REGEX='\.(jpg|jpeg|png|heic|heif|cr2|cr3|nef|arw|dng|raf|orf)$'

usage() {
  echo "usage: $0 <folder> [--recursive] [--limit N] [--json OUT.json]" >&2
  exit 1
}

[ $# -ge 1 ] || usage
FOLDER="$1"; shift
[ -d "$FOLDER" ] || { echo "error: not a directory: $FOLDER" >&2; exit 1; }

RECURSIVE=0
LIMIT=0
JSON_OUT=""

while [ $# -gt 0 ]; do
  case "$1" in
    --recursive) RECURSIVE=1; shift ;;
    --limit) LIMIT="$2"; shift 2 ;;
    --json) JSON_OUT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TRIPLE="$(rustc --print host-tuple 2>/dev/null || echo aarch64-apple-darwin)"
BINARY="$ROOT_DIR/src-tauri/binaries/photobook-engine-${TRIPLE}"

if [ ! -x "$BINARY" ]; then
  echo "sidecar binary not found at $BINARY -- building it (bun run sidecar)..." >&2
  (cd "$ROOT_DIR" && bun run sidecar)
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required (brew install jq)" >&2
  exit 1
fi

echo "Collecting images under $FOLDER ..." >&2
if [ "$RECURSIVE" = "1" ]; then
  FIND_ARGS=(-type f)
else
  FIND_ARGS=(-maxdepth 1 -type f)
fi

# `mapfile` is a bash-4 builtin; macOS ships bash 3.2 at /bin/bash, so this
# reads into the array with a plain `while read` loop instead, which works
# on both (same fix as scripts/benchmark.sh).
PATHS=()
while IFS= read -r found_path; do
  PATHS+=("$found_path")
done < <(find "$FOLDER" "${FIND_ARGS[@]}" | grep -iE "$SUPPORTED_EXT_REGEX" | sort)

if [ "$LIMIT" -gt 0 ] 2>/dev/null; then
  PATHS=("${PATHS[@]:0:$LIMIT}")
fi

COUNT=${#PATHS[@]}
if [ "$COUNT" -eq 0 ]; then
  echo "no supported images found under $FOLDER (looked for: jpg jpeg png heic heif cr2 cr3 nef arw dng raf orf)" >&2
  exit 1
fi
echo "Calibrating $COUNT file(s)..." >&2

# Build the NDJSON request. jq -R -s -c turns the path list into a JSON
# string array without needing to hand-escape paths ourselves.
PATHS_JSON=$(printf '%s\n' "${PATHS[@]}" | jq -R -s -c 'split("\n") | map(select(length > 0))')

REQUEST=$(jq -n -c --arg id "1" --argjson paths "$PATHS_JSON" \
  '{id: $id, kind: "calibrate", paths: $paths}')

RESPONSE=$(printf '%s\n' "$REQUEST" | "$BINARY")

if [ -n "$JSON_OUT" ]; then
  echo "$RESPONSE" | jq '.' > "$JSON_OUT"
  echo "wrote raw response to $JSON_OUT" >&2
fi

TYPE=$(echo "$RESPONSE" | jq -r '.result.type')
if [ "$TYPE" != "calibrated" ]; then
  echo "error: sidecar did not return a calibrated response:" >&2
  echo "$RESPONSE" | jq '.' >&2
  exit 1
fi

RECORDS=$(echo "$RESPONSE" | jq -c '.result.data')

# Every face row across every "ok" record, flattened, path already on each
# row. Images with zero detected faces contribute nothing here -- that's
# expected and not an error (see CalibrationRecord's doc comment in
# Calibrator.swift).
ROWS=$(echo "$RECORDS" | jq -c '[.[] | select(.status == "ok") | .faces[]]')
ROW_COUNT=$(echo "$ROWS" | jq 'length')

echo
echo "=== Per-face geometry ==="
if [ "$ROW_COUNT" -eq 0 ]; then
  echo "(no faces detected in any file under $FOLDER)"
else
  echo "$ROWS" | jq -r '
    ["path","lipPts","cornerY","centreY","width","lift","midlineY","midpointLift","confidence","reason"],
    (.[] | [
        (.path | split("/") | last),
        (.outerLipPointCount|tostring),
        (if .cornerY then (.cornerY*1000|round/1000|tostring) else "-" end),
        (if .centreY then (.centreY*1000|round/1000|tostring) else "-" end),
        (if .width then (.width*1000|round/1000|tostring) else "-" end),
        (if .lift then (.lift*1000|round/1000|tostring) else "-" end),
        (if .midlineY then (.midlineY*1000|round/1000|tostring) else "-" end),
        (if .midpointLift then (.midpointLift*1000|round/1000|tostring) else "-" end),
        (if .confidence then (.confidence*1000|round/1000|tostring) else "-" end),
        (.unusableReason // "-")
      ])
    | @tsv' | column -t -s $'\t'
fi

# jq median helper: sorted middle value, averaging the two middles on an
# even-length array. Guarded against an empty array by the caller (`stats`
# below only runs it on already-filtered, non-empty selections when possible,
# and returns null cleanly on an empty one either way).
JQ_MEDIAN='def median: sort as $s | ($s|length) as $n | if $n == 0 then null elif ($n % 2) == 1 then $s[($n-1)/2] else ($s[$n/2 - 1] + $s[$n/2]) / 2 end;'

echo
echo "=== Summary ==="
echo "Faces detected (all, including pose/point-count-gated): $ROW_COUNT"

USABLE_COUNT=$(echo "$ROWS" | jq '[.[] | select(.lift != null)] | length')
echo "Faces with usable geometry (lift computed): $USABLE_COUNT"

UNUSABLE_COUNT=$((ROW_COUNT - USABLE_COUNT))
if [ "$UNUSABLE_COUNT" -gt 0 ]; then
  echo
  echo "Excluded (unusable) by reason:"
  echo "$ROWS" | jq -r '[.[] | select(.unusableReason != null) | .unusableReason] | group_by(.) | map("  \(.[0]): \(length)") | .[]'
fi

if [ "$USABLE_COUNT" -eq 0 ]; then
  echo
  echo "lift: n/a (no usable faces)"
  echo "midpointLift: n/a (no usable faces)"
else
  echo
  # $JQ_MEDIAN is bash-expanded (double-quoted); the rest is single-quoted so
  # jq's own "$lift"/"\(...)" syntax reaches jq untouched instead of being
  # interpreted by bash.
  echo "$ROWS" | jq -r "$JQ_MEDIAN"'
    [.[] | select(.lift != null) | .lift] as $lift |
    [.[] | select(.midpointLift != null) | .midpointLift] as $mlift |
    "lift          n=\($lift|length)  min=\($lift|min|.*1000|round/1000)  median=\($lift|median|.*1000|round/1000)  max=\($lift|max|.*1000|round/1000)",
    "midpointLift  n=\($mlift|length)  min=\($mlift|min|.*1000|round/1000)  median=\($mlift|median|.*1000|round/1000)  max=\($mlift|max|.*1000|round/1000)"
  '
fi

FAILED_COUNT=$(echo "$RECORDS" | jq '[.[] | select(.status == "failed")] | length')
if [ "$FAILED_COUNT" -gt 0 ]; then
  echo
  echo "=== $FAILED_COUNT failed file(s) ==="
  echo "$RECORDS" | jq -r '.[] | select(.status == "failed") | "\(.path): \(.message)"'
fi
