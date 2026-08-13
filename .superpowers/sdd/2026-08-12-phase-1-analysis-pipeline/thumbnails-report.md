# Thumbnail pipeline — implementation report

Branch: `feat/phase-1-analysis-pipeline`

## What was built

### 1. Swift — thumbnail writing (`sidecar/Sources/PhotobookEngine/`)

- **New: `ThumbnailWriter.swift`** — `ThumbnailWriter.write(image:hash:directory:maxPixel:)`.
  - Skips re-encoding if `<directory>/<hash>.jpg` already exists (existence check happens *before* any resize/encode work).
  - Otherwise creates the directory if absent, downsamples the already-decoded `CGImage` via a `CGContext` draw (no second file decode), and writes JPEG via `CGImageDestinationCreateWithURL` + `UTType.jpeg` at quality 0.8.
  - Long edge target is a parameter; `Analyzer` calls it with `400`.

- **`Analyzer.swift`**:
  - `PhotoFeatures` gained `thumbnailPath: String?`.
  - `analyzeOne(path:thumbnailDir:)` (new optional param, defaults to `nil`) reuses the `CGImage` already decoded via `ImageLoader.loadThumbnail` for the main analysis pass — never decodes twice. Thumbnail write is wrapped in `do/catch`: on failure it logs to stderr and sets `thumbnailPath = nil` without throwing, so the rest of the `PhotoFeatures` record is unaffected.
  - `analyze(paths:thumbnailDir:)` threads the directory through to each `analyzeOne` call under the existing `concurrentPerform` fan-out.

- **`Protocol.swift`**: `Request` gained `var thumbnailDir: String?`.
- **`Handler.swift`**: `.analyze` case passes `request.thumbnailDir` through to `Analyzer.analyze`.

### 2. Rust — wiring (`src-tauri/src/`)

- **`protocol.rs`**: `Request` gained `thumbnail_dir: Option<String>` with `#[serde(rename = "thumbnailDir", skip_serializing_if = "Option::is_none")]` (the struct has no blanket `rename_all`, so this had to be spelled out explicitly to match Swift's camelCase field). Added two tests pinning the exact wire spelling and omission-when-absent.
- **`sidecar.rs`**: threaded `thumbnail_dir` through `Sidecar::request` → `Sidecar::analyze` → `SidecarPool::analyze_all`.
- **`commands.rs::analyze_folder`**: computes `app.path().app_data_dir()?.join("thumbnails")`, creates it with `create_dir_all`, and passes the path string into `analyze_all`. `thumbnailPath` flows through `finalize_photos` untouched (it operates generically on `serde_json::Value` and only strips `phash`).
- **`Cargo.toml`**: `tauri` feature `protocol-asset` added (required to compile with `assetProtocol` enabled in config; the Tauri CLI auto-added this to `Cargo.toml`/`Cargo.lock` on the first `tauri dev` run against the new config — kept as a necessary, not accidental, change).

### 3. Tauri config (`src-tauri/tauri.conf.json`)

```json
"security": {
  "csp": "default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:",
  "assetProtocol": { "enable": true, "scope": ["$APPDATA/thumbnails/*"] }
}
```
`csp` was `null` before; needed a real policy that allows `asset:`/`http://asset.localhost` in `img-src` while keeping everything else locked to `'self'`.

### 4. Frontend

- **`app/types/features.ts`**: `AnalyzedPhoto.thumbnailPath: string | null` (matches the existing convention used for `smileFraction`, which is also nullable/omittable on the wire).
- **`app/pages/index.vue`**: added a plain `<img>` cell rendered via a `#thumbnail-cell` slot, `src` from `convertFileSrc(row.original.thumbnailPath)`. No restyling — same table, one new column, `width="60" height="60"` and nothing else.
- **Unrelated but necessary fix**: `index.vue`'s `UTable` usage predates the installed `@nuxt/ui@4.10.0` — it used `:rows`/`{key,label}` columns, which is the removed v2 API. The installed component's `defineProps` has no `rows` prop at all (confirmed by reading `node_modules/@nuxt/ui/dist/runtime/components/Table.vue`), so the table rendered **zero rows regardless of data** before this fix. Changed to `:data` + `{accessorKey, header}` (TanStack column defs), which is what the installed version actually consumes. This was required to verify anything renders at all, not a style change.
- **`tests/features.test.ts`**: added `thumbnailPath: null` to the local `photo()` factory so it still satisfies the type.

## Tests written

`sidecar/Tests/PhotobookEngineTests/ThumbnailWriterTests.swift` (3 tests):
- `thumbnailWriterWritesJpegAtHashKeyedPathWithinLongEdgeBudget` — writes, then decodes the result and asserts `width == 400`, `height < width`, path is `<dir>/<hash>.jpg`.
- `thumbnailWriterDoesNotRewriteAnExistingThumbnail` — pre-seeds the target path with non-JPEG sentinel bytes, calls `write`, asserts the sentinel is untouched.
- `thumbnailWriterThrowsWhenTargetDirectoryCannotBeCreated` — points `directory` at a path that's already a regular file, asserts `write` throws.

`sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift` — one new test, `analyzerThumbnailWriting`, covering three cases via three *sequential* `Analyzer.analyze` calls in a single `@Test` function (see "concurrency note" below):
1. `thumbnailDir` provided → `thumbnailPath` is set, file exists, keyed by content hash.
2. No `thumbnailDir` → `thumbnailPath` stays `nil` (regression guard against unconditional writing).
3. Write fails (blocker-file-as-directory trick) → record stays `.ok` with `thumbnailPath == nil` and the rest of the fields (`width`, `height`) are intact — not degraded to `.failed`.

**Concurrency note (why one test function, not three):** swift-testing schedules `@Test` functions concurrently by default. Each case here drives a real Vision pass (`Analyzer.analyzeOne` always calls `VisionAnalyzer.analyze`, thumbnail work or not). Three separate `@Test` functions reproducibly starved Vision's internal `VNControlledCapacityTasksQueue` during development — confirmed via `sample`, same failure mode as the documented 300-photo regression test in this same file (`analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`), because `VisionAnalyzerTests.swift`'s tests call `VisionAnalyzer.analyze` directly, bypassing `Analyzer`'s `visionSemaphore` throttle entirely. Combining the three cases into one function with sequential calls removed the added concurrent pressure and the suite passed reliably (71/71) on repeated runs.

### Mutation evidence (required: the degrade-gracefully case)

Temporarily changed the catch-and-nil logic in `Analyzer.swift` to rethrow instead:
```swift
let thumbnailPath: String? = try thumbnailDir.flatMap { dir in
    try ThumbnailWriter.write(image: image, hash: hash, directory: dir, maxPixel: thumbnailMaxPixel)
}
```
Ran `swift test --package-path sidecar --filter analyzerThumbnailWriting`:
```
✘ Test analyzerThumbnailWriting() recorded an issue at AnalyzerTests.swift:208:21
↳ a thumbnail write failure must not turn the record into .failed
✘ Test run with 1 test in 0 suites failed after 0.245 seconds with 1 issue.
```
Confirmed the mutant is caught, then reverted. Re-ran the full suite: 71/71 pass again.

## Commands run, actual output

```
$ swift test --package-path sidecar
✔ Test run with 71 tests in 1 suite passed after 7.711 seconds.

$ bash scripts/build-sidecar.sh && cargo test --manifest-path src-tauri/Cargo.toml
test result: ok. 61 passed; 0 failed  (lib)
test result: ok. 1 passed; 0 failed   (tests/sidecar_ping.rs, spawns the real built binary)

$ bun run test          # vitest
Test Files  3 passed (3)
Tests       412 passed (412)

$ bun run lint           # oxlint --deny-warnings
(no output, exit 0)
```

Manual sidecar wire check (`swift build -c release` then piping a request directly):
```
$ echo '{"id":"1","kind":"analyze","paths":["sidecar/Fixtures/landscape.jpg"],"thumbnailDir":"/tmp/pbg-thumb-test"}' | ./sidecar/.build/release/PhotobookEngine
{"id":"1","result":{"type":"analyzed","data":[{"status":"ok","features":{...
  "thumbnailPath":"/tmp/pbg-thumb-test/08c8f73e....jpg", ...}}]}}
$ file /tmp/pbg-thumb-test/08c8f73e....jpg
JPEG image data, ..., baseline, precision 8, 400x267, components 3
```
400px long edge, correctly hash-keyed, confirmed via `file`/`sips`.

## What was verified in the running app

Launched via `bun run dev` (real Tauri window, real Swift sidecar binary, real webview). Screenshots captured with `screencapture -l<windowID>` (app window only, via a small Swift/CoreGraphics helper to resolve the window ID — no Accessibility permission was available in this sandbox to click the native folder-picker button or open dev tools, so verification used two workarounds, both temporary and fully reverted):

1. Temporarily bypassed the native folder dialog (hardcoded a folder path in `useAnalysis.ts` + an `onMounted` auto-trigger in `index.vue`) to drive `analyze_folder` without a click. **Result: a real, unmodified, pre-existing bug was found this way** (see below) — the live IPC round-trip through `tauri_plugin_shell` timed out.
2. To isolate and verify specifically the piece this task owns — `convertFileSrc` + the asset-protocol scope + the CSP — bypassed the slow IPC and injected a fake `AnalysisSummary` (via a temporary `onMounted` in `index.vue`) pointing `thumbnailPath` at a real, already-on-disk thumbnail file at the real `$APPDATA/thumbnails/<hash>.jpg` path (produced by an earlier successful sidecar run against the same fixture). Screenshot confirms the `<img>` renders real pixel content (a solid blue rectangle matching `landscape.jpg`'s actual computed palette, not a broken-image icon, no CSP console-blocked load). Screenshot saved to the scratchpad as `thumbnail-verification-screenshot.png`.

All temporary changes (debug `eprintln!`s in `sidecar.rs`, the widened timeout, the hardcoded folder, the `onMounted` auto-triggers, the fake summary) were reverted; `git diff --stat` and `git status` were checked afterward to confirm only the intended files changed. Full test suites were re-run clean after reverting (see above).

## Significant finding: pre-existing bug in the live sidecar IPC (not fixed, out of scope, documented for follow-up)

While driving `analyze_folder` through the real app for verification, every real invocation failed with `sidecar timed out after {N}s` on both retry attempts, even though the Swift sidecar was demonstrably completing successfully (thumbnails were written to `$APPDATA/thumbnails/` as a side effect of successful analysis).

Root-caused with timestamped `eprintln!` tracing (temporary, reverted): `Sidecar::request()` does a **synchronous, blocking** `self.lines.recv_timeout(timeout)` call from inside an `async fn` Tauri command, on the same `tauri::async_runtime` (tokio) worker-thread pool that the sidecar's own stdout-draining task (`tauri::async_runtime::spawn` in `Sidecar::spawn`) needs to be scheduled on. On this sandboxed environment (likely few available cores), the blocking wait starves the drain task of a worker thread, so the drain task can't run — and can't hand the already-arrived response back to `recv_timeout` — until the blocking wait's *own* timeout elapses and frees the thread. Evidence: the response was measured arriving at `t ≈ configured_timeout` regardless of what the timeout was set to (13.10s for a 13s timeout, 63.01s for a 63s timeout) — the real Vision-analysis time is unrelated to this delay; it's the timeout duration itself that determines when the thread frees up.

This is pre-existing code (`Sidecar::spawn`/`request`, unchanged by this task except for adding the `thumbnail_dir` parameter) that appears to have never been exercised end-to-end against a real `AppHandle` — `sidecar_ping.rs`'s integration test spawns the binary directly via `std::process::Command`, bypassing `tauri_plugin_shell` entirely, and `SidecarPool::analyze_all`/`Sidecar::analyze` are explicitly called out in comments as "not unit tested." I did not fix this — it's a distinct, systemic threading issue (likely needs the blocking wait moved onto `spawn_blocking` or the whole request path rewritten as async) outside this task's scope, and touching it risked destabilizing something with no test coverage of its own. Flagging it here since it will block real usage of "Choose photo folder" on machines/environments with limited worker threads.

## Uncertain / worth a second look

- The CSP (`default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:`) was not exercised against a production (`tauri build`) bundle, only `tauri dev` — and even then, only indirectly (the fake-summary image load succeeded, which does exercise `img-src`, but `script-src`/`connect-src` correctness for the real IPC path couldn't be confirmed given the timeout bug above blocked a real end-to-end run).
- The IPC timeout bug above means I could not get a genuine, non-workaround screenshot of "click button → real analysis → real thumbnails appear." The screenshot proves the rendering half of the pipeline; the orchestration half is proven only by unit/integration tests plus the manual CLI wire check.
- `$APPDATA/thumbnails/*` as the asset-protocol scope glob was verified to match a real file one level deep (flat `<hash>.jpg` layout, no nesting) — correct for how `ThumbnailWriter` actually lays files out.
