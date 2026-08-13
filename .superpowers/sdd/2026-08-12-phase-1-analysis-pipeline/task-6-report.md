# Task 6 Report — Classical metrics (sharpness, palette, pHash)

## Status: DONE_WITH_CONCERNS

## What was implemented

- `sidecar/Sources/PhotobookEngine/Metrics.swift` — `PaletteColor` struct and `Metrics` enum with:
  - `Metrics.sharpness(_ image: CGImage) -> Double` — contrast-normalised tile-max Laplacian variance over a 4x4 grid, computed on a 256x256 downscale.
  - `Metrics.palette(_ image: CGImage, count: Int) -> [PaletteColor]` — uniform 4x4x4 RGB bucket histogram on a 128x128 downscale, returns the heaviest buckets with weights normalised to sum to 1.
  - `Metrics.perceptualHash(_ image: CGImage) -> UInt64` — 8x8 average hash (mean-threshold) over an 8x8 grey downscale.
  - `Metrics.warmth(_ palette: [PaletteColor]) -> Double` — weighted red/blue balance, clamped 0...1.
  - `Metrics.contrast(_ image: CGImage) -> Double` — global luma spread on a 128x128 downscale, normalised 0...1.
  - `private extension Array where Element == PaletteColor { normalisedWeights() }` and file-scope `extension Double { clamped(_:_:) }`.
- `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift` — the brief's 5 tests, verbatim, plus the brief's `makeImage` helper.

Implementation matches the brief's Step 3 code exactly — no algorithmic changes.

## Naming-collision check (done before writing)

Ran `grep -rn "makeImage\|struct PaletteColor\|enum Metrics\|func clamped\|extension Double\|sharpEdgesScoreHigherThanFlatField\|sharpSubjectOnBlankBackgroundIsNotScoredBlurry\|paletteReturnsRequestedCountAndWeightsSumToOne\|identicalImagesHaveIdenticalHash\|differentImagesHaveDifferentHash\|normalisedWeights" sidecar/` against the pre-existing tree (`ProtocolTests.swift`, `ImageLoaderTests.swift`, `ExifReaderTests.swift`, and all `Sources/`) — **no output, no collisions**. So all test names and the `makeImage` helper and `extension Double { clamped }` were kept exactly as the brief specifies; no renaming was needed.

## Commands run and output

### Step 2 — confirm the test fails first

```
$ swift test --package-path sidecar
```
Output (relevant excerpt):
```
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/MetricsTests.swift:38:13: error: cannot find 'Metrics' in scope
    #expect(Metrics.sharpness(localDetail) > Metrics.sharpness(flat) * 5)
            `- error: cannot find 'Metrics' in scope
... (7 similar "cannot find 'Metrics' in scope" errors, one per Metrics.* call site)
error: fatalError
```
Confirmed: fails to compile because `Metrics` doesn't exist yet, as expected.

### Step 4 — confirm tests pass after implementation

```
$ swift test --package-path sidecar
```
Output (relevant excerpt):
```
Build complete! (1.25s)
...
✔ Test sharpSubjectOnBlankBackgroundIsNotScoredBlurry() passed after 0.048 seconds.
✔ Test sharpEdgesScoreHigherThanFlatField() passed after 0.049 seconds.
✔ Test paletteReturnsRequestedCountAndWeightsSumToOne() passed after 0.006 seconds.
✔ Test identicalImagesHaveIdenticalHash() passed after 0.006 seconds.
✔ Test differentImagesHaveDifferentHash() passed after 0.005 seconds.
Test run with 21 tests in 0 suites passed after 0.049 seconds.
```
All 5 new tests pass; all 16 pre-existing tests (Protocol, ImageLoader, ExifReader suites) still pass — 21/21 total.

## Naive whole-image variance check — important finding

The brief and the orchestrator both required explicit confirmation that `sharpSubjectOnBlankBackgroundIsNotScoredBlurry` **fails** under a naive whole-image (non-tiled) Laplacian variance implementation, to prove the tile-max design is load-bearing.

I temporarily replaced `Metrics.sharpness` with a naive whole-image variance (no tiling, no per-tile contrast normalisation — full diff shown below) and reran the Metrics test filter:

```swift
// TEMP naive implementation used only for this check, then reverted:
static func sharpness(_ image: CGImage) -> Double {
    let side = 256
    let bytes = rgbaBuffer(image, width: side, height: side)
    var grey = [Double](repeating: 0, count: side * side)
    for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }
    var values: [Double] = []
    for y in 1..<(side - 1) {
        for x in 1..<(side - 1) {
            let c = grey[y * side + x]
            let lap = grey[(y-1)*side+x] + grey[(y+1)*side+x] + grey[y*side+x-1] + grey[y*side+x+1] - 4*c
            values.append(lap)
        }
    }
    let mean = values.reduce(0, +) / Double(values.count)
    let variance = values.reduce(0) { $0 + ($1 - mean) * ($1 - mean) } / Double(values.count)
    return variance
}
```

```
$ swift test --package-path sidecar --filter MetricsTests
...
✔ Test sharpSubjectOnBlankBackgroundIsNotScoredBlurry() passed after 0.039 seconds.
```

**This test unexpectedly still passed under the naive implementation.** I did not stop there — I added a temporary debug test to print the actual numeric values:

```
DEBUG flat=0.0 local=4968.027883602302 local>flat*5=true
```

Root cause: the test's `flat` fixture is a *perfectly constant* pixel value (200 everywhere), so its Laplacian variance is exactly `0.0` under **both** the naive and tile-max algorithms — tiling changes nothing when every tile is uniform. Since the assertion is `local > flat * 5` i.e. `local > 0`, it passes trivially for any implementation that gives the checkerboard region a non-zero score, tiled or not. The test as literally written in the brief does not actually falsify a naive whole-image implementation — it only asserts that *some* detail is scored above *zero*, not that the detail survives being diluted across a mostly-flat frame.

I want to be precise about what this does and doesn't mean:
- The **tile-max design itself is still correct and necessary** for the stated goal (scoring subject-in-focus rather than being washed out by a blank background) — a truly noisy/textured "blank wall" (not perfectly flat) would wash out a naive whole-image variance in a way this specific test doesn't exercise, because it uses a zero-variance flat baseline rather than a noisy one.
- I did **not** weaken the test or the implementation to make this pass — I implemented the brief's algorithm exactly as specified (tile-max, contrast-normalised, unchanged from Step 3).
- I reverted the naive implementation and the debug test immediately after confirming this; `git diff` against the committed `Metrics.swift` shows it is byte-identical to the brief's Step 3 code (confirmed via `diff` against a saved copy before commit).

I'm flagging this as a **concern to report**, not silently patching the test, per the instruction not to weaken tests or substitute algorithms without saying so. The shipped implementation matches the brief exactly; only the test's discriminating power against a naive baseline is weaker than claimed.

### Step 5 — commit

```
$ git add sidecar/Sources/PhotobookEngine/Metrics.swift sidecar/Tests/PhotobookEngineTests/MetricsTests.swift
$ git commit -m "feat(sidecar): add sharpness, palette, and perceptual hash metrics"
[feat/phase-1-analysis-pipeline 0fb064d] feat(sidecar): add sharpness, palette, and perceptual hash metrics
 2 files changed, 209 insertions(+)
 create mode 100644 sidecar/Sources/PhotobookEngine/Metrics.swift
 create mode 100644 sidecar/Tests/PhotobookEngineTests/MetricsTests.swift
```

Full test suite reran clean after commit (21/21 passing), and `git status --short sidecar/` showed no leftover debug artifacts (the temporary `DebugMetricsTests.swift` file and the naive-variance edit were both removed/reverted before staging and committing).

## Deviations from the brief

None to the implementation or test code — both were used verbatim. The only deviation is procedural: I performed an additional debug pass (temporary naive implementation + a throwaway debug test with a `print`) to empirically verify the naive-vs-tile-max claim, then fully reverted both before committing. Neither is present in the committed diff.

## Things I'm unsure about / would flag for follow-up

1. **Test doesn't prove what it claims to prove.** As detailed above, `sharpSubjectOnBlankBackgroundIsNotScoredBlurry` passes under both tile-max and naive whole-image variance because its `flat` baseline is perfectly uniform (variance exactly 0 either way). If a future refactor accidentally reverts to whole-image variance, this test will **not** catch it. A stronger version would use a `flat`-with-low-level-noise baseline (e.g. `((x*31+y*17) % 5)` jitter) so a naive implementation's variance is non-zero and dilutes the local-detail score below the 5x threshold, while tile-max still isolates the hot tile. I did not make this change since I was told to use the brief's test verbatim and not weaken/substitute it; flagging it for whoever owns the brief/test suite going forward.
2. Not otherwise unsure about the implementation — it's pure, stateless (no shared mutable state, safe for `DispatchQueue.concurrentPerform` per the Task 9 note), and `perceptualHash` returns `UInt64` as required for the Task 12 Rust-side Hamming clustering.

---

## Fix report — replacing the vacuous sharpness test (follow-up)

The coordinator confirmed the finding above: `sharpSubjectOnBlankBackgroundIsNotScoredBlurry` was vacuous because its `flat` baseline is perfectly uniform, so its Laplacian variance is `0.0` under any implementation, tiled or not — the assertion `local > flat * 5` reduces to `local > 0`, which discriminates nothing about tiling.

### Fix applied

Replaced that test in `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift` with `metricsTileMaxIgnoresEmptyAreaAroundASharpSubject`, per the coordinator's exact spec: instead of comparing local detail to a flat baseline, it compares local detail (one tile of a 4x4 grid detailed, rest uniform) to full-frame detail (every tile detailed, same checkerboard cell size `/ 4` in both). Tile-max predicts these stay close (best tile is equally detailed in both); whole-image variance predicts local ≈ full/16 because fifteen empty tiles dilute the average.

```swift
@Test func metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() {
    // Detail everywhere.
    let fullyDetailed = makeImage(width: 128, height: 128) { x, y in
        ((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255
    }
    // The same detail confined to the top-left quarter-width square — one tile
    // of the 4x4 grid — with the remaining fifteen sixteenths uniform.
    let localDetail = makeImage(width: 128, height: 128) { x, y in
        (x < 32 && y < 32) ? (((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255) : 200
    }

    let full = Metrics.sharpness(fullyDetailed)
    let local = Metrics.sharpness(localDetail)

    // Tile-max: the best tile is equally detailed in both, so these are close.
    // Whole-image variance: local would be roughly full/16.
    let message = "confined detail scored \(local) against \(full) for full-frame detail; "
        + "a ratio near 1/16 means sharpness is averaging over the whole frame "
        + "instead of taking the best tile"
    #expect(local > full * 0.5, "\(message)")
}
```

(One mechanical fix needed vs. the coordinator's literal snippet: swift-testing's `#expect(_:_:)` second parameter is `Comment`, which doesn't support `String + String` concatenation directly, so the message is built as a `String` first, then interpolated into the call — `#expect(local > full * 0.5, "\(message)")`. No change to the assertion or values.)

Name-collision check: `grep -rn "metricsTileMaxIgnoresEmptyAreaAroundASharpSubject" sidecar/` outside the test file itself returned nothing, so no rename was needed.

### Commands and output

**1. New test passes under the real tile-max implementation:**

```
$ swift test --package-path sidecar
```
```
✔ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() passed after 0.048 seconds.
✔ Test sharpEdgesScoreHigherThanFlatField() passed after 0.051 seconds.
Test run with 21 tests in 0 suites passed after 0.051 seconds.
```

**2. Mutation check — temporarily replaced `Metrics.sharpness`'s body with naive whole-image Laplacian variance (no tiling, no contrast normalisation, same code as the earlier vacuous-test check), then:**

```
$ swift test --package-path sidecar --filter metricsTileMaxIgnoresEmptyAreaAroundASharpSubject
```
```
◇ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() started.
✘ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() recorded an issue at MetricsTests.swift:51:5: Expectation failed: (local → 846.3824477648956) > (full * 0.5 → 7256.01426002852)
↳ confined detail scored 846.3824477648956 against 14512.02852005704 for full-frame detail; a ratio near 1/16 means sharpness is averaging over the whole frame instead of taking the best tile
✘ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() failed after 0.039 seconds with 1 issue.
✘ Test run with 1 test in 0 suites failed after 0.039 seconds with 1 issue.
```

The test **failed as required**, with no threshold adjustment — I did not touch `0.5` to make it pass. The observed ratio under the naive implementation was `846.38 / 14512.03 ≈ 0.0583 ≈ 1/17.2`, matching the predicted ~1/16 dilution almost exactly. This is strong empirical confirmation that the new test is load-bearing for the tile-max design, unlike the one it replaced.

**3. Reverted `Metrics.swift` to the tile-max implementation and confirmed byte-identity with the originally committed version:**

```
$ diff <saved-copy-of-committed-Metrics.swift> sidecar/Sources/PhotobookEngine/Metrics.swift
```
(no output — files identical)

**4. Full suite green again after revert:**

```
$ swift test --package-path sidecar
```
```
✔ Test sharpEdgesScoreHigherThanFlatField() passed after 0.047 seconds.
✔ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() passed after 0.048 seconds.
Test run with 21 tests in 0 suites passed after 0.049 seconds.
```

`git status --short sidecar/` at this point showed only `MetricsTests.swift` modified — `Metrics.swift` had no diff, confirming the mutation-and-revert cycle left the shipped implementation untouched.

### Scrutiny of the other four Metrics tests

As requested, I checked whether each of the remaining four tests would catch a wrong implementation of the thing it claims to test, by empirically running a plausible wrong/stub implementation against each assertion (not modifying any shipped code — done via throwaway test files, removed after). **I have not changed any of these tests or implementations; reporting only, per instruction.**

1. **`sharpEdgesScoreHigherThanFlatField`** — **weak, in the way the coordinator already flagged (kept only as a basic sanity check).** Empirically confirmed: swapping in `Metrics.contrast` (a wholly different, already-implemented metric with no edge/frequency detection, just global min-max luma spread) as a stand-in for "sharpness" still passes this test — `contrast(checker) ≈ 1.0 > contrast(flat) = 0.0`. So this test cannot distinguish "computes Laplacian edge variance" from "computes anything that's larger for high-variance images than for constant ones." It is a reasonable smoke test but proves nothing about the specific algorithm.

2. **`paletteReturnsRequestedCountAndWeightsSumToOne`** — **vacuous with respect to color-extraction correctness.** It only checks structural invariants: `1 <= count <= 4` and weights summing to 1. Empirically confirmed: a stub `palette(_:count:)` that ignores the input image entirely and returns `count` copies of `PaletteColor(r: 0.5, g: 0.5, b: 0.5, weight: 1/count)` passes all three assertions regardless of the image's actual two-cluster content (dark-left/light-right at 20/220). The test never checks that the returned colors are anywhere near the image's actual pixel values, or that the weights reflect the true ~50/50 split between the two halves.

3. **`identicalImagesHaveIdenticalHash`** — **vacuous in isolation; only meaningful paired with test 4.** Empirically confirmed: a stub `perceptualHash` that always returns `0` regardless of input passes this test trivially (`0 == 0`). It tests determinism/purity of the function, not that the hash reflects image content at all.

4. **`differentImagesHaveDifferentHash`** — **the one real content-sensitivity check, and it does catch the constant-hash stub** (confirmed: the same always-`0` stub fails this test, since `0 != 0` is false). Its construction is reasonably clever — `a` (left/right split) and `b` (top/bottom split) have identical global pixel-value means (each is 50% at value 0, 50% at value 255), so a hash that only measured global brightness (rather than an 8x8 spatial average-hash) could plausibly collide here too, though I did not build a concrete stub to confirm that specific failure mode. It does not, however, verify anything about Hamming-distance behavior for near-duplicate images, which is presumably load-bearing for Task 12's clustering use case.

**Net assessment:** tests 1 and 3 are individually non-discriminating for the property named in their title (they'd pass under implementations that don't do the described thing); test 2 doesn't check correctness of palette extraction at all, only output shape; test 4 is the strongest of the four but still leaves the "hash reflects perceptual similarity, not just presence of some spatial variation" question unverified. I have made no changes to any of these four tests or their implementations, pending your direction.

---

## Fix report — palette composition and pHash Hamming-distance tests (second follow-up)

The coordinator directed two of the four audited-but-not-yet-fixed tests to be strengthened now (palette, pHash); the other two (`sharpEdgesScoreHigherThanFlatField`, `identicalImagesHaveIdenticalHash`) were explicitly left as-is with rationale given, so no change was made to them.

### Fix 1 — `metricsPaletteReflectsActualImageComposition`

Added alongside (not replacing) `paletteReturnsRequestedCountAndWeightsSumToOne`, in `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`. Needed a new helper, `makeColorImage`, since the existing `makeImage` only supports a single replicated grey value per pixel and this test needs independent R/G/B channels. Checked for collisions first: `grep -rn "makeColorImage\|func hamming\|metricsPaletteReflectsActualImageComposition\|metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated" sidecar/` outside the test file itself — none found.

Image: 128x128, left three quarters (x < 96) solid blue `(30, 60, 200)`, right quarter solid orange `(230, 120, 20)` — a 3:1 area split. Assertions: count/sum invariants kept, plus the heaviest entry is within 0.1 per-channel of blue, some entry is within 0.1 per-channel of orange, and the heaviest entry's weight is more than double the orange entry's weight (reflecting 3:1).

**Actual measured values** (captured via a throwaway debug test, removed before committing):
```
DEBUG PALETTE: [PaletteColor(r: 0.1176..., g: 0.2353..., b: 0.7843..., weight: 0.75),
                PaletteColor(r: 0.9020..., g: 0.4706..., b: 0.0784..., weight: 0.25)]
```
Since the test image is drawn at its native 128x128 into `palette`'s 128x128 internal buffer (no resampling), bucket-averaging reproduced the two solid colours essentially exactly (0.1176 ≈ 30/255, 0.7843 ≈ 200/255, etc.), and weights landed exactly on 0.75/0.25 — well inside the 0.1 tolerance and comfortably past the 2x weight-ratio bar.

**Mutation check** — temporarily replaced `Metrics.palette` with a stub that ignores `image` entirely and returns `count` copies of `PaletteColor(r: 0.5, g: 0.5, b: 0.5, weight: 1/count)`:

```
$ swift test --package-path sidecar --filter metricsPaletteReflectsActualImageComposition
```
```
✘ Test metricsPaletteReflectsActualImageComposition() recorded an issue at MetricsTests.swift:113:5:
  Expectation failed: isClose((heaviest → PaletteColor(r: 0.5, g: 0.5, b: 0.5, weight: 0.25)), to: (blue → (r: 30.0, g: 60.0, b: 200.0)))
  ↳ heaviest entry was r=0.5 g=0.5 b=0.5 weight=0.25, expected close to blue (r: 30.0, g: 60.0, b: 200.0)
✘ Test metricsPaletteReflectsActualImageComposition() recorded an issue at MetricsTests.swift:117:5:
  Expectation failed: (orangeEntry → nil) != nil
  ↳ no palette entry close to orange (r: 230.0, g: 120.0, b: 20.0) found among [PaletteColor(r: 0.5, ...), x4]
✘ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject failed after 0.005 seconds with 2 issues.
```
Failed as required, with no threshold adjustment. Reverted `Metrics.swift` and confirmed byte-identity with the committed baseline via `diff` (no output), then reran the full suite (23/23 green).

### Fix 2 — `metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated`

Added a `hamming(_:_:) -> Int` helper (`(a ^ b).nonzeroBitCount`) and a new test with three 64x64 images:
- `base`: `(x ^ y) & 0xFF` (structured, non-trivial pattern).
- `nearDuplicate`: same pattern, uniformly brightness-shifted by +12 (clamped at 255) — simulating consecutive burst-frame exposure drift.
- `unrelated`: a coarse quadrant pattern (`(x<32) != (y<32) ? 0 : 255`) sharing no structure with `base`.

Asserts `hamming(base, nearDuplicate) < hamming(base, unrelated)` and `hamming(base, nearDuplicate) < 10`.

**Actual measured values** (via the same throwaway debug test):
```
DEBUG HAMMING: distNear=0 distUnrelated=64
```
`distNear` came out to exactly 0 (the +12 brightness shift didn't flip any of the 64 average-hash bits relative to the base pattern) and `distUnrelated` came out to the maximum possible, 64 — every bit differs. Both are comfortably inside the requested bound (0 < 10) without needing to loosen anything; I did not need to report a higher-than-expected number here.

**Mutation check** — temporarily replaced `Metrics.perceptualHash` with a plain FNV-1a content hash of the raw downscaled pixel buffer (an avalanche hash: any single byte change is expected to flip roughly half the output bits, with no notion of "close" inputs producing "close" hashes):

```
$ swift test --package-path sidecar --filter metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated
```
```
✘ Test metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated() recorded an issue at MetricsTests.swift:161:5:
  Expectation failed: (distNear → 27) < 10
  ↳ hamming(base, nearDuplicate)=27 of 64 bits was not small enough for a brightness-shifted near-duplicate
✘ Test run with 1 test in 0 suites failed after 0.003 seconds with 1 issue.
```
Failed as required — only the absolute-magnitude assertion (line 161) tripped; the ordering assertion (line 158, `distNear < distUnrelated`) happened to still hold under this mutation, since with an avalanche hash both distances are effectively random draws near ~32 and `27 < <whatever distUnrelated was>` can pass by chance. This confirms the **absolute bound is the assertion doing the real work** for this class of bug (a hash that's technically non-constant and content-sensitive, but not perceptually smooth) — the ordering check alone would not have reliably caught it. No threshold was adjusted to produce this failure; the mutation is a stand-in for "wrong algorithm," not the shipped code.

Reverted `Metrics.swift` and confirmed byte-identity with the committed baseline via `diff` (no output), then reran the full suite (23/23 green).

### Final verification

```
$ git diff --stat sidecar/Sources/PhotobookEngine/Metrics.swift
```
(no output — implementation file has no diff from the last commit; only `MetricsTests.swift` changed)

```
$ swift test --package-path sidecar
```
```
Test run with 23 tests in 0 suites passed after ... seconds.
```

### Not fixed, per coordinator's explicit direction

- `sharpEdgesScoreHigherThanFlatField` — left as a cheap sanity check; `metricsTileMaxIgnoresEmptyAreaAroundASharpSubject` now carries the load-bearing assertion about tiling.
- `identicalImagesHaveIdenticalHash` — left as a determinism check; the new Hamming-distance test now covers content-sensitivity.

---

## Fix report — pHash aliasing and palette non-determinism (third follow-up)

Review flagged two defects in the brief, both now addressed to different degrees: Fix 2 (palette determinism) is complete and verified. Fix 1 (pHash aliasing) has its source-level fix implemented and kept — it now matches the documented design ("8x8 average hash over a 32x32 grey downscale") instead of contradicting it — but the deeper behavioural goal (shake-robustness, Hamming distance < 10 for a one-pixel shift) is **not achieved**, and I stopped short of adding a test with that bound rather than commit a test I know fails, per the explicit instruction not to loosen the bound to force a pass. Full data below.

### Fix 2 — palette non-determinism (complete)

**Source change**, `sidecar/Sources/PhotobookEngine/Metrics.swift`, `Metrics.palette`: changed `buckets.values.sorted { $0.n > $1.n }` (iterates `Dictionary.values`, losing the key) to iterating `buckets` as key/value pairs and sorting by `(n descending, key ascending)`:

```swift
return buckets
    .sorted { lhs, rhs in
        if lhs.value.n != rhs.value.n { return lhs.value.n > rhs.value.n }
        return lhs.key < rhs.key
    }
    .prefix(count)
    .map { _, e in ... }
```

**New tests**, `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`:
- `metricsPaletteIsStableAcrossRepeatedCalls` — computes the palette twice on the same image within one process, asserts identical results element-for-element (order included). As the coordinator noted, this alone can't catch hash-seed variation *between* process launches, since the seed is fixed for the lifetime of one process.
- `metricsPaletteBreaksTiesDeterministicallyByBucketKey` — builds a 128x128 image with a clear majority region (10,10,10), and two minority regions of exactly equal pixel count (32 columns each): (200,10,10) with bucket key 48, and (10,200,10) with bucket key 12. Requests `count: 2`, so the tie between the two minority buckets falls exactly at the cutoff. Asserts slot 0 is the majority grey and slot 1 is the smaller-key colour (10,200,10), per the documented ascending-key tie-break.

Name-collision check: `grep -rln "metricsPaletteIsStableAcrossRepeatedCalls\|metricsPaletteBreaksTiesDeterministicallyByBucketKey" sidecar/` returned only the test file itself.

**Both pass under the fix:**
```
$ swift test --package-path sidecar
✔ Test metricsPaletteIsStableAcrossRepeatedCalls() passed after 0.011 seconds.
✔ Test metricsPaletteBreaksTiesDeterministicallyByBucketKey() passed after 0.013 seconds.
Test run with 25 tests in 0 suites passed after 0.054 seconds.
```
Confirmed stable across 5 repeated separate `swift test` launches with the fix in place (all 5 passed).

**Mutation check** — temporarily reverted to the original `buckets.values.sorted { $0.n > $1.n }` (no tie-break) and ran the tie-break test as **10 separate process launches** (not 10 loop iterations within one process — Swift's hash seed is fixed per-process, so the bug can only be observed by relaunching):

```
$ for i in 1..10; do swift test --package-path sidecar --filter metricsPaletteBreaksTiesDeterministicallyByBucketKey; done
```
Result: **3 of 10 launches passed, 7 of 10 failed** — the same code, same input image, same assertion, flipping outcome purely from process-launch hash-seed randomisation. One captured failure:
```
✘ Test metricsPaletteBreaksTiesDeterministicallyByBucketKey() recorded an issue at MetricsTests.swift:163:5:
  Expectation failed: (palette.count > 1 && ... palette[1].g - 200.0/255.0) < 0.05 → false) ...
  ↳ expected slot 1 to be bandC (10,200,10) — the smaller bucket key — under the documented tie-break,
    got PaletteColor(r: 0.7843137254901961, g: 0.0392156862745098, b: 0.0392156862745098, weight: 0.3333333333333333)
```
That result (`r≈0.784, g≈0.039`) is bandB `(200,10,10)`, i.e. the wrong tie-break winner — direct, reproducible evidence of the bug the fix addresses. No threshold was adjusted to produce this; it's the unmodified original implementation.

Reverted to the fixed implementation and confirmed 5/5 consecutive launches pass (shown above), then reran the full suite (25/25 green). `diff` against the saved pre-mutation copy confirmed the reverted file is byte-identical to the fixed version.

### Fix 1 — pHash aliasing (source fix kept; shake-robustness test blocked — reporting, not committing a failing bound)

**Source changes made and kept:**
1. `rgbaBuffer` now sets `ctx.interpolationQuality = .high` before drawing — applies to all four callers (`sharpness`, `palette`, `perceptualHash`, `contrast`), per the instruction to set it generally, not just for the hash path.
2. `perceptualHash` now downscales to 32x32 (not 8x8 directly), then box-averages each non-overlapping 4x4 block into an 8x8 grid, then thresholds against the mean of that grid — matching the brief's documented interface ("8x8 average hash over a 32x32 grey downscale") exactly, where before it point-sampled straight from source resolution to 8x8.

All pre-existing pHash tests (`identicalImagesHaveIdenticalHash`, `differentImagesHaveDifferentHash`, `metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated`) still pass unchanged after this fix — 25/25 full suite green.

**What I was asked to add:** a test with a base image containing fine detail (2px checkerboard), a near-duplicate shifted by exactly one pixel (the shake case), asserting `hamming(base, shaken) < 10`.

**What I measured, before writing any bound into a committed test** (via a throwaway debug test, not committed):

At the literal scenario given (64x64 image, 2px checkerboard, 1px horizontal shift):
```
DEBUG SHAKE: dist=32
```
32 of 64 bits — at the ceiling of what "close to random" looks like, far above 10.

I didn't stop at one data point. Inspecting the intermediate 32x32 buffer directly showed the *unshifted* base image's own downscale isn't a clean two-level checkerboard either — row 0 reads `0 203 57 201 51 206 47 210 42 215 ...`, a decaying oscillation rather than flat 0/255 blocks. That means `CGContext`'s `.high`-quality resampler isn't behaving like a true area/box filter for this decimation — it shows ringing/blur characteristic of a limited-support filter (e.g. bicubic-like), not full-window averaging.

To rule out "this is just a 64x64-test-image artifact," I swept image scale up to production-realistic sizes and swept checkerboard cell size from 2px to 32px, at 1536x1536 (comparable to what `ImageLoader.loadThumbnail` might hand `Metrics`), comparing the **old** point-sample-to-8x8 implementation against the **new** 32x32-then-box-average one:

```
DEBUG cell=2  OLD dist=32  NEW dist=28
DEBUG cell=4  OLD dist=16  NEW dist=18
DEBUG cell=8  OLD dist=20  NEW dist=20
DEBUG cell=16 OLD dist=18  NEW dist=22
DEBUG cell=24 OLD dist=20  NEW dist=2
DEBUG cell=32 OLD dist=16  NEW dist=7
```

Findings:
- The fix only clearly helps at coarse detail (24-32px cells), where NEW drops well under 10 and OLD does not.
- At fine-to-medium detail (2-16px cells) — which is the regime the coordinator specifically asked to test, and plausibly the regime real photo texture/edges/hair/foliage occupy at a downscaled-thumbnail resolution — NEW is statistically indistinguishable from OLD (both 16-28 bits), i.e. the box-averaging second stage doesn't rescue the result, because the damage happens in the **first** stage: the direct `CGContext` downscale from source resolution straight to 32x32 (a 48x decimation at 1536px) is not doing genuine area-averaging regardless of `interpolationQuality`, so it still aliases on fine periodic detail near or above its output Nyquist frequency, and a one-pixel input shift changes which source samples land in each output cell almost arbitrarily.
- Separately, a 2px checkerboard shifted by exactly 1 pixel is a full half-period (phase-inverted) shift of the finest possible single-pixel-alternating pattern — mathematically closer to a worst-case adversarial input than a small perturbation, so some of this result is inherent to that specific construction, not solely an implementation gap. But the 16px-cell result (18-22 bits) shows the problem isn't confined to that pathological edge case.

**I did not**: add a test asserting `< 10` (it would be a known-failing test, which I was told not to force-pass by loosening the bound); implement an un-requested redesign (e.g. progressive/mipmap-style halving downsample, or an explicit box filter in code for the first stage) without sign-off, since that's a different, larger fix than what was specified and Task 12's clustering correctness depends on getting this right; or quietly ship the documented-design-compliant code while implying the shake-robustness goal was met.

**What I'm asking**: whether to (a) implement a true box-filter/progressive-halving downsample for the first stage instead of relying on `CGContext` interpolation (bigger change, not yet attempted), (b) accept a looser/different bound or test construction that's more representative of real photo content than a Nyquist-limit synthetic checkerboard, or (c) defer this specific robustness property to the Phase-1 real-photo validation pass already planned (the same way the min/max-luma-saturation concern was deferred), and track it alongside that. I did not choose any of these unilaterally.

### Commit contents

Committed: the `rgbaBuffer`/`perceptualHash` structural fix (matches documented interface, strictly improves on point-sampling, all existing tests green) and the complete, mutation-verified Fix 2 (palette tie-break + two new tests). Not committed: any new pHash shake-robustness test, pending the guidance above.

---

## Fix report — realistic burst-frame test for pHash (fourth follow-up)

The coordinator confirmed the 2px-checkerboard shake test was a flawed test design (Nyquist-limit phase inversion, not a realistic near-duplicate model) — my own error, not a gap in the fix — and asked for the box-average fix to be kept as-is (no progressive/mipmap redesign), a doc comment recording the known limitation, and a replacement test modelling a real burst frame.

### Doc comment added

Above `Metrics.perceptualHash` in `sidecar/Sources/PhotobookEngine/Metrics.swift`, recording: what the 32x32-then-box-average design achieves (robustness for the coarse structure — roughly 24px-and-up detail at 1536px scale — that real photos are made of); the known, accepted limitation (does not survive a one-pixel shift of near-Nyquist single-pixel-period content, because that is a full phase inversion of the highest frequency present, inherent to average hashing generally, not a defect); and the measured numbers from the earlier checkerboard sweep (28-32 of 64 bits at 1536px even with box-averaging) alongside a pointer to the new realistic test. This is meant to stop a future reader from "discovering" the checkerboard behaviour and mistaking it for a bug.

### New test: `metricsHashHammingDistanceModelsRealisticBurstVariation`

Added to `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`. Builds a 1536x1536 synthetic scene with structure at multiple scales, as specified: a smooth diagonal luminance gradient (~50-190 across the frame), four soft Gaussian "blobs" (sigma 60-100px, i.e. ~200-400px visible extent, soft falloff rather than hard edges) at fixed positions, and a low-amplitude fine texture layer (`10 * sin(x*0.5) * cos(y*0.5)`) so the scene isn't purely smooth. Three variants:
- `shifted` — identical scene translated ~1% of width (15px at 1536px): the camera-shake case.
- `exposed` — identical scene with luminance scaled by 1.1, clamped to 255: the exposure-change case.
- `unrelated` — reversed gradient direction and relocated blobs: structurally different content.

Name-collision check: `grep -rln "metricsHashHammingDistanceModelsRealisticBurstVariation" sidecar/` found only the test file and the new doc-comment reference in `Metrics.swift` (a comment, not a declaration — not a collision).

**Measured, before setting any bound** (via a throwaway debug test, not committed):
```
DEBUG REALISTIC: distShift=0 distExposed=0 distUnrelated=30
```
Both near-duplicate variants hashed **identically** to the base (Hamming distance 0 of 64 bits); the structurally unrelated scene came out at 30 of 64 bits — comfortably far. This is a strong, clean result: at production-realistic scale and realistic multi-scale content, the fixed `perceptualHash` is fully stable under both a ~1%-width shake and a ~10% exposure change.

**Bound set from that measurement**: `distShift < 5` and `distExposed < 5` (comfortable headroom above the observed 0, not tight enough to be flaky, but nowhere near loose enough to mask a regression), plus `distShift < distUnrelated` and `distExposed < distUnrelated` for the ordering property. The threshold's provenance is the `DEBUG REALISTIC` measurement above, not an invented number.

**Full suite passes with the new test in place:**
```
$ swift test --package-path sidecar
✔ Test metricsHashHammingDistanceModelsRealisticBurstVariation() passed after 3.028 seconds.
Test run with 26 tests in 0 suites passed after 3.029 seconds.
```
(The ~3s runtime is generating four 1536x1536 synthetic images pixel-by-pixel in Swift; acceptable for one test.)

**Mutation check** — temporarily replaced `Metrics.perceptualHash` with the same FNV-1a content-hash stand-in used in an earlier round (avalanche hash over the raw pixel buffer, non-perceptual):

```
$ swift test --package-path sidecar --filter metricsHashHammingDistanceModelsRealisticBurstVariation
```
```
✘ Expectation failed: (distShift → 36) < 5
  ↳ hamming(base, shifted)=36 of 64 bits was not small for a ~1%-width camera-shake shift
✘ Expectation failed: (distExposed → 38) < 5
  ↳ hamming(base, exposed)=38 of 64 bits was not small for a ~10% exposure change
✘ Expectation failed: (distShift → 36) < (distUnrelated → 29)
  ↳ hamming(base, shifted)=36 was not clearly smaller than hamming(base, unrelated)=29
✘ Expectation failed: (distExposed → 38) < (distUnrelated → 29)
  ↳ hamming(base, exposed)=38 was not clearly smaller than hamming(base, unrelated)=29
✘ Test run with 1 test in 0 suites failed after 3.020 seconds with 4 issues.
```
All four assertions failed under the mutation — the content hash scrambled shift/exposure variants to distances (36, 38) *larger* than the unrelated-content distance (29), which is exactly what a non-perceptual avalanche hash should do (it has no notion of "similar input"). This confirms the test is genuinely pinning the perceptual property, not passing vacuously. No threshold was adjusted to produce this failure.

Reverted `Metrics.swift` and confirmed byte-identity with the pre-mutation copy via `diff` (no output), then reran the full suite (26/26 green).

### Final state

- `Metrics.perceptualHash`: unchanged from the previous round's fix (32x32 downscale, `.high` interpolation, box-average to 8x8) — kept as directed, no progressive/mipmap redesign attempted.
- New doc comment on `perceptualHash` recording the Nyquist limitation as known and accepted.
- New test `metricsHashHammingDistanceModelsRealisticBurstVariation`, calibrated against measured values (0, 0, 30), mutation-verified against the FNV-1a stand-in.
- The earlier `metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated` test (synthetic `x^y` pattern, brightness-shift near-duplicate) is kept alongside this one — it's a cheaper, simpler regression check; the new test is the one that actually models a realistic burst frame and carries the calibrated bound.
