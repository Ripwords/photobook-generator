# Task 15 report: analyze_folder command and results UI

Commit: `b883ef6` — "feat: add analyze_folder command and results view"
Branch: `feat/phase-1-analysis-pipeline`

## What was built

- `src-tauri/src/commands.rs` (new): `supported_extension`, `hash_file` (SHA-256
  over raw bytes via streaming reads, no image decode), `AppState`,
  `AnalysisSummary`, `finalize_photos` (pure, see below), and the
  `#[tauri::command] analyze_folder`.
- `src-tauri/src/lib.rs` (modified): registers `mod commands`,
  `.manage(commands::AppState::default())`, and
  `invoke_handler(tauri::generate_handler![commands::analyze_folder])`.
- `app/types/features.ts` (new): `AnalyzedPhoto`, `FailedPhoto`,
  `AnalysisSummary`, `isFailed`, `keepers`.
- `app/composables/useAnalysis.ts` (new): `useAnalysis()` wiring the native
  folder picker to `invoke("analyze_folder", …)`.
- `app/pages/index.vue` (modified): button, summary counts, keepers `UTable`.
- `tests/features.test.ts` (new), `src-tauri/src/commands.rs` `#[cfg(test)]`
  module.

One deliberate deviation from the brief's literal code: the derivation logic
that the brief inlines directly into `analyze_folder` (sort-by-path, cluster,
percentile, phash-strip) is factored into a standalone `pub(crate) fn
finalize_photos(Vec<Value>) -> Vec<Value>`. `analyze_folder` itself needs a
live `AppHandle`/`SidecarPool`/`Db`, so it can't be unit tested directly; this
mirrors the exact reasoning already in the codebase for
`sidecar::analyze_batches` (extracted from `SidecarPool::analyze_all` in Task
14 for the same reason — see that file's own doc comment). This let me pin
the ordering/phash-stripping contracts directly instead of only trusting the
inline code by inspection.

## TDD sequence

1. Wrote `tests/features.test.ts` and `src-tauri/src/commands.rs` (test module
   only, no implementation) plus `pub mod commands;` in `lib.rs`.
2. Confirmed both fail:
   - `bun run test`: `Error: Cannot find module '../app/types/features'`
   - `cargo test`: 7× `error[E0425]: cannot find function 'supported_extension'/'hash_file' in this scope`
3. Implemented `app/types/features.ts`, `src-tauri/src/commands.rs` (full),
   rest of `lib.rs`, `useAnalysis.ts`, `index.vue`.
4. Confirmed both pass (full output below).
5. Committed.

## Mutation evidence

**The brief's own "drops utility photos from keepers" test is vacuous.**
Both its photos default to `nearDupCluster: 0` and tie on `sharpnessPct`/
`aestheticPct` (50/50). Deleting the `if (photo.isUtility) continue;` line
entirely does **not** change the test's outcome: both photos would still
funnel into the same cluster-0 slot in the `best` map, the tie-break keeps
whichever was inserted first (the incumbent, since neither of the two
`sharpnessPct >` / `aestheticPct >` conditions is satisfied on a tie), and the
result is still length 1. I verified this by hand-tracing `keepers()` with
the filter removed — the dedup-by-cluster logic alone collapses it.

I did not alter the brief's test (per instructions). Instead I added:
- `"drops a utility photo even when it is the sole member of its own cluster"`
  — puts the utility photo in a distinct cluster (`nearDupCluster: 2`) so
  dedup can't accidentally absorb it; a deleted filter now visibly changes
  the result from length 1 to length 2.
- `"drops all photos when every photo is utility"` — both non-utility
  survivors' clusters are also utility; catches a filter that only fires on
  a subset.
- `"breaks a sharpness tie using aesthetic percentile"` — isolates the
  secondary tie-break condition, which none of the brief's tests exercise.

The brief's `"keeps one photo per near-duplicate cluster"` test **is** sound:
returning every photo instead of deduping would give length 3, not 2, so it
fails correctly under that mutation.

**Rust side** (`finalize_photos`, all new tests beyond the brief's
extension/hash tests, which are fine as written):
- `strips_phash_from_output` — pins contract #3.
- `sorts_output_by_path_regardless_of_input_order` — pins contract #4.
- `face_count_is_derived_from_the_faces_array_length`.
- `attributes_percentile_data_to_the_correct_photo_after_sorting` and
  `attributes_event_cluster_to_the_correct_photo_after_sorting` — pin contract
  #1 directly: reverse-path-order input with distinguishing scores/timestamps,
  asserting the derived value travels with the right photo post-sort, not the
  pre-sort index. I hand-verified this catches the specific bug the brief
  warns about (computing derived arrays before sorting, then zipping by index
  after sorting) — under that mutation the two-element case here would swap
  the assertions and fail.

## Verified

```
$ bun run test
 Test Files  3 passed (3)
      Tests  412 passed (412)

$ cargo test --manifest-path src-tauri/Cargo.toml
test result: ok. 59 passed; 0 failed  (lib unit tests, incl. 9 new in commands::tests)
test result: ok. 1 passed; 0 failed   (tests/sidecar_ping.rs integration test)

$ bun run lint
(no output — clean)

$ bun run test:swift
Test run with 67 tests in 1 suite passed after 4.836 seconds.
```

### Hostile fixture corpus (Phase 1 exit criterion)

Drove the real built sidecar binary
(`src-tauri/binaries/photobook-engine-aarch64-apple-darwin`) directly over
NDJSON with all 3 top-level `sidecar/Fixtures/*.jpg` plus all 8
`sidecar/Fixtures/hostile/*` files in one `analyze` request:

```
EXIT CODE: 0
record count: 11
ok      landscape.jpg
ok      gps-south-west.jpg
ok      portrait-rot90.jpg
failed  hostile/truncated.jpg          decodeFailed(...)
failed  hostile/empty.jpg              unreadable(...)
ok      hostile/cmyk.jpg
ok      hostile/one-pixel.jpg
ok      hostile/corrupt-scan-data.jpg
failed  hostile/random.jpg             unreadable(...)
ok      hostile/no-extension
failed  hostile/text.jpg               unreadable(...)
```

11 inputs → 11 records, process exits 0. Matches the one-record-per-path
guarantee `analyze_batches`/`finalize_photos` depend on.

### App launch, build, and bundling

- `bun run sidecar && bun run dev`: compiled cleanly, Tauri window opened.
  Screenshot of the dev-mode window (title bar traffic lights dim/inactive,
  as expected in dev) confirms "Photobook Generator" title and "Choose photo
  folder" button render correctly.
- `bun run build`: release build succeeded (57s), produced
  `src-tauri/target/release/bundle/macos/Photobook Generator.app`.
  - `codesign -dv`: `flags=0x20002(adhoc,linker-signed)` — signed (ad-hoc,
    matching this project having no configured signing identity).
  - `Contents/MacOS/photobook-engine` present, Mach-O 64-bit arm64 — sidecar
    is embedded in the bundle.
  - Launched the actual signed `.app` via `open`, confirmed the window opens
    with active (colored) traffic lights and the same UI renders correctly.
    Screenshot captured of the app window only (via window-ID-scoped
    `screencapture -l<id>`, resolved through a small Swift `CGWindowListCopyWindowInfo`
    snippet — no accessibility APIs involved, just window enumeration).

### What could NOT be verified: interactive picker → table render

I could not click "Choose photo folder", drive the native `NSOpenPanel`, or
watch the keepers table render inside the live app window. `osascript`/System
Events UI scripting is blocked in this environment
(`osascript -e 'tell application "System Events" to return UI elements enabled'`
→ `false`; a direct scripting attempt errored `-1719 "not allowed assistive
access"`). Granting Accessibility permission requires a human clicking
"Allow" in System Settings, which I did not attempt to prompt for or work
around. No `cliclick`/Hammerspoon were available either, and Tauri's WKWebView
isn't reachable via Playwright/CDP the way an Electron/Chromium app would be.

I tried one more avenue: `tauri::test::mock_builder()` (a real, documented
Tauri testing pattern) to call `commands::analyze_folder` directly against
`sidecar/Fixtures` without a window. This dead-ended at compile time —
`analyze_folder`'s generated command signature takes a concrete
`AppHandle<Wry>` (the default runtime), not one generic over `Runtime`, so it
rejects `MockRuntime`'s `AppHandle`. Making it generic purely for this
throwaway check felt like a bigger, unrequested production-code change, so I
reverted that attempt (temporary `Cargo.toml` dev-dependency and test file,
both removed — final commit only contains the brief's file list).

**What stands in for it:** every step `analyze_folder` performs is otherwise
verified — `hash_file`/`supported_extension` unit tested; the real sidecar
binary verified end-to-end against the real Fixtures directory including the
hostile corpus; `finalize_photos` (sort, cluster, percentile, phash-strip)
unit tested against realistic JSON shapes with positional-mismatch-catching
assertions; `Db`/`cluster`/`ranking`/`SidecarPool` each already covered by
their own test suites (pre-existing, all still green). What's *not* covered
by any of this is the literal glue of AppHandle → dialog → invoke → Vue
reactivity → UTable render, which is thin (the code the brief specifies,
copied close to verbatim) but genuinely unexercised end-to-end.

### RAW/HEIC throughput — explicitly out of scope per instructions

Per the task instructions I do not have access to and did not go looking for
the user's own photos. RAW and HEIC decode throughput (spec §14) remains
**unmeasured**. The Fixtures directory contains only JPEGs, so this also
couldn't be indirectly exercised. This is a real gap against the Phase 1 exit
criteria ("RAW throughput measured and recorded") that requires the user to
run `bun run dev`, pick a folder of their own HEIC/RAW photos, and time it
per the brief's Step 4 instructions (also worth spot-checking 3 portrait
photos for `width < height` at that point, per the brief).

## The NaN/percentile seam

`percentiles()` returns 0 for both a genuine bottom-of-population score and a
NaN input — asked to assess whether NaN is reachable here.

**Conclusion: not reachable in the current pipeline**, for three independent reasons:

1. `aestheticScore` comes from `VNImageAestheticsScoresObservation.overallScore`
   (a bounded `Float`, `Analyzer.swift`/`VisionAnalyzer.swift`), not a
   division or other NaN-producing computation.
2. `sharpness` (`Metrics.swift`) is a Laplacian-variance-over-contrast ratio
   with `let contrast = max(hi - lo, 1.0)` — an explicit floor that forecloses
   the one obvious `0/0` div-by-zero path.
3. Even if either value somehow became NaN in-process, Swift's `JSONEncoder`
   **cannot serialize `Double.nan`** — it throws `EncodingError`, which
   `Handler.swift`'s `emit()` catches at the whole-`Response` level and
   replaces with a generic `{"type":"error", "message":"internal encoding
   error"}` for that request id. On the Rust side that surfaces as
   `SidecarError::Engine`/`Malformed`, which `analyze_batches` turns into
   synthetic `failed` records for the whole batch — never as a numeric NaN
   flowing through to `percentiles()`. `serde_json::Value::as_f64()` also
   cannot itself produce `NaN`: JSON's number grammar has no NaN literal, so
   a missing/non-numeric field yields `None` (→ `unwrap_or(0.0)`), not a NaN
   value.

So the `percentiles` NaN-containment behavior (tested thoroughly in
`ranking.rs`, itself good defensive design) is currently unreachable dead
code from this pipeline's actual data flow — worth keeping as a hard
guarantee against future producers of the `aestheticScore`/`sharpness`
fields, but not something a real photo can trigger today.

## Deviations from the brief

1. `finalize_photos` extraction (see above) — behavior-identical, adds
   testability.
2. Nine additional tests beyond the brief's four TS + five Rust tests (see
   Mutation evidence). No brief test was modified or removed.
3. Ran `bun run build` and inspected the resulting bundle (not explicitly
   required by Step 4, but is a Phase 1 exit criterion) — confirmed signed
   and sidecar-embedded.

## Uncertain / worth a second look

- The webview receives every raw `PhotoFeatures` field the Swift sidecar
  produces (`exif`, `faceAreaFraction`, `saliencyBox`, `horizonTiltDeg`,
  `hasText`, `palette`, `warmth`, `contrast`, plus the un-normalized
  `aestheticScore`/`sharpness`) in addition to the fields declared on
  `AnalyzedPhoto` — `phash` is the only field actually stripped. This matches
  the brief's contract #3 literally ("strip `phash`... the UI has no use for
  it") and is not a bug, but it does mean the TS `AnalyzedPhoto` interface
  is narrower than the real runtime object; TypeScript's structural typing
  doesn't flag this (no excess-property check applies to a value flowing
  through `invoke<T>`, only to object literals), so nothing breaks, but it's
  worth knowing before anyone reaches for a field not declared on the
  interface and it silently works.
- GUI interactive verification gap above is the main open item before Phase 1
  can be called fully exited.
