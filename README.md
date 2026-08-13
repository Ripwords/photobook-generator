# PhotobookGen

A macOS photobook layout app built with Tauri 2 (Rust) + Nuxt 4, backed by a Swift
sidecar (`sidecar/`, SwiftPM package `PhotobookEngine`) that performs photo analysis.
Target platform is macOS 15.0+, arm64 only.

## Prerequisites

- [Bun](https://bun.sh) (package manager)
- Rust (stable toolchain, arm64 macOS target)
- Xcode / Swift toolchain (for the sidecar package)

## Getting started

```bash
bun install
bun run dev
```

`bun run dev` and `bun run build` both build the Swift sidecar automatically via Tauri's
`beforeDevCommand` / `beforeBuildCommand` hooks (see `src-tauri/tauri.conf.json`), so no
manual step is normally required.

### Important: building/testing the Rust crate directly

`src-tauri/tauri.conf.json` declares the sidecar binary under `bundle.externalBin`. This
makes `tauri-build`'s build script validate, **at compile time**, that
`src-tauri/binaries/photobook-engine-<target-triple>` exists on disk — before any Rust
code is compiled.

If you invoke `cargo build` or `cargo test` **directly** inside `src-tauri/` (bypassing
the `bun run dev` / `bun run build` hooks above), you must build the sidecar first:

```bash
bun run sidecar
cargo test --manifest-path src-tauri/Cargo.toml
```

Skipping this produces an error like:

```
resource path `binaries/photobook-engine-aarch64-apple-darwin` doesn't exist
```

which points at the missing binary, not at the missing `bun run sidecar` step — this note
exists so that error is easy to diagnose.

## Scripts

- `bun run dev` — start the Tauri app in development mode (builds sidecar first)
- `bun run build` — build the production app bundle (builds sidecar first)
- `bun run sidecar` — build the Swift sidecar binary and copy it into
  `src-tauri/binaries/`
- `bun run test` — run frontend tests (Vitest)
- `bun run test:rust` — run Rust tests (`cargo test`)
- `bun run test:swift` — run Swift tests (`swift test`)
- `scripts/benchmark.sh <folder>` — measure sidecar per-photo performance against real
  photos (see below)

## Benchmarking photo analysis performance

`scripts/benchmark.sh` drives the sidecar's `benchmark` NDJSON request (alongside `ping`
and `analyze` — see `sidecar/Sources/PhotobookEngine/Protocol.swift`) over every supported
image in a folder and reports per-stage wall-clock timings: file hash, EXIF read, decode,
Vision pass, classical metrics, and (optionally) thumbnail write. It prints a per-file
table and a per-extension aggregate, so RAW, HEIC and JPEG throughput are directly
comparable instead of blended into one number.

This is a permanent diagnostic, not scaffolding — re-run it after any change to
`ImageLoader`, `Analyzer`, `VisionAnalyzer` or `Metrics` to catch a throughput or
correctness regression before it ships. It's also how the RAW decode performance fix was
measured; see
`.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/raw-performance-report.md` for the
baseline-vs-fix numbers, the per-format embedded-preview fallback rates, and a quality
comparison between the preview-decode and full-decode paths on real RAW files.

```bash
bun run sidecar                                    # builds the sidecar binary if needed
scripts/benchmark.sh ~/Pictures/SonyImport --recursive
scripts/benchmark.sh sidecar/Fixtures --limit 20    # quick smoke test, no real photos needed
```

Flags:

- `--recursive` — descend into subfolders (real photo exports are often nested; the app's
  own folder scan is intentionally non-recursive today, see `docs/PROJECT-STATUS.md`'s
  "known smaller debts", but this script isn't bound by that)
- `--limit N` — only benchmark the first N matching files
- `--thumbnails DIR` — also time the contact-sheet thumbnail write stage, writing into
  `DIR` (omit to skip that stage entirely, which is the default — most throughput
  questions are about decode/Vision, not thumbnail writing)
- `--json OUT.json` — dump the raw sidecar response alongside the printed tables

Requires `jq`. The script builds the sidecar binary automatically (via `bun run sidecar`)
if `src-tauri/binaries/photobook-engine-<target-triple>` doesn't exist yet.

Benchmark runs are deliberately **sequential**, not the concurrent `concurrentPerform` fan-out
a real `analyze` batch uses — see `Benchmarker.swift`'s doc comment. That makes per-stage
numbers clean and comparable across formats, but means a benchmark run's total wall time
under-represents real multi-core throughput; look at the aggregate ratios and per-format
comparisons, not the raw total, when judging real-world speed.
