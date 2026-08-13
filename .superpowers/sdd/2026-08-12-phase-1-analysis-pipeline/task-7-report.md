# Task 7 report: Vision analysis pass

## Addendum: fix for landmark coordinate-space bug found in review

**Status: DONE**

### The bug

Review found one Critical finding: `VNFaceLandmarkRegion2D.normalizedPoints` is
normalised relative to the **face's own bounding box**, not the image. The
original code only flipped `y` (`1.0 - $0.y`), which is a coherent flip
*within* bbox-relative space but never offsets or scales into the face's
position in the frame. The result: `landmarks` and `box` on the same
`FaceObservation` silently used two different coordinate spaces — `box`
image-normalised, `landmarks` face-box-normalised. Vision's own
`pointsInImageOfSize:` exists precisely because this conversion is non-trivial
(and failable); had `normalizedPoints` already been image-normalised, that
method would be a pure multiply that could never fail.

### The fix

- Added a new pure function, `VisionAnalyzer.imageNormalizedTopLeft(_:faceBoxInVisionSpace:)`,
  in `sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`:
  ```swift
  static func imageNormalizedTopLeft(_ point: CGPoint, faceBoxInVisionSpace box: CGRect) -> [Double] {
      let xImage = box.origin.x + point.x * box.width
      let yImageBottomLeft = box.origin.y + point.y * box.height
      let yImageTopLeft = 1.0 - yImageBottomLeft
      return [Double(xImage), Double(yImageTopLeft)]
  }
  ```
- The landmark-mapping call site now passes `face.boundingBox` — the raw,
  **Vision-space (bottom-left-origin)** face bounding box straight from
  `VNFaceObservation` — as `faceBoxInVisionSpace`, not the already-flipped
  `topLeft(face.boundingBox)` used for the `box` field. Mixing those two would
  have been an easy, invisible second bug (double-flipping the offset), so a
  doc comment on the function calls this out explicitly.
- Added a doc comment on `FaceObservation` stating `box` and `landmarks` are
  both image-normalised, top-left-origin, and explaining why (so this doesn't
  drift apart again).
- `imageNormalizedTopLeft` is `static func`, not `private`, for the same
  testability reason as `topLeft` — no fixture contains a face, so the only
  coverage possible is calling the function directly with hand-picked values.

### Tests added (`sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`)

Three new tests, each with expected values computed by hand in a comment
*before* being asserted (not derived from the implementation):

1. `visionLandmarkPointAtFaceBoxCentreMapsIntoImageSpace` — face box off-centre
   in the frame (origin `(0.5, 0.3)`, size `(0.2, 0.4)`), point at the box's own
   centre `(0.5, 0.5)`. Hand-computed expected: `x_img = 0.6`, `y_top = 0.5`.
2. `visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace` — same box, points at
   the face-relative corners `(0,0)` and `(1,1)`. Hand-computed expected:
   `(0,0) -> [0.5, 0.7]`, `(1,1) -> [0.7, 0.3]`.
3. `visionLandmarkConversionReducesToSimpleFlipForFullFrameFaceBox` — face box
   is the full frame (`origin (0,0)`, `size (1,1)`), point `(0.3, 0.8)`.
   Hand-computed expected: `[0.3, 0.2]`, i.e. the offset-and-scale correctly
   reduces to the plain `1 - y` flip in this degenerate case.

### Commands and actual output

Build after the fix:
```
$ swift build --package-path sidecar
Build complete! (0.73s)
```

Full suite after the fix (33 tests: 30 previous + 3 new):
```
$ swift test --package-path sidecar
...
✔ Test visionLandmarkPointAtFaceBoxCentreMapsIntoImageSpace() passed after 0.008 seconds.
✔ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() passed after 0.008 seconds.
✔ Test visionLandmarkConversionReducesToSimpleFlipForFullFrameFaceBox() passed after 0.008 seconds.
...
✔ Test run with 33 tests in 0 suites passed after 3.055 seconds.
```

### Mutation-check evidence

Mutated `imageNormalizedTopLeft` to drop the offset-and-scale, leaving only the
old (buggy) flip:
```swift
static func imageNormalizedTopLeft(_ point: CGPoint, faceBoxInVisionSpace box: CGRect) -> [Double] {
    // MUTATED: offset-and-scale into the face box dropped, only the flip remains.
    return [Double(point.x), Double(1.0 - point.y)]
}
```
Ran just the new tests:
```
$ swift test --package-path sidecar --filter visionLandmark
✔ Test visionLandmarkConversionReducesToSimpleFlipForFullFrameFaceBox() passed after 0.001 seconds.
✘ Test visionLandmarkPointAtFaceBoxCentreMapsIntoImageSpace() recorded an issue at VisionAnalyzerTests.swift:68:5:
  Expectation failed: (abs(result[0] - 0.6) → 0.09999999999999998) < (1e-9 → 1e-09)
✘ Test visionLandmarkPointAtFaceBoxCentreMapsIntoImageSpace() failed after 0.001 seconds with 1 issue.
✘ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() recorded an issue at VisionAnalyzerTests.swift:79:5:
  Expectation failed: (abs(bottomLeft[0] - 0.5) → 0.5) < (1e-9 → 1e-09)
✘ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() recorded an issue at VisionAnalyzerTests.swift:80:5:
  Expectation failed: (abs(bottomLeft[1] - 0.7) → 0.30000000000000004) < (1e-9 → 1e-09)
✘ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() recorded an issue at VisionAnalyzerTests.swift:83:5:
  Expectation failed: (abs(topRight[0] - 0.7) → 0.30000000000000004) < (1e-9 → 1e-09)
✘ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() recorded an issue at VisionAnalyzerTests.swift:84:5:
  Expectation failed: (abs(topRight[1] - 0.3) → 0.3) < (1e-9 → 1e-09)
✘ Test visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() failed after 0.001 seconds with 4 issues.
✘ Test run with 3 tests in 0 suites failed after 0.001 seconds with 5 issues.
```
The centre and corner tests genuinely fail with the bug reintroduced — they
would have caught this exact defect. The full-frame test still passes under
the mutation, as expected and documented in its comment: a full-frame box is
the one case where the buggy flip and the correct conversion coincide, so that
test exists only to document the reduction, not to catch this mutation.

Reverted the mutation and re-ran the full suite to confirm all 33 tests pass:
```
$ swift test --package-path sidecar
...
✔ Test run with 33 tests in 0 suites passed after 3.063 seconds.
```

### Consequence for Task 8 (smile proxy) — flagged as requested

Task 8's smile proxy computes a ratio of vertical mouth-corner lift to mouth
width from `FaceObservation.landmarks`. Before this fix, `landmarks` were
face-box-relative, so that ratio was already in the right space for
thresholds tuned against bbox-relative geometry — it would likely have worked
**by accident**. After this fix, `landmarks` are image-normalised (same space
as `box`), so a raw vertical-lift-over-width ratio computed directly from
`landmarks` is now scaled by the face box's aspect ratio and by the image's
own aspect ratio — it is no longer the bbox-relative quantity any proxy
thresholds would assume.

**Task 8 must re-normalise `landmarks` back into face-box-relative space using
`box` before computing any intra-face ratio** (the inverse of
`imageNormalizedTopLeft`: subtract `box`'s origin, divide by `box`'s width/
height, per axis — mind that `box` is already top-left-flipped, unlike the
Vision-space rect `imageNormalizedTopLeft` consumes). Do not skip this and
recalibrate thresholds against image-normalised coordinates instead — that
would silently break for any photo where a face is small relative to the
frame or the frame itself isn't square, i.e. almost always.

### Commit

`fd1bcee` — fix(sidecar): make face landmarks image-normalised to match box coordinate space


## Status: DONE

## What was implemented

- `sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`
  - `struct FaceObservation: Codable` — `box`, `yaw`, `pitch`, `roll`, `captureQuality`, `landmarks` exactly as specified.
  - `struct VisionResult: Codable` — `isUtility`, `aestheticScore`, `faces`, `saliencyBox`, `horizonTiltDeg`, `sceneTags`, `hasText` exactly as specified.
  - `enum VisionAnalyzer` with `static func analyze(_ image: CGImage) -> VisionResult`, matching the brief's implementation verbatim with two adjustments (see Deviations).
  - One `VNImageRequestHandler` is created and used for **all** requests: `perform([aesthetics, faces, saliency, horizon, classify, text])` in the first call, then `perform([landmarks, quality])` on the *same* handler in a second call, only when faces were detected.
  - `landmarks.inputFaceObservations` and `quality.inputFaceObservations` are both seeded from `faces.results` before the second `perform`, so results stay index-aligned with `detected` — no independent-array pairing.
  - `topLeft(_:)` converts every emitted box (face boxes, saliency box) from Vision's bottom-left-origin normalized rect to top-left-origin `[x, y, w, h]`.
  - `sceneTags` is filtered with `hasMinimumRecall(0.0, forPrecision: 0.8)`, not a flat confidence cutoff, per the precision-based filtering context given for this task.
  - Face landmark points are converted to `[[Double]]` via `landmarkResults[index].landmarks?.allPoints?.normalizedPoints.map { [$0.x, 1.0 - $0.y] }` — top-left, ready for Task 8's smile proxy.

- `sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`
  - The brief's three tests, renamed with a `vision` prefix per the task's module-wide-uniqueness rule:
    - `visionReturnsResultForPlainImageWithoutCrashing`
    - `visionBoxesAreNormalisedAndTopLeftOrigin`
    - `visionSceneTagsAreReturnedForARecognisableImage`
  - One additional test, `visionTopLeftFlipsBottomLeftOriginToTopLeft`, added for the mutation-check requirement (see below).

## Deviations from the brief's literal code, and why

1. **`fixture` helper needs `import Foundation`.** The brief's test snippet imports only `Testing` and `CoreGraphics`, but its `fixture()` function uses `URL(fileURLWithPath:)`, which requires `Foundation` (the existing `ImageLoaderTests.swift` already imports it for the same reason). Added `import Foundation`. Without it the file fails to compile with `cannot find 'URL' in scope`, which is exactly what step 2 hit before the fix (see command log below).

2. **`topLeft(_:)` changed from `private static func` to `static func` (internal).** The brief marks it `private`. I needed a way to mutation-check the coordinate flip per the task's explicit requirement ("verify the test genuinely fails if the top-left flip is removed"). The three brief-supplied tests run on flat-color, faceless fixtures, so `saliencyBox`/`faces` may legitimately be empty or, if present, may not exercise asymmetric y/height values that would distinguish a correct flip from a broken one (e.g. a full-frame `[0,0,1,1]` saliency box is flip-invariant). Rather than depend on Vision producing a specific detection on a fixture the brief itself says is unsuited for content assertions, I exposed `topLeft` at internal visibility (default Swift access; `@testable import` already grants test-target visibility, so `internal` is the minimum change) and added a direct unit test with asymmetric values that only a correct flip satisfies. This does not change the type's public API surface (the enum type itself has no `public` modifier either way) and does not weaken any assertion — see mutation-check evidence below.

3. **Removed a redundant `try?` around `hasMinimumRecall(0.0, forPrecision: 0.8)`.** On this SDK (MacOSX26.5, Xcode toolchain, Swift 6.3.3) `VNClassificationObservation.hasMinimumRecall(_:forPrecision:)` is declared as a non-throwing `BOOL` in `VNObservation.h`, confirmed via:
   ```
   grep -rn "hasMinimumRecall" $(xcrun --sdk macosx --show-sdk-path)/System/Library/Frameworks/Vision.framework/Headers/
   → - (BOOL) hasMinimumRecall:(float)minimumRecall forPrecision:(float)precision;
   ```
   The brief's `(try? $0.hasMinimumRecall(...)) ?? false` compiled but produced `warning: no calls to throwing functions occur within 'try' expression`. Changed to a direct (non-optional) call: `$0.hasMinimumRecall(0.0, forPrecision: 0.8)`. Behavior is identical; this only removes a warning-producing no-op `try?`.

No other logic, structure, or requirement from the brief was changed. The single-handler batching, the two-phase seeded landmarks/quality requests, and the precision-based scene-tag filter are all exactly as specified.

## Commands run, in order, with actual output

### Step 2: confirm the test fails before implementation exists

```
$ swift test --package-path sidecar --filter Vision
```
First attempt (before adding `import Foundation` to the test file):
```
/Users/.../VisionAnalyzerTests.swift:6:5: error: cannot find 'URL' in scope
...
/Users/.../VisionAnalyzerTests.swift:13:18: error: cannot find 'VisionAnalyzer' in scope
...
error: fatalError
```
After adding `import Foundation` (still no implementation file present):
```
/Users/.../VisionAnalyzerTests.swift:14:18: error: cannot find 'VisionAnalyzer' in scope
/Users/.../VisionAnalyzerTests.swift:21:18: error: cannot find 'VisionAnalyzer' in scope
/Users/.../VisionAnalyzerTests.swift:30:18: error: cannot find 'VisionAnalyzer' in scope
/Users/.../VisionAnalyzerTests.swift:44:15: error: cannot find 'VisionAnalyzer' in scope
error: fatalError
```
Matches the expected failure mode: `cannot find 'VisionAnalyzer' in scope`.

### Step 3/4: implementation, build, full test run

```
$ swift build --package-path sidecar
Build complete! (0.57s)
```
(No warnings after removing the redundant `try?`.)

```
$ swift test --package-path sidecar
...
◇ Test run started.
↳ Testing Library Version: 1902
↳ Target Platform: arm64e-apple-macos14.0
...
✔ Test visionTopLeftFlipsBottomLeftOriginToTopLeft() passed after 0.012 seconds.
...
✔ Test visionBoxesAreNormalisedAndTopLeftOrigin() passed after 0.291 seconds.
✔ Test visionReturnsResultForPlainImageWithoutCrashing() passed after 0.292 seconds.
✔ Test visionSceneTagsAreReturnedForARecognisableImage() passed after 0.294 seconds.
✔ Test run with 30 tests in 0 suites passed after 3.076 seconds.
```
30 tests total (26 pre-existing + 4 new), all passing, no regressions.

## Mutation-check evidence (coordinate conversion)

Per the task's explicit requirement, I mutated `topLeft(_:)` to remove the flip:
```swift
static func topLeft(_ r: CGRect) -> [Double] {
    // MUTATED: flip removed, passes bottom-left y through unchanged.
    [Double(r.origin.x), Double(r.origin.y),
     Double(r.width), Double(r.height)]
}
```
and re-ran only the coordinate test:
```
$ swift test --package-path sidecar --filter visionTopLeftFlipsBottomLeftOriginToTopLeft
✘ Test visionTopLeftFlipsBottomLeftOriginToTopLeft() recorded an issue at VisionAnalyzerTests.swift:47:5:
  Expectation failed: (abs(box[1] - 0.7) → 0.6) < (1e-9 → 1e-09)
✘ Test visionTopLeftFlipsBottomLeftOriginToTopLeft() failed after 0.001 seconds with 1 issue.
✘ Test run with 1 test in 0 suites failed after 0.001 seconds with 1 issue.
```
The test genuinely fails when the flip is broken — it is not vacuous. The test uses asymmetric `y=0.1, height=0.2` (so bottom-left `y=0.1` vs. correct top-left `y=1-0.1-0.2=0.7` are numerically distinct — no coincidental pass as could happen with e.g. `y=0.3, height=0.4` where both forms happen to equal `0.3`).

I then reverted the mutation and re-ran the full suite to confirm all 30 tests pass again:
```
$ swift test --package-path sidecar
...
✔ Test run with 30 tests in 0 suites passed after 3.079 seconds.
```

I did not separately mutation-check the box-array-bounds (`.count == 4`) or range assertions in `visionBoxesAreNormalisedAndTopLeftOrigin`, since those are the brief's own literal tests operating on content-free fixtures where Vision may legitimately return `nil` for `saliencyBox` — the task explicitly says to assert the contract, not the content, for those.

## Uncertain / worth flagging

- On the flat-color fixtures actually used (`landscape.jpg`, navy field), I observed (via the passing brief tests) `faces.isEmpty == true` and `sceneTags.count <= 8` hold, but I did not print/inspect the actual runtime values of `aestheticScore`, `saliencyBox`, or `sceneTags` beyond what the assertions checked — there was no need to for the given contract-only tests, and the brief explicitly cautions against asserting content on these fixtures. If a future task wants to sanity-check specific field values on `portrait-rot90.jpg` or `gps-south-west.jpg`, that would need new fixture-specific tests, which I deliberately did not add per the "do not invent expectations" instruction.
- `internal` visibility on `topLeft` is a minor, deliberate widening of the brief's `private` for testability; flagging in case a stricter API-surface policy is expected for this codebase (no such policy was evident in `ImageLoader.swift`/`Metrics.swift`, which also expose only what's needed at file/module scope).
