# Task 9 Report: Wire the analyze request end-to-end in Swift

Status: DONE
Branch: feat/phase-1-analysis-pipeline
Commit: `1d320bd` — feat(sidecar): add concurrent analyze pipeline with per-photo failure records

## What was implemented

- `sidecar/Sources/PhotobookEngine/Analyzer.swift` (new): `PhotoFeatures`, `PhotoRecord` (hand-rolled
  `Codable` enum with `{"status": "ok"|"failed", ...}` envelope), and `Analyzer` with:
  - `analysisMaxPixel = 1536`
  - `contentHash(path:)` — SHA-256 over raw file bytes (CryptoKit), streamed in 1MB chunks
  - `analyzeOne(path:) throws -> PhotoFeatures` — reads EXIF, hashes the file, decodes a thumbnail,
    runs Vision + classical metrics inside `autoreleasepool { ... }`
  - `analyze(paths:) -> [PhotoRecord]` — `DispatchQueue.concurrentPerform`, writes into a
    pre-sized `[PhotoRecord?]` by index under `NSLock`, returns `results.compactMap { $0 }`
- `sidecar/Sources/PhotobookEngine/Protocol.swift`: added `ResponseResult.analyzed([PhotoRecord])`
  with encode/decode arms (`"analyzed"` type tag).
- `sidecar/Sources/PhotobookEngine/Handler.swift`: replaced the `.analyze` arm's
  `"not implemented"` error with `Response(id: request.id, result: .analyzed(Analyzer.analyze(paths: request.paths ?? [])))`.
- `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift` (new, 4 tests, all prefixed `analyzer*`
  per the module-wide unique-name rule).

## Deviation from the brief (and why)

**Brief said edit `main.swift`; I edited `Handler.swift` instead.** The brief was written before a
prior task refactored dispatch logic out of `main.swift` into `Handler.swift`'s `handle(line:)`
function — the task prompt itself flagged this ("`Handler.swift` owns NDJSON line handling;
`main.swift` is just the stdin loop"). `main.swift` is now only a 6-line stdin read loop with no
`switch request.kind`; the `.analyze` case the brief wants replaced actually lives in
`Handler.swift`. Confirmed by reading both files before editing. The change is behaviorally
identical to what the brief specified, just in the file that now actually contains that logic.

**Brief's `analysesAGoodPhoto` test asserted `f.phash != 0`; I changed this assertion.** The task
prompt's own context section warns the fixtures are "flat colour fields, no faces... Assert the
contract... not photographic content." I verified this directly:

```
$ sips -g all sidecar/Fixtures/landscape.jpg
  pixelWidth: 1200
  pixelHeight: 800
  ... (uniform sRGB fill, no texture)
```

`Metrics.perceptualHash` computes an 8x8 average hash and sets a bit only where a cell's value is
*strictly greater* than the mean of all 64 cells. On a genuinely flat field every cell equals the
mean exactly, so no bit is set and the hash is legitimately `0` — this is correct behavior, not a
bug, and documented as such in `Metrics.swift`'s own doc comment. Running the brief's literal test
first confirmed this failure mode (see below), so I replaced the assertion with range/contract
checks: hash is a 64-char lowercase hex SHA-256, sharpness/warmth/contrast are in their valid
ranges, palette is non-empty, and (since the fixture has no faces) `faces` is empty, `faceAreaFraction
== 0`, `smileFraction == nil`. Test names: `analyzerAnalysesAGoodPhoto`,
`analyzerReportsFailureForMissingFileWithoutCrashing`, `analyzerOneBadPhotoDoesNotAbortTheBatch`,
`analyzerPreservesInputOrderUnderConcurrency`.

No other deviations. The `FaceObservation.outerLips` shape change required no adjustment — `vision.faces`
is passed through wholesale to `PhotoFeatures.faces` and to `SmileProxy.fraction(faces:threshold:)`
exactly as the brief specified.

## Commands run, with actual output

### Step 2: confirm the test fails before implementation

```
$ swift test --package-path sidecar
...
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift:12:19: error: cannot find 'Analyzer' in scope
error: fatalError
```//
(4 such "cannot find 'Analyzer' in scope" errors, one per test function — matches the brief's
expected failure.)

### First full run after implementation (found the phash issue)

```
$ swift test --package-path sidecar
...
✘ Test analyzerAnalysesAGoodPhoto() recorded an issue at AnalyzerTests.swift:20:5: Expectation failed: (f.phash → 0) != 0
✘ Test analyzerAnalysesAGoodPhoto() failed after 0.388 seconds with 1 issue.
✔ Test analyzerOneBadPhotoDoesNotAbortTheBatch() passed after 0.395 seconds.
✔ Test analyzerPreservesInputOrderUnderConcurrency() passed after 0.428 seconds.
✔ Test run with 53 tests in 0 suites failed after 3.150 seconds with 1 issue.
```

Confirmed this was the flat-fixture issue described above (not a bug in `Metrics` or `Analyzer`),
fixed the assertion, reran:

### Step 4: full suite green

```
$ swift test --package-path sidecar
...
✔ Test analyzerOneBadPhotoDoesNotAbortTheBatch() passed after 0.341 seconds.
✔ Test analyzerAnalysesAGoodPhoto() passed after 0.348 seconds.
✔ Test analyzerPreservesInputOrderUnderConcurrency() passed after 0.381 seconds.
✔ Test metricsHashHammingDistanceModelsRealisticBurstVariation() passed after 3.184 seconds.
✔ Test run with 53 tests in 0 suites passed after 3.185 seconds.
```

53/53 pass (49 pre-existing + 4 new).

### Release build

```
$ swift build --package-path sidecar -c release
...
warning: mutation of captured var 'results' in concurrently-executing code [#SendableClosureCaptures]
    (at Analyzer.swift:128, `results[index] = record`)
Build complete! (3.46s)
```

This is a Swift 6 strict-concurrency warning, not an error — the mutation is correctly serialized by
`NSLock` around the array write, exactly as the brief specifies. Left as-is since the brief's code
is verbatim here and the task explicitly scopes out anything beyond wiring this pipeline (no
`Sendable`-conformance rework requested). Flagging for awareness, not blocking.

### End-to-end pipe check (brief's Step 4)

```
$ echo '{"id":"1","kind":"analyze","paths":["sidecar/Fixtures/landscape.jpg"]}' \
    | ./sidecar/.build/release/PhotobookEngine
{"result":{"data":[{"features":{"path":"sidecar\/Fixtures\/landscape.jpg","hash":"08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159", ...,"width":1200,"height":800, ...},"status":"ok"}],"type":"analyzed"},"id":"1"}
```

Contains `"status":"ok"` and `"width":1200` as required.

Extra multi-path check (order preservation + failure isolation + hash correctness, run by hand
against the release binary):

```
$ printf '{"id":"2","kind":"analyze","paths":["/nonexistent/nope.jpg","sidecar/Fixtures/landscape.jpg","sidecar/Fixtures/portrait-rot90.jpg"]}\n{"id":"3","kind":"ping"}\n' \
    | ./sidecar/.build/release/PhotobookEngine
{"result":{"data":[
  {"message":"unreadable(\"/nonexistent/nope.jpg\")","path":"/nonexistent/nope.jpg","status":"failed"},
  {"features":{...,"width":1200,"height":800,"hash":"08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159",...},"status":"ok"},
  {"features":{...,"width":800,"height":1200,"hash":"b6271290dbc71c0e4da48de24655ef50531e2a4cdc89d888c4955be18851f632",...},"status":"ok"}
],"type":"analyzed"},"id":"2"}
{"result":{"data":{"version":"0.1.0"},"type":"pong"},"id":"3"}
```

Confirms: order preserved (failed, ok, ok matching input order), portrait EXIF orientation
correctly applied (800x1200 post-orientation, not 1200x800), and the sidecar still answers `ping`
correctly after handling `analyze` — no crash, no dangling state.

Independently verified the content hash matches raw file bytes (not decoded pixels), which Task 15's
Rust cache check depends on:

```
$ shasum -a 256 sidecar/Fixtures/landscape.jpg
08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159  sidecar/Fixtures/landscape.jpg
```

Matches the sidecar's reported `hash` exactly.

## Mutation-check evidence (order-preservation test)

Per the task's instruction to verify the order test genuinely exercises the race rather than
passing coincidentally: temporarily replaced the by-index write in `analyze(paths:)` with an
append-on-completion pattern (`var results = [PhotoRecord]()`; `results.append(record)` under the
same lock instead of `results[index] = record`), keeping everything else identical.

```swift
// MUTATION-CHECK: intentionally broken to append in completion order
// instead of writing by index...
var results = [PhotoRecord]()
...
lock.lock()
results.append(record)
lock.unlock()
...
return results   // was: results.compactMap { $0 }
```

Ran the order test 3 times against this broken version:

```
$ swift test --package-path sidecar --filter analyzerPreservesInputOrderUnderConcurrency   (x3)
✘ Test analyzerPreservesInputOrderUnderConcurrency() recorded an issue at AnalyzerTests.swift:70:56: Issue recorded
  ↳ last must be the failure
✘ Test analyzerPreservesInputOrderUnderConcurrency() failed after 0.235 seconds with 1 issue.
```

Failed all 3/3 runs — reliably, not flakily. This is not a coincidence of identical fixtures: the
test deliberately puts the missing-file failure *last* in input order while it's actually the
*fastest* task (fails on file-open before any Vision work), so under an append-order
implementation it lands near the front of the results array almost every time, while the 8 real
photos each pay for a full Vision pass (aesthetics, face detection, saliency, horizon,
classification, text recognition) and finish later. This asymmetry — not sheer thread-count luck —
is what makes the race manifest reliably.

Reverted to the correct by-index implementation (`diff` confirmed byte-identical to the version
before mutation), reran the full suite: 53/53 pass again. Then committed the correct version.

## Uncertain / worth a second look

- The `#SendableClosureCaptures` warning at build time (noted above) is cosmetic under the current
  Swift 6 concurrency-checking mode for this target, but could become a hard error if the package's
  concurrency strictness setting changes later. Not fixed here since it's outside this task's scope
  and the code matches the brief exactly.
- `ExifReader.read` is called even for the missing-file case and throws first (via
  `CGImageSourceCreateWithURL` failing), so `.failed` messages for missing files read as
  `unreadable("/path")` — this comes from `ExifReader.ReadError`, not `ImageLoader.LoadError`, since
  EXIF is read before the thumbnail decode in `analyzeOne`. This is fine for Task 9's contract (one
  record, `.failed`, with `path` correctly set) but Task 10's hostile-input corpus may want to check
  which specific error message shows up for different failure modes (corrupt vs. missing vs.
  zero-byte file) — worth keeping in mind there.

## Post-review fixes

Review of the initial submission returned one Critical finding (a defect in the brief, not in my
disclosed deviations, both of which were accepted as-is) and one Minor. Both addressed below.

### Finding 1 (Critical): `autoreleasepool` wrapped the wrong span

**Problem.** The brief's own code (and my initial implementation, copied verbatim) opened the pool
*after* `ImageLoader.loadThumbnail` had already run:

```swift
let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)  // outside the pool
return autoreleasepool {
    let vision = VisionAnalyzer.analyze(image)   // only this + Metrics were inside
    ...
}
```

`loadThumbnail` passes `kCGImageSourceShouldCacheImmediately: true` to
`CGImageSourceCreateThumbnailAtIndex`, which forces the full JPEG/HEIC decompression, colour
management, and downsample to happen synchronously at that call — the single largest allocation
site in the per-photo pipeline, larger than the Vision pass or metrics that follow. Since it ran
outside the pool, and `concurrentPerform` worker threads carry no autorelease pool of their own,
those decode allocations would accumulate unbounded across a large (e.g. 300-photo) batch —
exactly the failure mode `autoreleasepool` exists to prevent.

**Fix.** Moved the pool boundary to open right after `contentHash` (which is pure file I/O, no CG
allocations) and before `loadThumbnail`, so it now encloses the decode, the Vision pass, and the
metrics together:

```swift
return try autoreleasepool {
    let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)
    let vision = VisionAnalyzer.analyze(image)
    let palette = Metrics.palette(image, count: 6)
    ...
}
```

`autoreleasepool` rethrows, so `try` moved from the `loadThumbnail` call site to the `autoreleasepool`
call itself (`return try autoreleasepool { ... }`), and `loadThumbnail`'s own `try` now lives inside
the closure. Added a comment at the call site explaining why the pool must start there and not
later. File: `sidecar/Sources/PhotobookEngine/Analyzer.swift`.

I could not write an automated regression test for pool placement itself (autorelease pool timing
isn't observable from Swift Testing without attaching a memory profiler mid-run), so this is
verified by code inspection plus the existing behavioral tests confirming no regression — see
command output below.

### Finding 2 (Minor): no automated test for the `PhotoRecord`/`ResponseResult.analyzed` JSON contract

Added 6 tests to `AnalyzerTests.swift` under a `// MARK: - JSON contract with Rust` section:

- `photoRecordEncodesOkEnvelope` — encodes `.ok`, asserts `status == "ok"`, `features.path`/`width`/`phash`
  match, and the failure-only keys (`path`, `message`) are absent from the top level.
- `photoRecordEncodesFailedEnvelope` — encodes `.failed`, asserts `status == "failed"`, `path`,
  `message` match, and `features` is absent.
- `photoRecordRoundTripsOkThroughJSON` / `photoRecordRoundTripsFailedThroughJSON` — encode then
  decode back through `PhotoRecord`, assert the reconstructed case and its fields match the original.
- `responseResultAnalyzedEncodesTypeAndDataEnvelope` — encodes a `Response` with `.analyzed([...])`,
  asserts the envelope is exactly `{"type":"analyzed","data":[...]}` (what Rust's
  `#[serde(tag = "type", content = "data")]` requires), with two records of different status inside.
- `responseResultAnalyzedRoundTripsThroughJSON` — full `Response` round-trip through `.analyzed`.

Also added the `sampleFeatures()` helper and a `decodedJSONObject(_:)` helper (via
`JSONSerialization` + `#require`) used by the envelope-shape assertions.

### Comment on the `#SendableClosureCaptures` warning (no code change requested, one comment added)

Per the reviewer's note that this is genuinely cosmetic (every write to `results` is behind
`NSLock`, and `concurrentPerform` blocks the calling thread until all iterations finish before the
final read, giving a real happens-before edge), I added a comment at the lock/`concurrentPerform`
call site in `Analyzer.analyze` explaining why the pattern is sound today and that it would need
`Mutex` (or an `@unchecked Sendable` wrapper) if the package ever opts into full Swift 6 language
mode. No behavioral change.

### Commands run, with actual output

Full suite after both fixes:

```
$ swift test --package-path sidecar
...
✔ Test analyzerOneBadPhotoDoesNotAbortTheBatch() passed after 0.355 seconds.
✔ Test analyzerAnalysesAGoodPhoto() passed after 0.359 seconds.
✔ Test analyzerPreservesInputOrderUnderConcurrency() passed after 0.391 seconds.
✔ Test metricsHashHammingDistanceModelsRealisticBurstVariation() passed after 3.156 seconds.
✔ Test run with 59 tests in 0 suites passed after 3.157 seconds.
```

59/59 pass (53 from the first submission + 6 new JSON-contract tests). All 6 new tests individually
listed as passing in the run: `photoRecordEncodesOkEnvelope`, `photoRecordEncodesFailedEnvelope`,
`photoRecordRoundTripsOkThroughJSON`, `photoRecordRoundTripsFailedThroughJSON`,
`responseResultAnalyzedEncodesTypeAndDataEnvelope`, `responseResultAnalyzedRoundTripsThroughJSON`.

Release rebuild and pipe check, to confirm the `autoreleasepool` fix didn't change observable
behavior:

```
$ swift build --package-path sidecar -c release
...
warning: mutation of captured var 'results' in concurrently-executing code [#SendableClosureCaptures]
    (same line/warning as before — expected, now with explanatory comment)
Build complete! (1.76s)

$ echo '{"id":"1","kind":"analyze","paths":["sidecar/Fixtures/landscape.jpg"]}' \
    | ./sidecar/.build/release/PhotobookEngine
{"result":{"data":[{"status":"ok","features":{"hash":"08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159","phash":0,...,"width":1200,"height":800,...}}],"type":"analyzed"},"id":"1"}
```

Identical shape and values to the pre-fix run (hash, width, height, phash all match) — confirms the
`autoreleasepool` span change is purely a memory-lifetime fix with no effect on computed results.

### Commit

`177af08` — fix(sidecar): widen autoreleasepool to cover thumbnail decode, add PhotoRecord JSON contract tests
