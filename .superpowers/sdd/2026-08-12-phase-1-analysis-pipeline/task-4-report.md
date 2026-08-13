# Task 4 Report: ImageIO decode with orientation

## Summary

Implemented `ImageLoader.loadThumbnail(path:maxPixel:) throws -> CGImage` in
`sidecar/Sources/PhotobookEngine/ImageLoader.swift`, decoding a photo thumbnail via
ImageIO with EXIF orientation applied. Followed TDD exactly as specified in the brief:
wrote the failing test, confirmed it failed for the right reason (missing symbol),
implemented, then — because the brief specifically called out the risk of a test that
"passes for the wrong reason" — deliberately implemented once *without*
`kCGImageSourceCreateThumbnailWithTransform` first and reran to confirm the orientation
test fails exactly as predicted, before adding the option back and confirming all tests
pass.

## Files created

- `scripts/make-fixtures.swift` — generates deterministic JPEG fixtures via ImageIO
  (no ImageMagick/exiftool dependency).
- `sidecar/Fixtures/landscape.jpg` — 1200×800, orientation 1. Committed (small, deterministic).
- `sidecar/Fixtures/portrait-rot90.jpg` — 1200×800 stored, orientation 6. Committed.
- `sidecar/Sources/PhotobookEngine/ImageLoader.swift` — the implementation.
- `sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift` — the test file, copied
  verbatim from the brief.

No deviations from the brief. The implementation matches Step 3 of the brief verbatim,
including all four options in the thumbnail options dictionary
(`kCGImageSourceCreateThumbnailFromImageAlways`, `kCGImageSourceCreateThumbnailWithTransform`,
`kCGImageSourceThumbnailMaxPixelSize`, `kCGImageSourceShouldCacheImmediately`).

## Step 1: Write the failing test

Created `sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift` verbatim from the brief.

## Step 2: Run test to verify it fails

```
$ swift test --package-path sidecar
```

Output (truncated to the relevant errors):

```
[4/6] Compiling PhotobookEngineTests ImageLoaderTests.swift
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift:15:19: error: cannot find 'ImageLoader' in scope
13 |
14 | @Test func loadsLandscapeAtRequestedSize() throws {
15 |     let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 512)
   |                   `- error: cannot find 'ImageLoader' in scope
...
error: fatalError
```

Confirmed: fails to compile because `ImageLoader` does not exist yet, as expected.

## Fixture generation

Created `scripts/make-fixtures.swift` verbatim from the brief (ImageIO-only, no
ImageMagick/exiftool — neither is installed on this machine, per the task context).

```
$ swift scripts/make-fixtures.swift
wrote landscape.jpg (1200x800, orientation 1)
wrote portrait-rot90.jpg (1200x800, orientation 6)
```

```
$ ls -la sidecar/Fixtures/
-rw-r--r--@ 1 jiajingteoh  staff  16115 Aug 12 17:34 landscape.jpg
-rw-r--r--@ 1 jiajingteoh  staff  16115 Aug 12 17:34 portrait-rot90.jpg
```

## Step 3: Write minimal implementation, with a deliberate "prove the test would fail" check

Before implementing exactly as the brief specifies, I first wrote the implementation
**without** `kCGImageSourceCreateThumbnailWithTransform` (all other options present) and
ran only the orientation test, to verify the test is actually exercising the behavior it
claims to check — not just passing by accident:

```
$ swift test --package-path sidecar --filter appliesExifOrientationSoPortraitIsTaller
...
◇ Test appliesExifOrientationSoPortraitIsTaller() started.
✘ Test appliesExifOrientationSoPortraitIsTaller() recorded an issue at ImageLoaderTests.swift:23:5: Expectation failed: (img.height → 341) > (img.width → 512)
↳ orientation must be applied before we report dimensions
✘ Test appliesExifOrientationSoPortraitIsTaller() failed after 0.006 seconds with 1 issue.
✘ Test run with 1 test in 0 suites failed after 0.006 seconds with 1 issue.
```

Confirmed: without the transform option, ImageIO reports the raw stored 1200×800 pixel
buffer scaled to a 512-long-edge thumbnail (512×341 — landscape), ignoring the
orientation-6 tag. This is exactly the silent-wrong-dimensions failure mode the task
brief warns about. The test correctly catches it.

I then added `kCGImageSourceCreateThumbnailWithTransform: true` to the options
dictionary (final implementation, matching the brief verbatim) and reran full suite.

## Step 4: Run tests to verify they pass

```
$ swift test --package-path sidecar
...
◇ Test envelopeDecodesGarbageKind() started.
◇ Test throwsOnMissingFile() started.
◇ Test appliesExifOrientationSoPortraitIsTaller() started.
◇ Test encodesErrorResponse() started.
◇ Test encodesPongResponseWithMatchingId() started.
◇ Test loadsLandscapeAtRequestedSize() started.
◇ Test decodesPingRequest() started.
◇ Test unknownKindEchoesRequestId() started.
◇ Test invalidJsonProducesUnknownId() started.
✔ Test envelopeDecodesGarbageKind() passed after 0.001 seconds.
✔ Test decodesPingRequest() passed after 0.001 seconds.
✔ Test invalidJsonProducesUnknownId() passed after 0.001 seconds.
✔ Test unknownKindEchoesRequestId() passed after 0.001 seconds.
✔ Test encodesErrorResponse() passed after 0.001 seconds.
✔ Test encodesPongResponseWithMatchingId() passed after 0.001 seconds.
✔ Test throwsOnMissingFile() passed after 0.001 seconds.
✔ Test loadsLandscapeAtRequestedSize() passed after 0.005 seconds.
✔ Test appliesExifOrientationSoPortraitIsTaller() passed after 0.006 seconds.
✔ Test run with 9 tests in 0 suites passed after 0.006 seconds.
```

All 9 tests pass: the 3 new `ImageLoaderTests` plus the 6 pre-existing `ProtocolTests`.
No regressions.

## Step 5: Commit

```
$ git add sidecar scripts/make-fixtures.swift
$ git commit -m "feat(sidecar): add ImageIO thumbnail decode with orientation applied"
[feat/phase-1-analysis-pipeline 6129af0] feat(sidecar): add ImageIO thumbnail decode with orientation applied
 5 files changed, 93 insertions(+)
 create mode 100644 scripts/make-fixtures.swift
 create mode 100644 sidecar/Fixtures/landscape.jpg
 create mode 100644 sidecar/Fixtures/portrait-rot90.jpg
 create mode 100644 sidecar/Sources/PhotobookEngine/ImageLoader.swift
 create mode 100644 sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift
```

Commit SHA: `6129af0`

## Deviations

None. Implementation, test file, and fixture script match the brief verbatim.

## Uncertainties / notes for future tasks

- `sidecar/Fixtures/` fixtures are committed as instructed (small, deterministic,
  reproducible from a fresh clone via `swift scripts/make-fixtures.swift` if ever
  needed again — but the committed binaries mean the script doesn't need to be rerun
  in CI).
- `scripts/make-fixtures.swift` currently only writes to `sidecar/Fixtures/` at the top
  level. Task 10's planned `hostile/` subdirectory addition should be a straightforward
  append (new `write(...)` calls targeting `dir.appendingPathComponent("hostile/...")`
  or a second helper) — no restructuring needed.
- `LoadError` is a plain `enum` with associated `String` values (not `Equatable`), which
  is sufficient for `#expect(throws: ImageLoader.LoadError.self)` (type-based matching).
  If a later task needs to assert on the specific error case/message, `Equatable`
  conformance would need to be added — not required by this task's brief.
- Did not add any HEIC-specific fixtures in this task (out of scope — brief only asked
  for two JPEG fixtures); the `kCGImageSourceCreateThumbnailWithTransform` behavior for
  HEIC `irot`/`imir` reconciliation is implemented per the brief's requirement but only
  exercised indirectly (by including the option) — no HEIC fixture exists yet to test
  that path directly. This seems fine for Task 4's scope but is worth flagging in case
  a later task assumes HEIC coverage already exists.
