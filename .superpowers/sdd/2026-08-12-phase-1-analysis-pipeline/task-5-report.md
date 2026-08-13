# Task 5 Report: EXIF extraction

## What was implemented

- `sidecar/Sources/PhotobookEngine/ExifReader.swift` — `ExifData` (Codable struct) and `ExifReader.read(path:) throws -> ExifData`, implemented exactly per the brief's Step 3 code: reads properties via `CGImageSourceCopyPropertiesAtIndex` only (never `CGImageSourceCreateImageAtIndex` / `CGImageSourceCreateThumbnailAtIndex`), applies `swapsAxes` for orientations 5-8 to report post-orientation `pixelWidth`/`pixelHeight`, negates GPS latitude/longitude for `S`/`W` refs, parses `DateTimeOriginal` with a fixed `yyyy:MM:dd HH:mm:ss` UTC `DateFormatter`, and derives `flashFired` from bit 0 of the EXIF flash value.
- `sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift` — the brief's three tests, with one renamed (see Deviations).

## Commands run

### 1. Wrote failing test file, ran to confirm failure

```
$ swift test --package-path sidecar
```

First attempt (test function named `throwsOnMissingFile`, colliding with the existing one in `ImageLoaderTests.swift`) failed with a **redeclaration** error, not the expected "cannot find ExifReader" error:

```
error: emit-module command failed with exit code 1 (use -v to see invocation)
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift:24:12: error: invalid redeclaration of 'throwsOnMissingFile()'
```

Renamed the third test to `exifThrowsOnMissingFile()` (see Deviations below), reran:

```
$ swift test --package-path sidecar
...
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift:12:20: error: cannot find 'ExifReader' in scope
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift:19:20: error: cannot find 'ExifReader' in scope
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift:25:21: error: cannot find 'ExifReader' in scope
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift:26:13: error: cannot find 'ExifReader' in scope
error: fatalError
```

This is the expected failure mode from Step 2 of the brief.

### 2. Implemented `ExifReader.swift` (brief's Step 3, verbatim) and reran

```
$ swift test --package-path sidecar
...
Build complete! (0.96s)
◇ Test run started.
✔ Test encodesPongResponseWithMatchingId() passed after 0.001 seconds.
✔ Test invalidJsonProducesUnknownId() passed after 0.001 seconds.
✔ Test envelopeDecodesGarbageKind() passed after 0.001 seconds.
✔ Test encodesErrorResponse() passed after 0.001 seconds.
✔ Test decodesPingRequest() passed after 0.001 seconds.
✔ Test unknownKindEchoesRequestId() passed after 0.001 seconds.
✔ Test exifThrowsOnMissingFile() passed after 0.001 seconds.
✔ Test throwsOnMissingFile() passed after 0.001 seconds.
✔ Test readsDimensionsForLandscape() passed after 0.003 seconds.
✔ Test reportsPostOrientationDimensions() passed after 0.003 seconds.
✔ Test loadsLandscapeAtRequestedSize() passed after 0.005 seconds.
✔ Test appliesExifOrientationSoPortraitIsTaller() passed after 0.006 seconds.
✔ Test run with 12 tests in 0 suites passed after 0.006 seconds.
```

All 12 tests pass: 3 new (`readsDimensionsForLandscape`, `reportsPostOrientationDimensions`, `exifThrowsOnMissingFile`) plus the 9 pre-existing tests (unaffected).

### 3. Verified no pixel-decode APIs present

```
$ grep -n "CGImageSourceCreateImageAtIndex\|CGImageSourceCreateThumbnailAtIndex" sidecar/Sources/PhotobookEngine/ExifReader.swift
(no matches)
```

`ExifReader.swift` only calls `CGImageSourceCreateWithURL` and `CGImageSourceCopyPropertiesAtIndex` — properties only, as required.

### 4. Commit

```
$ git add sidecar/Sources/PhotobookEngine/ExifReader.swift sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift
$ git commit -m "feat(sidecar): add metadata-only EXIF reader with post-orientation dimensions"
[feat/phase-1-analysis-pipeline 735409c] feat(sidecar): add metadata-only EXIF reader with post-orientation dimensions
 2 files changed, 96 insertions(+)
 create mode 100644 sidecar/Sources/PhotobookEngine/ExifReader.swift
 create mode 100644 sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift
```

## Deviations from the brief

1. **Renamed the third test from `throwsOnMissingFile` to `exifThrowsOnMissingFile`.** The brief's test file, copied verbatim, declares a free function `throwsOnMissingFile()` — but `sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift` (from Task 4) already declares a top-level `@Test func throwsOnMissingFile()`. Swift Testing free functions live in the module's global namespace, so this is a genuine compile-time redeclaration conflict, not a false failure — confirmed by the first `swift test` run, which errored with "invalid redeclaration" before implementation even started (i.e., not the failure the brief's Step 2 expects). Renamed to `exifThrowsOnMissingFile()` to disambiguate; behavior and assertions are unchanged. No other content in either test file was altered.

Implementation code (`ExifReader.swift`) matches the brief's Step 3 exactly, no changes.

## Fields not exercised by tests (expected, per task instructions)

`captureDate`, `latitude`/`longitude`, `make`, `model`, `flashFired` are all `nil` for both fixtures (`landscape.jpg`, `portrait-rot90.jpg` carry no EXIF/GPS/TIFF data beyond dimensions+orientation). This matches the explicit instruction not to fabricate GPS-bearing fixtures; only dimensions and the missing-file error path are asserted.

## Uncertainties / things not independently verified

- I did not construct a real GPS-tagged or dated fixture to manually verify the `S`/`W` negation logic or the `DateTimeOriginal` parsing end-to-end — the brief explicitly says not to add such fixtures. This code path is implemented per the brief's exact logic but is untested by this task's suite (as intended; presumably covered later or trusted as reviewed logic from the brief).
- Did not check downstream Task 9 wiring (PhotobookEngine's `PhotoFeatures` struct doesn't exist yet, per instructions — out of scope).

## Status

DONE. No blockers encountered; the one deviation (test rename) was a pre-existing naming collision, not a design ambiguity in the EXIF logic itself.

---

## Fix report (post-review): locale pin + GPS/date test coverage

Review found two Important findings, both inherited from the brief's reference code:

1. `DateFormatter` had no explicit `locale`/`calendar`, so it inherited `Locale.autoupdatingCurrent` — on a non-Gregorian-calendar region (Thai Buddhist, Persian) or non-Latin-digit locale, `date(from:)` silently returns `nil` for well-formed EXIF, with `captureDate` just empty and nothing erroring.
2. GPS hemisphere negation and date parsing were only reviewed by reading the code — both fixtures carried no GPS/`DateTimeOriginal`, so those paths never executed under test.

### Fix 1: pin formatter locale/calendar

`sidecar/Sources/PhotobookEngine/ExifReader.swift`, in the `formatter` static property:

```swift
private static let formatter: DateFormatter = {
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.calendar = Calendar(identifier: .gregorian)
    f.dateFormat = "yyyy:MM:dd HH:mm:ss"
    f.timeZone = TimeZone(secondsFromGMT: 0)
    return f
}()
```

### Fix 2: new fixture + tests exercising GPS sign and date parsing

Extended `scripts/make-fixtures.swift` with `writeWithGpsAndDate(...)`, which writes `sidecar/Fixtures/gps-south-west.jpg` (1200x800, orientation 1) via `CGImageDestinationAddImage`'s properties dictionary with:
- `kCGImagePropertyGPSDictionary`: `GPSLatitude: 33.8688`, `GPSLatitudeRef: "S"`, `GPSLongitude: 151.2093`, `GPSLongitudeRef: "W"`
- `kCGImagePropertyExifDictionary`: `ExifDateTimeOriginal: "2024:06:15 10:30:00"`

Ran `swift scripts/make-fixtures.swift` from repo root. Confirmed the two pre-existing fixtures were byte-identical after regeneration (`md5` unchanged for `landscape.jpg`/`portrait-rot90.jpg`, since the generator is deterministic); only `gps-south-west.jpg` is new. Verified round-trip fidelity by reading the fixture's raw properties back with a scratch Swift script — ImageIO stored the GPS/EXIF values exactly as written (no rational-quantization drift), confirming the `33.8688`/`151.2093`/`"2024:06:15 10:30:00"` values used in test assertions are exact, not approximate.

Added to `sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift` (names prefixed `exif...` to avoid the free-function namespace collision risk flagged for Tasks 6-8):

- `exifReadsSouthernLatitudeAsNegative` — asserts `latitude < 0` and `abs(lat - (-33.8688)) < 0.0001`
- `exifReadsWesternLongitudeAsNegative` — asserts `longitude < 0` and `abs(lon - (-151.2093)) < 0.0001`
- `exifParsesCaptureDate` — asserts `captureDate!.timeIntervalSince1970 == 1718447400` (the known Unix timestamp for 2024-06-15T10:30:00Z, verified independently via `python3 -c "import datetime; print(datetime.datetime(2024,6,15,10,30,0,tzinfo=datetime.timezone.utc).timestamp())"` → `1718447400.0`), not a formatted-string comparison
- `exifFixturesWithoutGpsReportNilCoordinates` — asserts `landscape.jpg` and `portrait-rot90.jpg` still report `latitude == nil` and `longitude == nil`, so absence stays distinguishable from zero

### Command run and output (final, all fixes applied)

```
$ swift test --package-path sidecar
...
Build complete! (0.78s)
◇ Test run started.
✔ Test decodesPingRequest() passed after 0.001 seconds.
✔ Test invalidJsonProducesUnknownId() passed after 0.001 seconds.
✔ Test encodesErrorResponse() passed after 0.001 seconds.
✔ Test exifThrowsOnMissingFile() passed after 0.001 seconds.
✔ Test throwsOnMissingFile() passed after 0.001 seconds.
✔ Test envelopeDecodesGarbageKind() passed after 0.001 seconds.
✔ Test unknownKindEchoesRequestId() passed after 0.001 seconds.
✔ Test encodesPongResponseWithMatchingId() passed after 0.001 seconds.
✔ Test readsDimensionsForLandscape() passed after 0.004 seconds.
✔ Test reportsPostOrientationDimensions() passed after 0.004 seconds.
✔ Test exifFixturesWithoutGpsReportNilCoordinates() passed after 0.004 seconds.
✔ Test exifReadsSouthernLatitudeAsNegative() passed after 0.005 seconds.
✔ Test exifReadsWesternLongitudeAsNegative() passed after 0.005 seconds.
✔ Test exifParsesCaptureDate() passed after 0.005 seconds.
✔ Test loadsLandscapeAtRequestedSize() passed after 0.006 seconds.
✔ Test appliesExifOrientationSoPortraitIsTaller() passed after 0.006 seconds.
✔ Test run with 16 tests in 0 suites passed after 0.006 seconds.
```

16/16 tests pass (7 new EXIF tests total + 9 pre-existing).

### Mutation check: confirming the GPS tests genuinely catch a flipped sign

Per the review's requirement that a test which passes either way is worse than none, I temporarily inverted the negation conditions in `ExifReader.swift` (`ref == "S"` → `ref == "N"`, `ref == "W"` → `ref == "E"`) and reran just the two GPS tests:

```
$ swift test --package-path sidecar --filter "exifReadsSouthernLatitudeAsNegative|exifReadsWesternLongitudeAsNegative"
...
◇ Test exifReadsSouthernLatitudeAsNegative() started.
◇ Test exifReadsWesternLongitudeAsNegative() started.
✘ Test exifReadsSouthernLatitudeAsNegative() recorded an issue at ExifReaderTests.swift:33:5: Expectation failed: (lat → 33.8688) < (0 → 0.0)
✘ Test exifReadsWesternLongitudeAsNegative() recorded an issue at ExifReaderTests.swift:40:5: Expectation failed: (lon → 151.2093) < (0 → 0.0)
✘ Test exifReadsWesternLongitudeAsNegative() recorded an issue at ExifReaderTests.swift:41:5: Expectation failed: (abs(lon - (-151.2093)) → 302.4186) < 0.0001
✘ Test exifReadsSouthernLatitudeAsNegative() recorded an issue at ExifReaderTests.swift:34:5: Expectation failed: (abs(lat - (-33.8688)) → 67.7376) < 0.0001
✘ Test exifReadsWesternLongitudeAsNegative() failed after 0.006 seconds with 2 issues.
✘ Test exifReadsSouthernLatitudeAsNegative() failed after 0.006 seconds with 2 issues.
✘ Test run with 2 tests in 0 suites failed after 0.006 seconds with 4 issues.
```

Both tests fail as expected when the sign logic is broken. Restored the correct implementation (`git diff` against the pre-mutation copy confirmed byte-identical restoration) and reran the full suite to confirm all 16 pass again (output above).

### Commit

```
$ git add scripts/make-fixtures.swift sidecar/Sources/PhotobookEngine/ExifReader.swift sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift sidecar/Fixtures/gps-south-west.jpg
$ git commit -m "fix(sidecar): pin EXIF date formatter locale, add GPS/date test coverage"
[feat/phase-1-analysis-pipeline cc3b44a] fix(sidecar): pin EXIF date formatter locale, add GPS/date test coverage
 4 files changed, 72 insertions(+)
 create mode 100644 sidecar/Fixtures/gps-south-west.jpg
```

### Status

DONE. Both Important findings addressed; GPS-sign mutation check confirms the new tests are load-bearing, not decorative.
