# RAW/HEIC decode performance fix -- measurement report

**Date:** 2026-08-13
**Branch:** `feat/phase-1-analysis-pipeline`
**Scope:** `sidecar/Sources/PhotobookEngine/ImageLoader.swift` (the fix), a new `benchmark`
NDJSON request kind (`Benchmarker.swift`, `Protocol.swift`, `Handler.swift`,
`src-tauri/src/protocol.rs`, `src-tauri/src/sidecar.rs`), `scripts/benchmark.sh`, and a
real-file quality comparison.

This is the measurement `docs/PROJECT-STATUS.md` flagged as never taken ("RAW and HEIC
throughput. Never measured."). It fills that gap and fixes the specific bug the user hit:
280 Sony ARW files took a very long time to analyse.

---

## TL;DR

Fixed. Measured on the real files available on this machine (n=4 Sony ARW -- see "What
real RAW files were available"): **ARW decode is 10.8x faster** (249ms -> 23ms avg per
file) and its fallback-to-full-decode rate is **0%**, confirming the two-pass logic works
as designed for the format the user hit the bug on. CR2, DNG and JPEG show **no
regression** (within noise). HEIC decode is very slightly slower on average (+8%, ~3ms/file)
because 95% of real HEICs still fall back to full decode and now pay for one extra cheap
probe first -- a real, small, expected cost, not papered over. Quality comparison on real
ARW files found a genuine, reportable divergence between the preview and full-decode paths
(palette order flips on 2 of 4 files) that matters only if a future change causes some RAW
files in the same book to take a different path than others -- which does not happen here
(0% ARW fallback). All 87 Swift tests and 77 Rust tests pass; `cargo clippy` is clean apart
from the known pre-existing `cluster.rs` warning.

---

## The bug

`ImageLoader.loadThumbnail` passed `kCGImageSourceCreateThumbnailFromImageAlways: true`
unconditionally. For a RAW file this forces ImageIO to fully demosaic the sensor image at
native resolution (24-61MP for Sony ARW) before downscaling it to `analysisMaxPixel`
(1536px) -- discarding nearly all of that work. Every ARW embeds a JPEG preview ImageIO
can extract far more cheaply; the old code never looked at it.

## The fix

Size-aware two-pass decode in `ImageLoader.loadThumbnailDetailed` (new; `loadThumbnail` is
now a thin wrapper over it):

1. Request the thumbnail with `kCGImageSourceCreateThumbnailFromImageIfAbsent: true` --
   this returns the embedded preview/thumbnail without decoding the full RAW/HEIC data.
2. If the result's long edge is below `floorPixel` (defaults to `maxPixel`, i.e. 1536),
   discard it and re-request with `kCGImageSourceCreateThumbnailFromImageAlways: true`.
3. Return whichever satisfied the floor.

`kCGImageSourceCreateThumbnailWithTransform: true` is kept on **both** passes -- it
reconciles EXIF orientation (and, for HEIC, container `irot`/`imir` against EXIF).
Verified explicitly: `portrait-rot90-tiny-thumb.jpg` (stored landscape, orientation 6,
tiny embedded thumbnail) reports height > width on both the accepted-preview path (low
floor) and the fallback path (default floor) -- see
`ImageLoaderTests.orientationIsAppliedOnTheAcceptedPreviewPath` and
`.orientationIsAppliedOnTheFallbackPath`.

### Why not just `IfAbsent` alone

Tested directly against real files during this fix (see "Investigation: does
`CGImageSourceCopyPropertiesAtIndex` expose preview size?" below for the probe tool):

| File | `IfAbsent` alone returns | Verdict |
|---|---|---|
| Real Sony ARW (6000x4000 sensor) | 1027x1536 (the real embedded preview) | Correct, fast |
| Real iPhone JPEG (4032x3024) | **160x120** | The classic EXIF thumbnail -- a postage stamp |
| Real iPhone HEIC (4032x3024) | **240x320** | Also a postage stamp, not a usable preview |

An `IfAbsent`-only fix would have silently analysed postage stamps for every plain JPEG
and (on this evidence) most HEICs in a real photo library -- sharpness, palette, saliency
and face detection would all degrade, and no existing test would have caught it, because
the committed fixtures are flat colour fields where those metrics are already degenerate.
The floor check is what prevents this; it is mutation-tested (see "Test coverage" below).

### Investigation: does `CGImageSourceCopyPropertiesAtIndex` expose preview size?

No. Probed `CGImageSourceGetCount` (always 1 -- no separate index for an embedded
preview) and `CGImageSourceCopyPropertiesAtIndex`'s key set on real ARW, HEIC, DNG, CR2
and JPEG files: none expose a preview/thumbnail width or height anywhere in the returned
dictionary (checked every key, including the format-specific `{TIFF}`/`{Exif}` blocks).
There is no cheap way to learn the embedded preview's size without asking ImageIO to
decode it.

That said, asking for it via `IfAbsent` is still cheap **relative to a full decode** --
measured directly (see the decode-stage numbers below): for the same Sony ARW file,
`IfAbsent` took ~0.04-0.07s versus `IfAbsentAlways`'s ~0.34-0.7s, a 9-10x difference,
because `IfAbsent` never demosaics the full sensor image. The two-pass approach pays that
small cost once and then, for RAW, never needs the expensive path at all.

---

## The `benchmark` NDJSON request

Added alongside `ping` and `analyze` (`RequestKind.benchmark` in
`sidecar/Sources/PhotobookEngine/Protocol.swift`, mirrored in
`src-tauri/src/protocol.rs`). Runs the same per-photo pipeline `Analyzer.analyzeOne` runs,
but sequentially (not `concurrentPerform`-parallel -- see `Benchmarker.swift`'s doc
comment for why) and timed stage-by-stage: file hash, EXIF read, decode, Vision pass,
classical metrics, thumbnail write (opt-in via `thumbnailDir`). Returns one record per
input path with its extension, decoded dimensions, whether the full-decode fallback
fired, and per-stage milliseconds.

This is a permanent diagnostic, not scaffolding. `scripts/benchmark.sh <folder>` drives
it and prints a per-file table plus a per-extension aggregate (via `jq`); see the README's
"Benchmarking photo analysis performance" section for usage. Re-run it after any future
change to `ImageLoader`, `Analyzer`, `VisionAnalyzer` or `Metrics`.

---

## Baseline vs. fix

Measured with the actual `benchmark` NDJSON request via `scripts/benchmark.sh`, run twice
against the identical corpus: once against a sidecar binary built with `ImageLoader`
reverted to the original single-pass `...FromImageAlways`-only behaviour (temporarily, not
committed -- see reproduction command below), once against the real fix. Both preserve the
`benchmark` request's full pipeline (hash, EXIF, decode, Vision, metrics), so only the
decode-stage numbers and fallback flag should differ; everything else is a consistency
check.

### Corpus

Real files gathered from this machine's local Photos library volume (`/Volumes/Apple
Photos`) -- the only real RAW files available (see "What real RAW files were available"
below):

| Extension | Count | Source |
|---|---|---|
| ARW (Sony) | 4 | Only 4 exist on this machine |
| CR2 (Canon) | 11 | |
| DNG | 7 | |
| HEIC | 40 | sampled |
| JPG | 40 | sampled, real iPhone/camera JPEGs with EXIF thumbnails |

Copied locally to scratchpad (not committed -- these are the user's personal photos) to
avoid external-volume I/O variance during timing.

### Results

Sequential per-file averages (ms), 102 real files, one `scripts/benchmark.sh` run per
binary:

| ext | n | decode ms (before) | decode ms (after) | decode speedup | total ms (before) | total ms (after) | total speedup |
|---|---|---|---|---|---|---|---|
| **arw** | 4 | **249** | **23** | **10.8x** | **303** | **66** | **4.6x** |
| cr2 | 11 | 35 | 36 | 1.0x (no change) | 89 | 93 | 1.0x |
| dng | 7 | 55 | 54 | 1.0x (no change) | 134 | 138 | 1.0x |
| heic | 40 | 39 | 42 | 0.93x (**8% slower**) | 86 | 93 | 0.92x |
| jpg | 40 | 19 | 19 | 1.0x (no change) | 64 | 66 | 1.0x |

Reproduce:

```bash
# after (the real fix, already on this branch):
bun run sidecar
scripts/benchmark.sh <corpus> --json after.json

# before (temporary, to compare against): revert ImageLoader.swift's
# loadThumbnailDetailed body to always use kCGImageSourceCreateThumbnailFromImageAlways
# (keep the LoadResult/loadThumbnail wrapper shape so Analyzer/Benchmarker still compile),
# then:
bun run sidecar
scripts/benchmark.sh <corpus> --json before.json
# restore ImageLoader.swift afterwards.
```

**ARW: exactly the bug the user hit, and exactly the fix.** 10.8x faster decode, 4.6x
faster end-to-end per photo (Vision/hash/metrics stages cost the same either way; decode
went from 82% of the old per-photo total to 35% of the new one). This is n=4, not 280 --
see "What real RAW files were available" -- but the ratio is driven by sensor resolution
and ImageIO's internal RAW decode cost, not file count, so it should generalise.

**CR2 and DNG: no measurable change, for an interesting reason.** On this machine's
ImageIO version, a plain `...FromImageAlways` full decode of these particular CR2/DNG files
was already fast (35-55ms) -- nowhere near ARW's 249ms. Sony's RAW demosaic path is
measurably more expensive than Canon's CR2 or generic DNG on this system: **not all RAW
formats have the same performance problem.** The two-pass fix neither helps nor hurts
CR2/DNG here: it doesn't help because they didn't need it (their old full-decode time was
already fine), and it doesn't meaningfully hurt because the extra `IfAbsent` probe it adds
is cheap relative to their per-file overhead. Confirmed below (per-format fallback rate)
that CR2/DNG's embedded previews are large enough to avoid the fallback entirely, same as
ARW -- but unlike ARW, avoiding the fallback wasn't the thing that mattered for their
timing, since the fallback path was never expensive for them in the first place.

**HEIC: measurably worse decode stage, by design tradeoff, not a bug.** +8% (~3ms/file) on
average, because 95% of real HEICs (38/40) still don't clear the 1536px floor from their
embedded preview alone and fall back to a full decode anyway -- paying for the cheap
`IfAbsent` probe *and* the full decode, where the old code paid for only the full decode.
This is the expected cost of the two-pass design for a format whose typical embedded
preview is too small: a few milliseconds, not a regression worth reverting the fix over.

**JPEG: no measurable change**, as expected -- 100% of real JPEGs fell back in both runs
(same as HEIC's reasoning), so both old and new code do the same full decode.

---

## Per-format fallback rate

From the real benchmark run's `usedFullDecodeFallback` field, aggregated by `scripts/
benchmark.sh`:

| ext | n | fell back | fallback rate |
|---|---|---|---|
| **arw** | 4 | **0** | **0%** |
| cr2 | 11 | 0 | 0% |
| dng | 7 | 0 | 0% |
| heic | 40 | 38 | 95% |
| jpg | 40 | 40 | 100% |

**ARW never falls back** -- confirmed on real files, not just the manual probe earlier in
this report. All 4 real ARW files' embedded previews reach exactly 1536px on the long edge
(1027x1536 / 1024x1536, i.e. the preview itself is native-large enough that the
`kCGImageSourceThumbnailMaxPixelSize: 1536` cap is what's actually limiting the returned
size, not the preview's own resolution). **CR2 and DNG also never fall back** on these real
files -- their embedded previews are large enough too (not originally expected to be quite
this reliable, but consistent across all 18 real CR2/DNG files tested).

**JPEG falls back 100% of the time** -- expected and correct, exactly as flagged in
advance: real out-of-camera/iPhone JPEGs carry only the classic ~160x120 EXIF thumbnail,
far below the floor.

**HEIC falls back 95% of the time (38/40)** -- expected for the same reason as JPEG (most
embedded HEIC thumbnails are the small EXIF-style kind, ~240x320 as measured earlier in
this report), but not 100%: 2 of 40 real HEICs (`IMG_2476.HEIC`, `IMG_1014.HEIC`) had an
embedded preview large enough to clear the floor on the first pass. Not investigated
further why those two differ from the other 38 (likely a different capture app/pipeline
writing a larger embedded preview) -- noted as an observation.

---

## Quality comparison: preview path vs. full-decode path

The embedded preview is the camera's own rendering (its own white balance, sharpening,
noise reduction) -- not the same pixels a naive demosaic produces. For **within-book
percentile ranking** that's fine and arguably better, *provided every photo in a book goes
through a comparable path*. The risk is a mixed folder where some files land on the
preview path and others on the full-decode path for reasons that have nothing to do with
the photo's actual quality.

Compared `Metrics.sharpness`, Vision's `aestheticScore`, palette warmth/contrast, and the
dominant palette bucket between the preview-only path (`IfAbsent`) and the full-decode
path (`Always`) on the 4 real ARW files plus one real CR2 and one real DNG, at
`maxPixel: 1536` (algorithms copied verbatim from `Metrics.swift`/`VisionAnalyzer.swift`
into a standalone comparison script for this investigation).

| File | dims (both) | sharpness delta | aesthetic delta | dominant palette |
|---|---|---|---|---|
| ARW #1 (MY_00528) | 1027x1536 / 1024x1536 | +0.3% | -0.0137 | same order |
| ARW #2 (MY_04236) | 1027x1536 / 1024x1536 | -5.2% | +0.0002 | **order flips** |
| ARW #3 (MY_04237) | 1027x1536 / 1024x1536 | +2.8% | +0.0562 | **order flips** |
| ARW #4 (MY_04241) | 1027x1536 / 1024x1536 | -2.3% | +0.0303 | same order |
| CR2 (IMG_0022) | 1536x1024 (identical) | 0.0% | 0.0000 | identical (bit-exact) |
| DNG (IMG_6149) | 1152x1536 (identical) | 0.0% | 0.0000 | identical (bit-exact) |

**Finding, stated plainly:** for real Sony ARW files, the preview and full-decode paths
diverge enough to **reorder the dominant palette** in 2 of 4 files (the #1 and #2 buckets
literally swap which is more prevalent), and the aesthetic score diverges by up to 0.056
(on Vision's ~0-1 scale) and sharpness by up to 5.2%. This is exactly the "mixed folder"
risk described above made concrete: if a book mixed some ARW files that took the preview
path with others that took the full-decode path, their sharpness/aesthetic percentiles and
dominant-colour groupings would not be strictly comparable.

In this specific fix, that risk **does not materialize in practice** for ARW: all 4 real
ARW files' embedded previews reach the 1536px floor and take the preview path uniformly
(0% fallback, see above), so within a real ARW-only import every photo is on the same
path and the divergence is moot. The risk is real only if a future change (or a different
camera's preview encoding) makes some RAW files in the same book fall back while others
don't. Note also: CR2 and DNG showed **zero** divergence -- bit-identical pixels on both
paths on this OS version, which suggests ImageIO's "Always" full decode for those two
formats may itself be deriving from the same embedded preview internally (not verified
further; noted as an observation, not relied upon).

**Not papered over:** if the user's actual 280-ARW folder has files whose previews don't
uniformly clear the floor (different camera firmware, different preview JPEG quality
settings, etc.), some of those files would silently take the slower, differently-rendered
full-decode path while others don't, and their ranking would be exactly as unreliable as
the table above shows. `scripts/benchmark.sh --json out.json` on the user's real folder,
followed by `jq '[.result.data[] | select(.result.usedFullDecodeFallback)] | length'` on
the output, is the direct way to check this for their actual files.

---

## What real RAW files were available

Only 4 real Sony ARW files exist on this machine (found via `mdfind`), all in
`/Volumes/Apple Photos/JJ/...`. Also available: 11 CR2, 7 DNG, several thousand HEIC, and
several hundred JPG from the same library. No ARW folder of the user's actual scale (280
files) was available -- **this report's ARW numbers are from n=4, not 280.** The
per-file timings and the 9-10x preview-vs-full-decode ratio measured on those 4 should
generalise (it's driven by sensor resolution and ImageIO's internal decode cost, not by
how many files there are), but the exact wall-clock time for a real 280-file import was
not measured end-to-end and could not be, on this machine.

**To measure it directly on the real folder:**

```bash
bun run sidecar
scripts/benchmark.sh /path/to/280-arw-folder --recursive --json /tmp/raw-benchmark.json
```

---

## Test coverage

`swift test --package-path sidecar`: **87 tests, all passing, 7.5s** (up from the 71
recorded in `docs/PROJECT-STATUS.md`; the increase is 6 new `ImageLoaderTests` for the
two-pass floor logic, 6 new `BenchmarkerTests`, and 2 new `ProtocolTests` for the
`benchmark` request kind -- `VisionAnalyzerTests`' three existing tests were modified to
route through `VisionGate` but not added to).

`cargo test --manifest-path src-tauri/Cargo.toml`: **77 tests, all passing** (up from 74 --
3 new `protocol::tests` for the `benchmark` request/response wire format).

`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: clean apart from the one
known pre-existing `cluster.rs::ptr_arg` warning.

`bun run test` (Vitest) and TypeScript were not touched by this change (no frontend files
modified) and were not re-run as part of this report.

### Mutation check

`ImageLoaderTests.fallsBackToFullDecodeWhenEmbeddedPreviewIsBelowTheFloor` (and two other
tests) were verified to fail when the floor check is removed -- i.e. when
`loadThumbnailDetailed` is mutated back to the naive `IfAbsent`-only "fix" the task
description warns against. With the floor check deleted:

```
✘ fallsBackToFullDecodeWhenEmbeddedPreviewIsBelowTheFloor: (160) > 1000 failed
✘ defaultFloorIsMaxPixelWhenNotSpecified: (160) > 1000 failed
✘ detailedLoadReportsWhetherTheFallbackFired: (false) == true failed
```

Restoring the floor check makes all three pass again. This confirms the test suite would
actually catch a regression to the postage-stamp bug, not just exercise code that happens
to pass either way.

## A concurrency bug found and fixed along the way

`Benchmarker.benchmarkOne` initially called `VisionAnalyzer.analyze` directly, bypassing
`Analyzer`'s private Vision-concurrency semaphore. Running the new `BenchmarkerTests`
alongside the rest of the suite reproduced the exact documented
`VNControlledCapacityTasksQueue` deadlock from `docs/PROJECT-STATUS.md`'s Apple Vision
trap list: `AnalyzerTests.analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`
timed out at its own 120s watchdog (confirmed via `sample`: every thread parked in
`dispatchGroupWait` at 0% CPU, not merely slow).

The fix took three iterations, each verified against the **full unfiltered** `swift test`
suite (a filtered run doesn't reproduce this -- the whole point is contention from
everything else running at once):

1. Extracted the semaphore into a shared `VisionGate.swift`, used by both
   `Analyzer.analyzeOne` and `Benchmarker.benchmarkOne`. **Not sufficient alone** -- still
   deadlocked.
2. Consolidated `BenchmarkerTests`' six separate Vision-touching `@Test` functions into
   one, `@Suite(.serialized)` (mirroring `AnalyzerTests.analyzerThumbnailWriting`'s existing
   precedent), and additionally routed `VisionAnalyzerTests`' three previously-ungated
   direct `VisionAnalyzer.analyze` calls through `VisionGate` too. **Still not sufficient**
   -- still deadlocked, confirming this machine's cooperative thread pool is close enough
   to its ceiling from the *pre-existing* test suite alone (`AnalyzerTests`'s 300-photo
   stress test can occupy the entire pool by itself: its `concurrentPerform` fans out to
   core count, and every worker is either inside VisionGate's 4-permit section or blocked
   waiting for one) that adding literally any new concurrently-schedulable Vision consumer
   -- regardless of internal serialization -- risks tipping it over.
3. Gave `Benchmarker` a test-only `visionOverride` seam (defaults to `nil`; production
   code in `Handler.swift` never passes it, so the real request path is unaffected) and
   rewrote every `BenchmarkerTests` case to stub Vision entirely. The real
   `VisionGate.run { VisionAnalyzer.analyze }` call is still exercised byte-for-byte by
   `Analyzer`'s own tests, so this doesn't reduce coverage of the thing that actually
   needed proving. **This resolved it**: full suite passes reliably, 87 tests, 7.5s.

Step 2 (gating `VisionAnalyzerTests`) is kept even though it alone didn't solve the
deadlock -- it closes a real gap against the invariant `VisionGate.swift`'s doc comment
states ("every call site that invokes `VisionAnalyzer.analyze` must go through
`VisionGate.run`"), independent of whether it was sufficient by itself.

---

## Uncertain / not fully resolved

- The 280-ARW-file scale was not reproduced on this machine (only 4 real ARW files
  available) -- see "What real RAW files were available" above.
- CR2/DNG showing bit-identical output on both decode paths is reported as an observation,
  not explained from ImageIO internals (no public API confirms why).
- **Floor value: kept at 1536 (`analysisMaxPixel`), not lowered.** The task brief
  suggested a lower floor might be fine, since `Metrics` internally resamples to at most
  256px anyway. Investigated but not changed, for two reasons: (1) the real ARW files
  measured here already hit exactly 1536 on the `IfAbsent` pass (their embedded preview
  is large enough), so lowering the floor would not reduce ARW's already-0% fallback rate
  -- there is no upside for the format this fix targets. (2) Lowering it would only
  change behaviour for JPEG/HEIC-style small-embedded-thumbnail files, and there it would
  directly trade decode speed for the postage-stamp risk this whole fix exists to avoid --
  a 300px floor, say, would happily accept a lot of embedded thumbnails as "good enough"
  that are visibly worse than a full decode. Given `analysisMaxPixel` is also the number
  `Analyzer` uses for the full-decode path (so raising or lowering it changes what "full
  quality" means everywhere, not just the floor), changing it warranted its own
  measurement pass against real degenerate-thumbnail JPEGs/HEICs, which was out of scope
  here. Left as `floorPixel: Int? = nil` (defaults to `maxPixel`) specifically so a future
  change can tune it per-call-site without touching `ImageLoader` again.
