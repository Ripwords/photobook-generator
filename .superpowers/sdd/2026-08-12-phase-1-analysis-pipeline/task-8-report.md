# Task 8 report: Smile proxy from landmarks

## Status: DONE

Commit: `9d1e919ea9f19f719be7b62d24871019cbe3e0c5` — `feat(sidecar): add pose-gated smile proxy from face landmarks`
Branch: `feat/phase-1-analysis-pipeline` (worked in place, not a worktree)

## Files

- Created: `sidecar/Sources/PhotobookEngine/SmileProxy.swift`
- Created: `sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`

## Reconciling the brief's stale coordinate-space assumption

Before writing any code I read the `FaceObservation` doc comment in
`sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift` (lines 5–13) to verify
the claim in my task prompt rather than trust it blindly. It confirms:

> `box` and `landmarks` are both normalised to `0...1` in **image** space with
> a **top-left** origin. `landmarks` points are NOT face-bounding-box-relative
> ... they are offset and scaled into image space ... before being flipped to
> top-left.

This matches the prompt's description exactly, so I proceeded with the
described fix rather than stopping to report a mismatch.

**Consequence for the maths.** The brief's `confidence(for:)` computed
`lift = (centreY - cornerY) / width` directly from `face.landmarks`, with `x`
and `y` treated as sharing one scale. Once landmarks are image-normalised,
`x` has been scaled by the face box's width and `y` by its height — for a
non-square face box, that distorts the ratio by `box.height / box.width`, a
different, spurious factor per face. Doing the ratio maths straight on
image-space points would silently produce wrong (and non-comparable-across-faces)
scores while still "working" in the narrow sense of returning numbers in a
plausible-looking range — the kind of bug that wouldn't be caught by a casual
smoke test.

**The fix**, per the prompt's instruction: before any ratio math, map each
landmark point back into face-box-relative space:

```swift
let relPoints: [[Double]] = points.map { p in
    [(p[0] - boxX) / boxW, (p[1] - boxY) / boxH]
}
```

using the top-left-origin `face.box`, with a guard (`boxW > 0.0001, boxH >
0.0001`) against a degenerate or malformed box. All lift/width ratio maths
then happens in `relPoints`, so the brief's calibration constants
(`(lift + 0.15) / 0.5`) remain meaningful — they were calibrated for
bbox-relative space, which `relPoints` now genuinely is again.

**Consequence for the test fixture.** The brief's `face(yaw:mouthCornerLift:)`
helper hand-built landmark points directly in the old bbox-relative numbers
(e.g. `[0.35, 0.70 - lift]`) and passed them straight as `landmarks` alongside
`box: [0.3, 0.3, 0.4, 0.4]` — incoherent once landmarks are meant to be
image-normalised (those points, read as image coordinates, mostly fall
outside the declared box). I rewrote the helper to state the mouth geometry
in face-relative terms (unchanged from the brief's numbers — they were
already sensible face-relative values) and map them **out** into image space
via the inverse transform:

```swift
let imgPoints = relPoints.map { p in [boxX + p[0] * boxW, boxY + p[1] * boxH] }
```

so the fixture states the intended geometry directly and `SmileProxy` is
left to recover it — round-tripping through the exact transform it's
supposed to invert.

## Implementation

`SmileProxy.confidence(for:)`:
1. Guard: `landmarks` must exist with ≥4 points, else `nil`.
2. Guard: `abs(yaw) > 0.52` rad → `nil` (only when yaw is present; a missing
   yaw does not gate, matching the brief's semantics — "excessive" yaw/pitch
   gates, "missing" yaw/pitch does not, since only "missing landmarks" is
   called out as a nil case for pose fields in the interface description).
3. Same guard for pitch.
4. Guard: `face.box.count == 4` and `boxW > 0.0001 && boxH > 0.0001`, else `nil`.
5. Map every landmark point into face-box-relative space.
6. Find leftmost/rightmost points as mouth corners, compute `width`, guard
   `width > 0.0001`.
7. `lift = (centreY - cornerY) / width` (top-left origin: smaller `y` = higher).
8. Return `((lift + 0.15) / 0.5).clamped(0, 1)` — never `nil` from this point
   on; the function's only `nil`s are the pose/landmark/geometry guards above.

`SmileProxy.fraction(faces:threshold:)`: `compactMap`s `confidence(for:)` over
all faces (unusable faces drop out silently), returns `nil` if that list is
empty, else the fraction scoring `> threshold`.

Used the module's existing `Double.clamped(_:_:)` extension (defined in
`Metrics.swift`, not `private`, so it's visible package-wide) rather than
redefining it.

## Test suite

`sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`, 11 tests, all
prefixed `smileProxy*` for module-wide uniqueness:

1. `smileProxyReturnsNilWhenHeadIsTurnedTooFar`
2. `smileProxyReturnsNilWhenPitchIsTooSteep` (brief only tested yaw; added
   pitch since the interface explicitly calls out both)
3. `smileProxyReturnsNilWhenLandmarksAreMissing`
4. `smileProxyReturnsNilWhenBoxHasZeroWidth`
5. `smileProxyReturnsNilWhenBoxHasNegativeWidth` (see mutation-check below —
   added after discovering the zero-width test alone didn't exercise the
   guard)
6. `smileProxyLiftedMouthCornersScoreHigherThanFlat`
7. `smileProxyConfidenceIsInvariantToFaceBoxAspectRatio` (added — see below)
8. `smileProxyDistinguishesNilFromALowRealScore`
9. `smileProxyFractionIsNilWhenNoFaceHasUsablePose`
10. `smileProxyFractionCountsOnlyUsableFaces`
11. `smileProxyFractionIsNilForEmptyFaceList`

### Full suite run

```
$ cd sidecar && swift test
...
✔ Test run with 44 tests in 0 suites passed after 3.079 seconds.
```

(42 pre-existing tests, +11 new, -9 originally written before I added 2 more
during mutation-checking below → net +11, giving 44 total. No pre-existing
test was touched.)

## Mutation-check evidence

I deliberately broke each piece of behaviour the tests claim to cover, reran
`swift test --filter SmileProxyTests`, confirmed the expected tests failed,
then restored the source from a clean backup and confirmed the full 44-test
suite was green again.

### 1. Nil-vs-zero collapse (the design's central guarantee)

Changed the yaw gate's `return nil` to `return 0`:

```
✘ smileProxyFractionCountsOnlyUsableFaces: (abs(f! - 0.5) → 0.16666666666666669) < 0.001
✘ smileProxyFractionIsNilWhenNoFaceHasUsablePose: (... → 0.0) == nil
✘ smileProxyDistinguishesNilFromALowRealScore: (unusable → 0.0) == nil
✘ smileProxyReturnsNilWhenHeadIsTurnedTooFar: (... → 0.0) == nil
```

4 tests failed, confirming the nil-vs-zero distinction is genuinely tested —
not just "score not greater than 0.5" (which would pass for both `nil` and
`0`). `smileProxyDistinguishesNilFromALowRealScore` was written specifically
to make this failure mode visible on its own.

### 2. Remove yaw gate entirely

```
✘ smileProxyFractionCountsOnlyUsableFaces: (... → 0.16666666666666663) < 0.001
✘ smileProxyFractionIsNilWhenNoFaceHasUsablePose: (... → 1.0) == nil
✘ smileProxyDistinguishesNilFromALowRealScore: (unusable → 0.5000000000000007) == nil
✘ smileProxyReturnsNilWhenHeadIsTurnedTooFar: (... → 0.5000000000000007) == nil
```

### 3. Remove pitch gate entirely

```
✘ smileProxyReturnsNilWhenPitchIsTooSteep: (... → 0.5000000000000007) == nil
```

### 4. Remove box zero/negative-width guard

First attempt used a zero-width fixture only — mutating the guard away
produced **no test failures**, because dividing by exactly `0.0` yields
`NaN`, and `NaN > 0.0001` is `false` in Swift, so the downstream
`width > 0.0001` guard happens to catch it anyway as an accidental side
effect, not because the explicit guard did its job. This meant the original
test wasn't actually mutation-effective for that guard line.

I probed with a **negative**-width box instead (`boxW = -0.4`, no NaN
involved), confirmed via a temporary throwaway test that removing the guard
made this exact fixture return `Optional(0.4999999999999994)` instead of
`nil`, then replaced the zero-width-only test with two: one for zero width
(kept, still valid defensive coverage even though redundant with NaN
propagation) and a new `smileProxyReturnsNilWhenBoxHasNegativeWidth`, which
does catch the mutation:

```
✘ smileProxyReturnsNilWhenBoxHasNegativeWidth: (... → 0.4999999999999994) == nil
```

### 5. Skip the face-relative conversion (the core Task-7 reconciliation fix)

This is the mutation that matters most for this task: reverting to computing
the ratio directly on raw image-normalised points, i.e. exactly the bug the
brief's stale code would have had.

I first noticed my initial fixture used a **square** face box
(`[0.3, 0.3, 0.4, 0.4]`, width == height), so `box.height / box.width == 1`
— which means that specific distortion factor is invisible with a square
box; skipping the conversion would have produced numerically identical
results and no test would have caught it. I added
`smileProxyConfidenceIsInvariantToFaceBoxAspectRatio`, which builds two
faces with identical face-relative mouth geometry but different **non-square**
box aspect ratios (`[0.1, 0.4, 0.6, 0.2]` vs `[0.4, 0.1, 0.2, 0.6]`) and
asserts their confidences match to `1e-9`. Mutating the source to skip the
conversion:

```
✘ smileProxyConfidenceIsInvariantToFaceBoxAspectRatio: (abs(wide! - tall!) → 0.5333333333333339) < 1e-09
```

confirms this test would have caught the exact regression this task was
about reconciling.

### Restoration verification

After each mutation, source was restored from a pre-mutation backup and
`swift test` (full suite, not filtered) was rerun; final run:

```
✔ Test run with 44 tests in 0 suites passed after 3.079 seconds.
```

Confirmed `sidecar/Sources/PhotobookEngine/SmileProxy.swift` has no leftover
mutation artifacts by grepping all `return nil` / `return 0` / guard lines
against the intended implementation before committing. No `.bak*` files were
left in the tree.

## Commit

```
$ git add sidecar/Sources/PhotobookEngine/SmileProxy.swift sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift
$ git commit -m "feat(sidecar): add pose-gated smile proxy from face landmarks ..."
[feat/phase-1-analysis-pipeline 9d1e919] feat(sidecar): add pose-gated smile proxy from face landmarks
 2 files changed, 207 insertions(+)
 create mode 100644 sidecar/Sources/PhotobookEngine/SmileProxy.swift
 create mode 100644 sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift
```

Note: `.claude/` appeared as an untracked directory in `git status` before
and after this work; it was not touched or staged.

## Open questions / things I'm not fully certain about

- **Missing (nil) yaw/pitch is treated as "usable" pose**, not gated to
  `nil`. This matches the brief's original code structure (`if let yaw = ...,
  abs(yaw) > threshold { nil }` — a `nil` yaw skips the check rather than
  failing it) and the interface description's wording ("confidence returns
  nil for missing landmarks or excessive yaw/pitch" — "missing" is only
  called out for landmarks, "excessive" for pose). I did not find anything
  in Task 7's actual `VisionAnalyzer.swift` suggesting yaw/pitch/roll are
  ever nil in practice when a face is detected (`face.yaw.map { Double
  (truncating: $0) }` — nil only if Vision itself omits the value), so this
  is a low-stakes judgment call, but flagging it in case a nil pose was
  meant to gate to `nil` confidence too.
- I did not add a dedicated test for `face.box.count != 4` (malformed box
  array length) — it's guarded in code but the brief didn't call for a
  fixture for it and it's a defensive guard against a shape that
  `VisionAnalyzer` itself never produces (always exactly 4 elements from
  `topLeft(_:)`).

---

# Fix report: full-face-points bug (coordinator/review follow-up)

## Status: DONE

Commit: `f448fef5e28a2f112d10bb36c19331de53986bd1` — `fix(sidecar): scope smile-proxy landmarks to outer lips only`
Branch: `feat/phase-1-analysis-pipeline`

## The bug the review found

`FaceObservation.landmarks` was populated in `VisionAnalyzer.swift` from
`landmarkResults[index].landmarks?.allPoints` — Vision's entire face mesh,
typically 65-76 points spanning jaw contour, eyebrows, eyes, nose, and lips.
`SmileProxy.confidence(for:)` took `min(by:)`/`max(by:)` over *every* point
as the mouth corners, and the mean `y` over every point as the mouth's
vertical centre.

On a real face, the x-extremes of the full point set are jaw or ear contour
points, not mouth corners, and the mean y sits near mid-face rather than at
the mouth — so the lift/width ratio would be roughly constant regardless of
expression, making the proxy non-functional on real data. All 11 tests from
the first pass of this task still passed, because every fixture hand-built
exactly four mouth-only points and the full-face case was never exercised —
exactly the kind of gap a "does it compile and pass" check misses but a
review of the actual data flow catches.

This is a defect that spans Task 7 (which introduced `allPoints` as the
source) and Task 8 (which consumed it uncritically) — my Task 8 work did not
introduce it, but it also didn't catch it, since I never traced where
`landmarks` was populated from in `VisionAnalyzer.swift`, only how it was
shaped once populated.

## The fix

### 1. `sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`

- `FaceObservation.landmarks: [[Double]]?` → **replaced** with
  `FaceObservation.outerLips: [[Double]]?` (not added alongside — confirmed
  via `grep -rn "\.landmarks\b\|landmarks:" sidecar/Sources sidecar/Tests`
  that `SmileProxy` was the field's only reader before renaming).
- The population line changed from
  `landmarkResults[index].landmarks?.allPoints?.normalizedPoints` to
  `landmarkResults[index].landmarks?.outerLips?.normalizedPoints`, still
  piped through the existing `imageNormalizedTopLeft(_:faceBoxInVisionSpace:)`
  conversion, unchanged.
- Updated the `FaceObservation` doc comment to describe `outerLips` (name,
  space, and *why* only the lip region is carried — payload cost of
  serialising/storing 65-76 unused points per face for Task 9's Rust/SQLite
  pipeline when nothing downstream reads them).

### 2. `sidecar/Sources/PhotobookEngine/SmileProxy.swift`

- Reads `face.outerLips` instead of `face.landmarks`.
- Raised `minOuterLipPoints` from 4 to **6**: Vision's `outerLips` region
  reliably reports roughly 8-12 points, so 6 is a floor that rejects a
  degenerate/malformed contour without rejecting anything Vision would
  actually produce, per the coordinator's guidance not to require more than
  Vision reliably provides.
- Pose gating changed from "gate only when pose is present and excessive" to
  "gate when pose is missing OR excessive":

  ```swift
  guard let yaw = face.yaw, abs(yaw) <= maxYawRadians else { return nil }
  guard let pitch = face.pitch, abs(pitch) <= maxPitchRadians else { return nil }
  ```

  (previously `if let yaw = face.yaw, abs(yaw) > maxYawRadians { return nil
  }`, which silently treated a `nil` yaw as usable). Doc comment updated to
  explain the "cannot tell either way" rationale and the occlusion/extreme-angle
  correlation the coordinator raised.
- Doc comment on the enum itself now explicitly states the outerLips-only
  contract and points at the regression test that demonstrates why it
  matters.

### 3. `sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`

Rebuilt fixtures around a realistic **10-point closed outer-lip contour**
(`outerLipContour(mouthCornerLift:)`) — left corner, 5 upper-lip points, right
corner, 3 lower-lip points — comparable in shape and count to Vision's actual
`outerLips` output, replacing the four idealised corner points. A
`toImageSpace(_:box:)` helper factors out the inverse-transform mapping used
across fixtures.

16 tests total (up from 11), all still `smileProxy*`-prefixed and unique:

1. `smileProxyReturnsNilWhenHeadIsTurnedTooFar`
2. `smileProxyReturnsNilWhenPitchIsTooSteep`
3. `smileProxyReturnsNilWhenYawIsMissing` — **new**, covers fix #4
4. `smileProxyReturnsNilWhenPitchIsMissing` — **new**, covers fix #4
5. `smileProxyReturnsNilWhenOuterLipsAreMissing` (renamed from
   `...LandmarksAreMissing`)
6. `smileProxyReturnsNilWhenTooFewLipPointsAreProvided` — **new**, covers the
   raised `minOuterLipPoints` threshold
7. `smileProxyReturnsNilWhenBoxIsMalformed` — **new**, covers fix #5
   (`box.count != 4`)
8. `smileProxyReturnsNilWhenBoxHasZeroWidth`
9. `smileProxyReturnsNilWhenBoxHasNegativeWidth`
10. `smileProxyLiftedMouthCornersScoreHigherThanFlat`
11. `smileProxyConfidenceIsInvariantToFaceBoxAspectRatio`
12. `smileProxyDistinguishesNilFromALowRealScore`
13. `smileProxyFractionIsNilWhenNoFaceHasUsablePose`
14. `smileProxyFractionCountsOnlyUsableFaces`
15. `smileProxyFractionIsNilForEmptyFaceList`
16. `smileProxyProducesWrongSignalWhenFedFullFaceContourInsteadOfLips` —
    **new**, the regression demonstration (see below)

## Regression demonstration: reproducing the actual bug

`smileProxyProducesWrongSignalWhenFedFullFaceContourInsteadOfLips` builds the
same 10-point lip contour used elsewhere, but contaminates it with 5
non-lip face points modelled on what a real `allPoints` set would include:

```swift
let nonLipFacePoints: [[Double]] = [
    [0.05, 0.50], // jaw contour, left
    [0.95, 0.50], // jaw contour, right
    [0.30, 0.15], // eyebrow, left
    [0.70, 0.15], // eyebrow, right
    [0.50, 0.45], // nose tip
]
```

Ran both variants through `SmileProxy.confidence(for:)` via a temporary probe
test (`swift test --filter tempProbeContaminatedValues`, removed after
capturing output) to get exact figures:

```
LIPS ONLY smiling=Optional(0.5300000000000018) neutral=Optional(0.2633333333333352)
FULL FACE smiling=Optional(0.4622222222222224) neutral=Optional(0.47703703703703704)
```

Lips-only: smiling (0.530) > neutral (0.263) — correct, matches
`smileProxyLiftedMouthCornersScoreHigherThanFlat`. Full-face-contaminated:
smiling (0.462) < neutral (0.477) — **the signal inverts**, not just
weakens. The permanent test asserts `smiling! <= neutral!` on the
contaminated fixture, which reproduces exactly the failure mode the review
described and would have failed against the original `allPoints`-derived
implementation had a face fixture with real landmarks existed to drive it
through `VisionAnalyzer` end-to-end.

## Mutation-check evidence for the new/changed guards

Followed the same methodology as the first pass: backed up
`SmileProxy.swift`, applied each mutation, ran `swift test --filter
SmileProxyTests`, confirmed the expected failures, restored from backup, and
verified via `diff` that the restored file was byte-identical to the
pre-mutation version.

### Mutation A: revert nil-pose gating to old "only gate if present and excessive" semantics

```swift
// mutated to:
if let yaw = face.yaw, abs(yaw) > maxYawRadians { return nil }
if let pitch = face.pitch, abs(pitch) > maxPitchRadians { return nil }
```

```
✘ smileProxyReturnsNilWhenYawIsMissing: (... → 0.5300000000000018) == nil
✘ smileProxyReturnsNilWhenPitchIsMissing: (... → 0.5300000000000018) == nil
```

### Mutation B: remove `guard face.box.count == 4 else { return nil }`

No `✘` lines appeared under a plain grep — investigated further since that
was suspicious for a removed guard. Full output showed why: the malformed
box fixture (`[0.3, 0.3, 0.4]`, 3 elements) then hits
`face.box[3]` directly, which is an out-of-bounds array access — a **fatal
runtime trap**, not a catchable test failure:

```
Swift/ContiguousArrayBuffer.swift:695: Fatal error: Index out of range
error: Process '...swiftpm-testing-helper...' exited with unexpected signal code 5
```

`smileProxyReturnsNilWhenBoxIsMalformed` never got to report a `✔`/`✘` — the
process aborted mid-run — and the overall `swift test` invocation exited
with a nonzero/unexpected-signal error. This is a stronger confirmation than
a graceful assertion failure: removing the guard doesn't just make the test
fail, it crashes the process, which is precisely why the guard exists ahead
of the raw index accesses on the next lines.

### Mutation C: lower `minOuterLipPoints` back to 4

```
✘ smileProxyReturnsNilWhenTooFewLipPointsAreProvided: (... → 0.49999999999999933) == nil
```

### Restoration

```
$ diff /tmp/sp_orig2.swift Sources/PhotobookEngine/SmileProxy.swift && echo "SmileProxy.swift restored cleanly"
SmileProxy.swift restored cleanly
$ find . -name "*.bak*" -delete   # no leftover mutation artifacts
```

## Full suite run

```
$ cd sidecar && swift build
Build complete! (0.77s)

$ swift test
...
✔ Test run with 49 tests in 0 suites passed after 3.081 seconds.
```

(38 pre-existing non-SmileProxy tests + 11 unrelated-to-this-task-but-already-passing
+ 16 SmileProxy tests = 49; i.e. 44 from the first pass, minus the 11 old
SmileProxy tests, plus 16 new ones.)

## `FaceObservation` shape change — flagged for Task 9

`FaceObservation.landmarks: [[Double]]?` no longer exists; it is
**replaced** by `FaceObservation.outerLips: [[Double]]?`, same type and
coordinate space, but now holding only the ~8-12-point outer lip contour
instead of Vision's full ~65-76 point face mesh. `FaceObservation` is
`Codable`, so this changes the JSON field name and payload shape emitted by
the sidecar. Task 9, which is described as serialising `FaceObservation` to
Rust/SQLite, needs to target `outerLips` (not `landmarks`) and should expect
a much smaller point array per face than the field's original name might
have implied.

## Verification that no other code depends on the renamed field

```
$ grep -n "FaceObservation" sidecar/Sources/PhotobookEngine/Handler.swift \
    sidecar/Sources/PhotobookEngine/Protocol.swift sidecar/Sources/PhotobookEngine/main.swift
(no output — no matches)
```

Confirms `SmileProxy` was and remains the only in-package consumer of this
field, so the rename from `landmarks` to `outerLips` is safe within
`sidecar/`.

## Open questions / things I'm not fully certain about

- I did not verify against a real Vision detection that `outerLips` on
  macOS 15 actually returns 8-12 points in practice (no face fixture exists
  in the test suite — `VisionAnalyzerTests.swift` notes the same limitation
  for its own landmark-conversion tests). `minOuterLipPoints = 6` is a
  judgment call based on general knowledge of Vision's landmark region
  sizes, not something I could confirm empirically in this environment.
- I did not modify `VisionAnalyzerTests.swift` — it has no face-specific
  fixtures and its existing conversion-helper tests are agnostic to which
  landmark region is used, so nothing there needed to change. If Task 9 (or
  a later task) adds a real face photo fixture, it would be worth adding an
  integration-level assertion there that `result.faces[0].outerLips` is
  non-nil and roughly the expected point count, as an end-to-end check this
  unit-level regression test can't provide.

