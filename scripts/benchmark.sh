#!/usr/bin/env bash
# Runs the sidecar's `benchmark` NDJSON request over every supported image in
# a folder and prints per-file and per-extension-aggregate timing tables.
#
# This is a permanent diagnostic (see docs/PROJECT-STATUS.md's "RAW/HEIC
# throughput never measured" entry and the raw-performance report at
# .superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/raw-performance-report.md),
# not scaffolding -- re-run it after any change to ImageLoader, Analyzer,
# VisionAnalyzer or Metrics to catch a regression before shipping it.
#
# Usage:
#   scripts/benchmark.sh <folder> [--recursive] [--limit N] [--thumbnails DIR] [--json OUT.json]
#
# Examples:
#   scripts/benchmark.sh ~/Pictures/SonyImport --recursive
#   scripts/benchmark.sh sidecar/Fixtures --limit 20
set -euo pipefail

SUPPORTED_EXT_REGEX='\.(jpg|jpeg|png|heic|heif|cr2|cr3|nef|arw|dng|raf|orf)$'

usage() {
  echo "usage: $0 <folder> [--recursive] [--limit N] [--thumbnails DIR] [--json OUT.json]" >&2
  exit 1
}

[ $# -ge 1 ] || usage
FOLDER="$1"; shift
[ -d "$FOLDER" ] || { echo "error: not a directory: $FOLDER" >&2; exit 1; }

RECURSIVE=0
LIMIT=0
THUMB_DIR=""
JSON_OUT=""

while [ $# -gt 0 ]; do
  case "$1" in
    --recursive) RECURSIVE=1; shift ;;
    --limit) LIMIT="$2"; shift 2 ;;
    --thumbnails) THUMB_DIR="$2"; shift 2 ;;
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
# on both.
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
echo "Benchmarking $COUNT file(s) (sequential, so per-stage numbers are clean -- see Benchmarker.swift doc comment for why this isn't concurrentPerform)..." >&2

# Build the NDJSON request. jq -R -s -c turns the path list into a JSON
# string array without needing to hand-escape paths ourselves.
PATHS_JSON=$(printf '%s\n' "${PATHS[@]}" | jq -R -s -c 'split("\n") | map(select(length > 0))')

if [ -n "$THUMB_DIR" ]; then
  REQUEST=$(jq -n -c --arg id "1" --argjson paths "$PATHS_JSON" --arg dir "$THUMB_DIR" \
    '{id: $id, kind: "benchmark", paths: $paths, thumbnailDir: $dir}')
else
  REQUEST=$(jq -n -c --arg id "1" --argjson paths "$PATHS_JSON" \
    '{id: $id, kind: "benchmark", paths: $paths}')
fi

RESPONSE=$(printf '%s\n' "$REQUEST" | "$BINARY")

if [ -n "$JSON_OUT" ]; then
  echo "$RESPONSE" | jq '.' > "$JSON_OUT"
  echo "wrote raw response to $JSON_OUT" >&2
fi

TYPE=$(echo "$RESPONSE" | jq -r '.result.type')
if [ "$TYPE" != "benchmarked" ]; then
  echo "error: sidecar did not return a benchmarked response:" >&2
  echo "$RESPONSE" | jq '.' >&2
  exit 1
fi

RECORDS=$(echo "$RESPONSE" | jq -c '.result.data')

echo
echo "=== Per-file ==="
echo "$RECORDS" | jq -r '
  ["ext","status","w","h","fallback","hash_ms","exif_ms","decode_ms","vision_ms","metrics_ms","thumb_ms","total_ms","path"],
  (.[] | if .status == "ok" then
      [.result.ext, "ok", (.result.width|tostring), (.result.height|tostring),
       (.result.usedFullDecodeFallback|tostring),
       (.result.timings.hashMs|round|tostring), (.result.timings.exifMs|round|tostring),
       (.result.timings.decodeMs|round|tostring), (.result.timings.visionMs|round|tostring),
       (.result.timings.metricsMs|round|tostring), (.result.timings.thumbnailWriteMs|round|tostring),
       (.result.timings.totalMs|round|tostring), .result.path]
    else
      ["-", "failed", "-", "-", "-", "-", "-", "-", "-", "-", "-", "-", .path]
    end)
  | @tsv' | column -t -s $'\t'

echo
echo "=== Aggregate by extension ==="
echo "$RECORDS" | jq -r '
  [.[] | select(.status == "ok") | .result] as $ok |
  ($ok | group_by(.ext) | map({
      ext: .[0].ext,
      n: length,
      fallback_n: (map(select(.usedFullDecodeFallback)) | length),
      hash_ms_avg: ((map(.timings.hashMs) | add) / length),
      exif_ms_avg: ((map(.timings.exifMs) | add) / length),
      decode_ms_avg: ((map(.timings.decodeMs) | add) / length),
      vision_ms_avg: ((map(.timings.visionMs) | add) / length),
      metrics_ms_avg: ((map(.timings.metricsMs) | add) / length),
      thumb_ms_avg: ((map(.timings.thumbnailWriteMs) | add) / length),
      total_ms_avg: ((map(.timings.totalMs) | add) / length),
      total_ms_sum: (map(.timings.totalMs) | add)
    })) as $groups |
  ["ext","n","fallback_n","fallback_pct","hash_ms_avg","exif_ms_avg","decode_ms_avg","vision_ms_avg","metrics_ms_avg","thumb_ms_avg","total_ms_avg","total_ms_sum"],
  ($groups[] | [
      .ext, (.n|tostring), (.fallback_n|tostring),
      (((.fallback_n / .n) * 100)|round|tostring) + "%",
      (.hash_ms_avg|round|tostring), (.exif_ms_avg|round|tostring),
      (.decode_ms_avg|round|tostring), (.vision_ms_avg|round|tostring),
      (.metrics_ms_avg|round|tostring), (.thumb_ms_avg|round|tostring),
      (.total_ms_avg|round|tostring), (.total_ms_sum|round|tostring)
    ])
  | @tsv' | column -t -s $'\t'

FAILED_COUNT=$(echo "$RECORDS" | jq '[.[] | select(.status == "failed")] | length')
if [ "$FAILED_COUNT" -gt 0 ]; then
  echo
  echo "=== $FAILED_COUNT failed file(s) ==="
  echo "$RECORDS" | jq -r '.[] | select(.status == "failed") | "\(.path): \(.message)"'
fi
