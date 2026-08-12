# PhotobookGen — Project Status

**Last updated:** 2026-08-12
**Branch:** `feat/phase-1-analysis-pipeline` (52+ commits ahead of `master`, not yet merged)

This document exists so a new agent can pick the project up without re-deriving what
was already learned. Read it before touching code. The two authoritative documents are:

- `docs/superpowers/specs/2026-08-12-photobook-generator-design.md` — the design and its
  constraints. Still accurate.
- `docs/superpowers/plans/2026-08-12-phase-1-analysis-pipeline.md` — the Phase 1 plan.
  **Sections of it are wrong and carry `⚠️ SUPERSEDED` warnings.** Heed them; following the
  original text reintroduces two real bugs.

A blow-by-blow record of every fix round, ruling, and deferred finding is in
`.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/progress.md` (git-ignored, local
only). It is long but it is where the reasoning lives.

---

## What this is

A macOS-only desktop app that turns a folder of personal photos into a print-ready
photobook. It analyses each photo on-device, groups them into chapters and spreads,
scores a curated template library to pick a layout per spread, and exports colour-managed
print files. The user uploads those files to Pixajoy manually. The app never talks to
Pixajoy.

**Stack:** Tauri 2 + Nuxt 4 + a long-lived Swift sidecar, arm64 only, macOS 15+.

Hard constraint, load-bearing for the whole design: **images never leave the machine.**
All pixel analysis is on-device. Only derived JSON may ever be sent anywhere. This is why
there is no hosted vision model in the design, and why "sentiment" has to come from
Apple Vision plus local embeddings rather than a VLM.

---

## Current state: Phase 1 is complete

All 15 planned tasks are done and reviewed, plus four unplanned additions.

**Working end to end:** point the app at a folder → it hashes each file, consults a SQLite
cache, sends only cache misses to the Swift sidecar, runs Apple Vision (aesthetics,
`isUtility`, faces with pose and capture quality, saliency, horizon, scene tags, text
detection) plus classical metrics (tile-max sharpness, Oklab-ish palette, perceptual
hash) plus a landmark-geometry smile proxy → derives near-duplicate clusters, event
clusters and within-book percentile ranks → renders a photo-first contact sheet with
thumbnails, chapter dividers and burst-size badges.

| Layer | Where | Notes |
|---|---|---|
| Swift sidecar | `sidecar/Sources/PhotobookEngine/` | NDJSON over stdin/stdout, standalone CLI, testable with `cat fixtures.ndjson \| ./PhotobookEngine` |
| Rust backend | `src-tauri/src/` | `commands.rs` `db.rs` `cluster.rs` `ranking.rs` `sidecar.rs` `protocol.rs` |
| Nuxt UI | `app/` | `pages/index.vue`, `components/PhotoTile.vue`, `composables/useAnalysis.ts`, `types/features.ts` |
| Template library | `templates/` | 40 spread templates + validator at `tests/templates.test.ts` |

**Test counts at last run:** 425 TypeScript, 74 Rust, 71 Swift. All green, lint clean.
**Release build works:** `bun tauri build --bundles app` produces `PhotobookGen.app`.

### Unplanned additions beyond the plan

- **Thumbnails.** The sidecar writes a 400px JPEG per photo keyed by content hash and
  returns its path; the webview loads it via `convertFileSrc` and the asset protocol.
  Without this the UI showed a table of file paths, which is useless for judging photos.
- **Contact-sheet UI + custom theme.** Steel blue primary, cool grey neutrals, with
  lavender reserved for structural dividers and pastel yellow for the hero/keeper marker.
  Palette derives from the app icon render.
- **App icon.** `app-icon.png` is masked to Apple's macOS squircle (superellipse n=5,
  824px inside a 1024px canvas). Regenerate the set with `bun tauri icon app-icon.png`.
- **Completion notification** for long analysis runs, posted from Rust so no JS is
  involved. Gated on two conditions: elapsed ≥ `NOTIFY_MIN_ELAPSED` (10s) **and** the
  window is unfocused. Failure is non-fatal — a user who declines notifications still
  gets their analysis.

---

## What is NOT built

Phases 2 through 5 of the design. Each needs its own spec-then-plan cycle; do not start
implementing from the design doc alone.

| Phase | Scope | Blocked on |
|---|---|---|
| **2** | Layout engine + colour-managed export. Scores the 40 templates against photo features, picks a layout per spread, renders 300 DPI spreads. **This is the phase that first produces an uploadable book.** | Nothing. Next thing to do. |
| **3** | Spread preview UI + spread-level controls (regenerate, swap, lock, reject) | Phase 2 |
| **4** | Canvas editor: drag/resize/crop with snapping to the margin guides | Phase 3 |
| **5** | AI SDK v7 + DeepSeek chat agent driving the layout tools | Phases 2-4 |

Also explicitly deferred:

- **SigLIP 2 zero-shot mood axes.** Needs a Core ML conversion, a designed
  evidence-grounded vocabulary, and measurement against ~200 real photos. The design doc
  names it the weakest signal in the pipeline; everything works without it.
- **Palette harmony scoring** (Matsuda hue templates). Belongs with the Phase 2 scorer
  that consumes it.
- **GPS location clustering.** Only time-gap event clustering exists.
- **Same-person face clustering.** Blocked on licensing: InsightFace/ArcFace is
  non-commercial research-only including its auto-downloaded weights. Face *detection* is
  unaffected. Do not reach for InsightFace.
- **The cover.** Different geometry from the interior (~0.75" wrap band plus a spine whose
  width depends on page count and paper stock). Own spec.

---

## Print geometry (settled, and hard-won)

Derived by pixel-measuring the guide lines in Pixajoy editor screenshots. The saved
editor HTML is a Konva canvas snapshot; the layout config is fetched from their API at
runtime and is not in the files.

| | Value |
|---|---|
| Trim, per page | 11.000" × 8.500" |
| Bleed, outer edges | 0.197" (5 mm) |
| **Spread canvas** | **22.394" × 8.894"** = 6718 × 2668 px @ 300 DPI |
| Fold centre | x = 11.197" (normalised 0.5) |
| Gutter dead band | x 11.000"–11.394" (normalised 0.491203–0.508797) |
| Safe area | x 0.008797–0.991203, y 0.022150–0.977850 (normalised) |

**Template contract, validator-enforced:** `rect` values are normalised to the spread
canvas, but **`aspect_pref` is a real-world (inch) aspect ratio**, not the normalised rect
ratio. The canvas is 2.518:1, so a Phase 2 scorer that assumes normalised ratios will
mis-score every slot. Verified numerically: 102 of 103 slots have their real-world ratio
inside their declared range; zero have their normalised ratio inside it.

Every template is **exact-count** (`min_photos == max_photos == slots.length`), so the
Phase 2 packer selects templates by photo count rather than fitting a range.

---

## Verified vs unverified

Be precise about this. Several things look verified and are not.

### Genuinely verified
- Crash isolation on malformed input. Eight hostile fixtures (zero bytes, truncation,
  no header, corrupt Huffman scan data, 1×1, CMYK, no extension, directory path,
  permission denied) all produce error records; none crash the process.
- The Swift↔Rust wire format, pinned from both sides. Swift side uses
  `JSONSerialization` rather than its own `Decodable`, so a shared key-name typo cannot
  pass.
- `analyze_batches` returns exactly one record per input path under retry, total failure,
  and wrong-count responses — asserted by *offset*, with the path encoded in each record.
- The tokio starvation fix, via an integration test on a **1-worker-thread runtime**
  against the real sidecar binary, mutation-verified.
- Contrast ratios for every accent-bearing element in both light and dark mode.

### NOT verified — do these first when you have real photos
1. **RAW and HEIC throughput.** Never measured. The design's §14 flags it. Timeouts are
   deliberately generous (`10s + 3s/photo`) precisely because nobody knows.
2. **🚨 Vision's `outerLips` point count.** `SmileProxy` requires **at least 6** points.
   Apple's headers document only the *total* constellation (65 or 76 across all regions);
   no per-region count is published anywhere. **If Vision returns fewer than 6,
   `smile_fraction` is `nil` for every photo in the library — silently, with every test
   still green.** Check the real count first thing.
3. **The whole face code path.** No fixture contains a face, so landmark seeding,
   `FaceObservation` construction and the landmark coordinate conversion are never
   executed by any test. A synthetic drawn face will not reliably trip Vision's detector,
   so this needs real photos.
4. **Pixajoy's page-count semantics.** Does "20 pages" mean 10 spreads or 20? Open the
   editor and count thumbnails in the Pages panel. It lives in a product profile so it is
   a one-value change, but the capacity math doubles or halves on it.
5. **Whether the red editor guide is the trim line or a safe-text margin.** The distance
   is 5 mm either way and the practical rule is identical, so this is cosmetic — but it is
   unresolved.
6. **CSP and asset protocol in a packaged bundle.** Verified in `tauri dev` only.
7. **The notification banner actually rendering.** macOS showed its genuine first-run
   permission prompt for `PhotobookGen`, but nobody could click Allow (no Accessibility
   access, and blind-clicking a live desktop was correctly refused). The gating logic and
   non-fatal failure path are unit-tested; the banner itself has never been seen. Grant
   permission on first run and confirm.

---

## Traps that will bite you

Each of these was a real bug found during Phase 1. They are documented because the
tests did not catch them and a fresh reader would repeat them.

**Apple Vision**
- `VNFaceLandmarkRegion2D.normalizedPoints` are normalised to the **face's bounding box**,
  not the image. Offset and scale by the Vision-space (bottom-left) box, then flip. Do not
  mix the already-flipped box into the offset.
- Landmark points must come from **`outerLips`**, never `allPoints`. `allPoints` spans jaw
  to forehead, so min/max x are ear/jaw contour points and the smile signal *inverts*.
- Seed `VNDetectFaceLandmarksRequest` and `VNDetectFaceCaptureQualityRequest` with
  `inputFaceObservations`. Running them standalone and pairing result arrays by index
  attaches one person's quality score to another person's box.
- Use one `VNImageRequestHandler` for everything; it reuses the decoded surface.
- Vision **deadlocks** on `VNControlledCapacityTasksQueue` under many simultaneous
  handlers. `Analyzer.visionSemaphore` caps in-flight Vision work at 4, guarding **only**
  the Vision call — metrics run unguarded. Narrowing that guard was worth about a third of
  batch time. `@Suite(.serialized)` on `HostileInputTests` is a **separate** fix for a
  test-parallelism variant; neither is redundant.
- Vision has **no expression classifier** at any macOS version. That is why the smile
  proxy is geometric.

**ImageIO / EXIF**
- EXIF orientations 5–8 swap width and height. Compute layout boxes *after* orienting or
  every portrait photo lands in a landscape slot, silently.
- HEIC orientation lives in both container `irot`/`imir` and EXIF, and they can disagree.
  `kCGImageSourceCreateThumbnailWithTransform` reconciles them.
- Pin `DateFormatter.locale` to `en_US_POSIX`. Unpinned, a non-Gregorian system calendar
  makes `date(from:)` silently return nil and every capture date vanishes.
- The `autoreleasepool` must enclose the **decode**, not just the analysis.
  `loadThumbnail` forces full decompression, and `concurrentPerform` worker threads have
  no pool of their own.

**Rust / Tauri**
- `Sidecar::request` blocks. Call it inside `spawn_blocking`, or it starves the tokio
  worker its own stdout-drain task needs, and `analyze_folder` hangs until timeout.
- `bundle.resources` binaries are **not** codesigned; `externalBin` are. The sidecar must
  ship via `externalBin`, with the target-triple filename suffix.
- `bun run sidecar` must precede `cargo build`/`cargo test` — `tauri-build` validates the
  `externalBin` path at compile time. It is wired into the Tauri hooks but not into bare
  cargo invocations.
- Strip `phash` before returning to the webview; it is 64-bit and JavaScript loses
  precision above 2^53.
- `event_clusters` and `percentiles` return values in **input order**, not sorted order.
  Callers zip them positionally.

**Testing**
- **swift-testing's top-level `@Test func`s share one module namespace.** A generic name
  is a compile error, not a test failure. Prefix by area.
- `@Test(.timeLimit(...))` cannot fire on a synchronous body — cooperative cancellation
  needs a suspension point. And a `DispatchQueue.global()` watchdog shares the pool a
  deadlock exhausts. Use a raw `Thread`.
- **Non-square fixtures are mandatory** when testing anything aspect-dependent; a square
  box makes `height/width == 1` and hides the whole class of bug.
- A zero-width box can "pass" a guard test via NaN propagation. Use a negative width.

---

## Build, run, test

```sh
bun install
bun run sidecar        # REQUIRED before any cargo command
bun run dev            # dev app
bun tauri build --bundles app

bun run test           # TypeScript (vitest)
bun run test:rust      # cargo test
bun run test:swift     # swift test
bun run lint           # oxlint, warnings are failures
```

`.env` is gitignored and holds `DEEPSEEK_API_KEY` for Phase 5. **It will not work in a
packaged app** — `envPrefix` is `["VITE_","TAURI_"]` and a shipped `.app` has no `.env`
beside it. The design calls for the OS keychain, read from Rust.

---

## Known smaller debts

- `AnalysisSummary.photos` is `Vec<serde_json::Value>` with no compile-time link to the
  TypeScript `AnalyzedPhoto`. A field rename on either side is a silent `undefined`.
  Consider generated types before Phase 2 widens this boundary.
- **`count_keepers` in Rust duplicates `keepers()` in TypeScript.** Both decide which
  photo survives a near-duplicate cluster, in two languages, with no shared definition.
  The Rust copy exists only to put a number in the completion notification. If the culling
  rule changes in one and not the other, the notification quietly disagrees with the
  screen. Worth collapsing to one authority when Phase 2 touches culling.
- `.oxlintrc.json` enables only `correctness` and `suspicious`, so `no-explicit-any` is
  off and the "never use `any`" convention rests on discipline.
- No `typecheck` script; `nuxi typecheck` and `vue-tsc` both fail on environment issues.
- Nothing pins the build to `aarch64-apple-darwin`. No universal config exists, so the
  constraint is not violated, but it is not enforced either.
- `read_dir` is non-recursive. Real photo exports are often nested.
- macOS AppleDouble files (`._IMG_1234.JPG`) pass the extension check and surface as
  spurious failures.
- Two byte-identical files in the *same* scan both go to the sidecar; the cache only
  dedupes across runs.
- `SidecarPool::analyze_all`'s respawn-on-crash path is untested — it needs a live
  `AppHandle` and a trait abstraction was judged not worth it.
- `contrast` and the per-tile sharpness normalisation use raw min/max luma, which one hot
  pixel or specular highlight can saturate. Real degradation on real photos, invisible to
  synthetic fixtures. **Measure at the real-photo run before choosing a fix.**

---

## Immediate next steps

1. **Run the app against your own photos.** This settles items 1–3 in the unverified list
   at once, and item 2 (the `outerLips` count) can silently disable smile detection
   library-wide.
2. **Answer the Pixajoy page-count question** — 30 seconds in their editor.
3. **Spec and plan Phase 2** (layout engine + export). The 40 templates and the geometry
   are ready; the scorer, the packer and the Core Graphics renderer are not. End of Phase 2
   is the first point at which this project produces an actual printable book, which makes
   it the highest-value thing remaining.
