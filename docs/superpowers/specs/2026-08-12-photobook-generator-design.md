# Photobook Generator — Design

**Date:** 2026-08-12
**Status:** Approved, pending implementation plan

## 1. Purpose

A macOS desktop application that turns a folder of personal photos into a print-ready
photobook. It analyses each photo on-device, groups the set into chapters and spreads,
scores a curated template library to pick a layout per spread, and exports colour-managed
print files.

The application is **only** the layout builder. Exported files are uploaded to Pixajoy
manually through their "Upload Own Design" flow. Nothing in this project automates,
reverse-engineers, or integrates with the Pixajoy editor.

## 2. Constraints

These are settled and should not be relitigated during implementation.

| Constraint | Value |
|---|---|
| Platform | macOS only, arm64, macOS 15+ (Vision aesthetics requires 15.0) |
| Stack | Tauri 2 + Nuxt 4 + Swift sidecar, following `~/Documents/luna-ultra-desktop` conventions |
| Scale | 100–300 photos in, 10–20 spreads out |
| Input formats | HEIC, JPEG, PNG, camera RAW (CR2/NEF/ARW/DNG) |
| Text on pages | EXIF date/location stamps, plus user-typed text boxes. **No AI-written captions.** |
| Privacy | **Images never leave the machine.** All pixel analysis is on-device. Only a minimum-disclosure derived payload may be sent to a hosted LLM. |
| Hosted LLM | DeepSeek (`deepseek-flash`, V4.1 Flash), text-only, reasoning over derived JSON |
| Layout approach | Curated template library, scored against photo features |
| Product SKUs | 20 pages or 40 pages; the app recommends which based on keeper count |

Conventions inherited from luna-ultra-desktop: Bun, `ssr: false`, `@nuxt/ui` v4,
Tailwind 4, oxlint/oxfmt (warnings are failures), vitest, Conventional Commits, TDD.

## 3. Print geometry

Derived on 2026-08-12 by pixel-measuring the red guide lines in Pixajoy editor
screenshots. The saved editor HTML is a Konva canvas snapshot; the layout config is
fetched from Pixajoy's API at runtime and is not present in the files.

Measurement: page canvas 700.75 × 545.5 px, guide inset a uniform **12.0 px on all four
edges and both sides of the fold**. Guide rectangle 676.5 × 521 px → aspect 1.2985
against 11/8.5 = 1.2941, placing the scale at ~61.3 px/inch, so 12 px ≈ **0.197" (5 mm)**.

| | Inches | px @ 300 DPI |
|---|---|---|
| Trim, per page | 11.000 × 8.500 | 3300 × 2550 |
| Bleed, outer edges | 0.197 (5 mm) | 59 |
| Page canvas | 11.394 × 8.894 | 3418 × 2668 |
| **Spread canvas** | **22.394 × 8.894** | **6718 × 2668** |
| Trim rectangle within spread canvas | x 0.197→22.197, y 0.197→8.697 | — |
| Fold centre | x = 11.197 | x = 3359 |
| Gutter dead band | x 11.000→11.394 | 3300→3418 |

Bleed applies to the outer, top and bottom edges of a spread — never at the fold.

**Unresolved:** whether the red guide is the trim line (canvas = trim + bleed) or a
safe-text margin (canvas = trim). The distance is 5 mm either way and the practical rule
is identical: keep text and important subject matter inside the guide rectangle, push
background art out to the canvas edge. Settle it by pulling the layout JSON from
Pixajoy's API with a live project URL.

**Cover geometry is out of scope for v1.** It uses a different construction — a ~0.75"
wrap band on all four edges plus a spine whose width depends on final page count and
paper stock. It gets its own spec once the interior pipeline works.

## 4. Architecture

```
┌─ WKWebView ─────────────────────────────────────────┐
│  Nuxt 4 SPA (ssr: false)                            │
│    spread preview · canvas editor · chat panel      │
│    AI SDK v7 ToolLoopAgent + DirectChatTransport    │
└──────────────── invoke() ───────────────────────────┘
┌─ Rust (Tauri core) ─────────────────────────────────┐
│  layout engine  ·  book assembly  ·  SQLite cache   │
│  sidecar lifecycle · keychain · DeepSeek HTTP       │
└──────────────── NDJSON / stdio ─────────────────────┘
┌─ Swift sidecar (long-lived, spawned at launch) ─────┐
│  ImageIO decode (HEIC/RAW/JPEG) · Vision · CoreML   │
│  Core Image · Core Graphics colour-managed render   │
└─────────────────────────────────────────────────────┘
```

### 4.1 Why these boundaries

**Swift sidecar rather than in-process `objc2` bindings.** The decisive argument is
crash isolation. The app ingests 300 arbitrary user photos including camera RAW, and
ImageIO on malformed image files is a historically rich crash source. In-process, a
Swift `fatalError` is `SIGTRAP` and is not catchable from Rust; Objective-C exceptions
unwinding through Rust frames are undefined behaviour; and autorelease pools do not
exist on Rust-spawned threads, so CGImage allocations grow unbounded without explicit
Swift-side pools. A sidecar turns "one bad RAW kills the app and every unsaved edit"
into "one photo shows an error badge".

Supporting reasons: `objc2` cannot bind Apple's Foundation Models framework (pure Swift,
no Objective-C surface), three newer Vision requests are Swift-only, and `objc2-vision`
has ~78k downloads against 40M for `objc2-core-image` — much less battle-tested.

**Layout engine in Rust, not TypeScript or Swift.** It must be callable identically by
the editor, the auto-generator, and the agent's tools. One implementation reached through
`invoke` means no scoring logic duplicated across languages, and it is testable under
`cargo test` with no app running. Swift stays purely pixel-shaped; TypeScript stays
purely presentational.

**No MCP.** MCP pays for crossing a trust or vendor boundary, and this app has none — one
machine, one process tree, TypeScript on both sides, every tool compiled in. Adopting it
would cost compile-time types (every schema maintained twice instead of zero times),
breakpoints and stack traces (an MCP stdio server's stdout *is* the protocol channel),
and a second signed sidecar. Plain `tool()` definitions whose `execute` calls `invoke()`
are strictly better here. `ToolLoopAgent`'s `tools` is a plain object, so
`{ ...nativeTools, ...(await mcpClient.tools()) }` merges later at zero cost if a
third-party server ever becomes useful.

**No server.** Tauri's own guidance is `ssr: false`; Nitro routes do not exist in a
packaged app. `DirectChatTransport` runs the agent in-process in the webview. DeepSeek
calls go out through `@tauri-apps/plugin-http`, executing in Rust, capability-scoped to
`api.deepseek.com`, with the API key in the OS keychain.

### 4.2 Sidecar protocol

Newline-delimited JSON: one request per line on stdin, one response per line on stdout,
correlated by `id`. Requests are batched (`analyze` takes an array) to amortise startup
and keep the ANE warm. The process is spawned once at app launch and kept alive.

Measured cost: 0.126 ms round-trip for a 5.4 KB record against ~95 ms of Vision work —
0.13% overhead. Serialisation is not a consideration at this scale.

**Pixels never cross the IPC boundary.** A 6718 × 2668 RGBA spread is 71.7 MB; twenty
spreads is 1.43 GB. The sidecar writes PDF/TIFF/JPEG to disk and returns paths. The
webview loads previews via `convertFileSrc()` and the `asset:` protocol, which requires
`app.security.assetProtocol` configuration and a matching CSP entry.

Rust owns a per-photo timeout and a skip-and-continue policy, so a hung RAW decode
degrades to an error badge on one photo rather than a stalled import.

### 4.3 Packaging

Ship the sidecar via `bundle.externalBin`, **never `bundle.resources`** — `externalBin`
binaries are added to the bundler's `sign_paths` and signed inside-out with
`--options runtime` before the app bundle, whereas resources are never signed. The file
on disk must carry the target-triple suffix (`photobook-engine-aarch64-apple-darwin`);
the bundler does a naive string append with no fallback.

Capabilities require an explicit `shell:allow-execute` entry with `"sidecar": true` —
`shell:default` grants only `allow-open` and does not cover sidecars.

Nothing needs bundling for the Swift runtime: a Vision + Core Image Swift binary links
`/usr/lib/swift/libswiftCore.dylib` through a system rpath and runs standalone. Build
with `-target arm64-apple-macos<min>` matching `bundle.macOS.minimumSystemVersion`.

## 5. Data model

SQLite in `app_data_dir`, keyed by **content hash**, so re-imports and moved files never
trigger re-analysis.

| Table | Contents |
|---|---|
| `photos` | hash, path, mtime, format, dimensions **after orientation**, EXIF |
| `features` | analysis record, plus an `analyzer_version` column |
| `books` | SKU profile, title, source folder, seed |
| `spreads` | ordered index, template id, locked flag |
| `slots` | spread id, photo hash, crop rect, z-order, `user_overridden` flag |
| `text_boxes` | spread id, rect, content, style |

`analyzer_version` allows selective invalidation when analysis code changes, rather than
discarding the whole cache.

The product profile is data, not code:

```jsonc
{ "sku": "20pp", "spreads": 10,
  "trim": [11.0, 8.5], "bleed": 0.197, "gutter": 0.197 }
```

## 6. Analysis pipeline

Ordered so cheap, high-precision culls run before anything expensive.

1. **Scan + EXIF-only pass** — ~0.4 ms/photo. Timeline, GPS clusters, orientation,
   camera/lens, burst grouping. 300 photos in well under a second.
2. **Content hash → cache lookup.** Only misses continue.
3. **pHash near-duplicate pass** — roughly 1000× cheaper than embeddings, and family
   libraries are 20–40% burst frames. Collapses clusters before any model runs.
4. **Vision batch pass** — one `VNImageRequestHandler` per photo carrying *all* requests,
   because Vision reuses the decoded surface across them. Measured 95.5 ms serial,
   16.4 ms with `DispatchQueue.concurrentPerform` on 10 cores.
5. **`isUtility` cull** — strips screenshots, receipts, whiteboards, document scans.
   Highest-precision signal available, and free.
6. **SigLIP 2 embedding + zero-shot mood axes** — the one thing Vision cannot do.
7. **Within-book percentile ranking** — every score converted to a rank against this
   book's own population.

Expected: 300 photos analysed in ~5 seconds for JPEG/HEIC. **RAW decode has not been
benchmarked** and is materially slower; measure before promising an ingest time.

### 6.1 Vision requests used

`VNClassifyImageRequest`, `VNCalculateImageAestheticsScoresRequest` (macOS 15+),
`VNDetectFaceRectanglesRequest`, `VNDetectFaceLandmarksRequest`,
`VNDetectFaceCaptureQualityRequest`, `VNGenerateAttentionBasedSaliencyImageRequest`,
`VNDetectHorizonRequest`, `VNRecognizeTextRequest`, `VNGenerateImageFeaturePrintRequest`.

### 6.2 The feature record

```jsonc
{
  "hash": "…", "w": 4032, "h": 3024,        // post-orientation
  "is_utility": false,
  "aesthetic": { "raw": 0.42, "pct": 78 },
  "sharpness": { "raw": 214.7, "pct": 61 },
  "faces": [ { "box": [...], "yaw": 4.2, "pitch": -1.1,
               "capture_quality": 0.81, "smile_conf": 0.73 } ],
  "face_area_fraction": 0.18,
  "smile_fraction": 0.67,                    // null when undeterminable
  "saliency_box": [0.31, 0.22, 0.44, 0.51],
  "horizon_tilt_deg": -1.4,
  "scene_tags": ["beach", "sunset", "group"],
  "has_text": true,
  "palette": { "dominant": [...], "warmth": 0.71, "contrast": 0.44 },
  "mood_axes": { "energetic": 92, "intimate": 34, "celebratory": 71 },
  "embedding": [...],                        // local only, never transmitted
  "near_dup_cluster": 12,
  "event_cluster": 3
}
```

Three deliberate choices:

- **No `mood` string.** Mood is emitted as orthogonal percentile axes with confidence.
  A single confident label is a lossy summary of the pipeline's weakest signals, and is
  exactly what a downstream LLM would treat as ground truth.
- **`smile_fraction` is `null`, not `0`, when no face has usable head pose.** "Nobody
  smiling" and "couldn't tell" must not collapse into the same value.
- **`w`/`h` are post-orientation.** EXIF `Rotate90/270` swap dimensions; computing layout
  boxes before orienting puts every portrait photo in a landscape slot, silently.

### 6.3 Sentiment: the honest position

Vision has **no expression or emotion classifier at any macOS version**. It reports where
faces are, how well captured they are, and which way they point — never what they are
doing. Sentiment is therefore assembled from:

**Smile proxy.** Computed from the 65-point landmarks: mouth-corner elevation relative to
mouth width, plus eye aperture. Gated on `yaw`/`pitch` from the face rectangles request,
discarding faces beyond roughly 30° yaw. No extra model, no licence exposure. Aggregate
to a per-photo `smile_fraction` over faces with usable pose.

**Zero-shot mood via SigLIP 2 base/16-256** (Apache-2.0, ~95 MB int8), converted to Core
ML and run inside the Swift sidecar alongside the Vision pass. This is the
weakest signal in the pipeline, because CLIP-family models are trained on alt-text that
describes *what is in* a photo rather than how it feels. Four rules make it usable:

1. **Prompt for observable evidence, not the emotion word.** `"a photograph of people
   laughing together at a table"` → joyful, rather than `"a nostalgic photograph"`. The
   evidence-to-mood mapping lives in our code, where it can be inspected and corrected.
2. **Full-sentence templates with prompt ensembling** — 4–8 templates per class, text
   embeddings averaged and normalised. Free; text encoding happens once at startup.
3. **Contrastive axes, not a flat softmax.** energetic↔calm, intimate↔public,
   celebratory↔solemn. Include a neutral anchor so not every photo is forced a mood.
4. **Rank as percentiles within this book.** Raw CLIP similarities are badly calibrated.
   "The five most energetic photos in this set" is defensible; "0.31 energetic" is not.

**Colour is a lighting signal, not a mood signal.** A warm palette means golden hour or
tungsten light, not happiness. Colour is used for palette and template matching, where it
is genuinely reliable, and carries at most low weight as a mood tiebreaker.

**Face clustering (same-person grouping) is not in v1.** InsightFace/ArcFace is
non-commercial research-only, including its auto-downloaded weights, and no permissive
face embedder has been identified. Face *detection* is unaffected.

## 7. Layout engine

### 7.1 Coordinate system

Everything is normalised to the spread canvas in `[0,1]` and converted to inches or
pixels only at render. Three predicates are enforced:

- **`in_safe_area(rect)`** — text and faces must satisfy this
- **`crosses_gutter(rect)`** — nothing important may overlap the dead band
- **`bleeds_correctly(rect)`** — a full-bleed slot must extend *past* the canvas edge,
  never stop short, or trimming leaves a white sliver

### 7.2 Template format

Templates are JSON data, hot-reloadable, so tuning taste does not require a rebuild.

```jsonc
{
  "id": "hero-left-2up",
  "slots": [
    { "rect": [0.0, 0.0, 0.52, 1.0], "role": "hero",
      "bleed": ["left","top","bottom"], "aspect_pref": [1.2, 1.6] },
    { "rect": [0.58, 0.18, 0.36, 0.64], "role": "support",
      "bleed": [], "aspect_pref": [1.0, 1.5] }
  ],
  "text_zones": [ { "rect": [0.58, 0.86, 0.36, 0.08], "align": "left" } ],
  "min_photos": 2, "max_photos": 2,
  "density": "sparse", "energy": "calm"
}
```

`density` and `energy` are what the pacing pass and the chat agent reason over, and are
why "make spread 4 calmer" can resolve to a concrete template swap.

Target library size: ~30–50 templates covering 1-up full bleed, 2-up symmetric and
asymmetric, 3-up with hero, 4-up and 6-up grids, and full-bleed-with-inset.

### 7.3 Scoring

For each spread the engine enumerates viable templates × photo assignments and scores:

| Term | Measures |
|---|---|
| Aspect fit | How much crop the photo needs to fill the slot |
| Saliency retention | Fraction of `saliency_box` surviving the crop |
| Face safety | Face box cut by a slot edge or crossing the gutter |
| Gutter cleanliness | Salient content within the dead band |
| Hero match | Highest aesthetic percentile should land in the largest slot |
| Palette harmony | Oklab hue-template fit across the spread's photos |
| Variety | Repeating or mirroring the previous spread's template |
| Text room | A required text box must fit a `text_zone` inside the safe area |

**Face safety and gutter cleanliness are hard constraints** — a candidate that cuts a
face at the fold is rejected, not down-weighted. The remaining terms are weighted soft
scores, with weights stored alongside the templates so taste is tunable without a rebuild.

**Crop selection** is a separate deterministic step: given a photo and target aspect,
choose the crop window maximising saliency plus face coverage subject to bounds.

### 7.4 Book assembly

1. **Cull.** Drop `is_utility`. Within each near-duplicate cluster keep one winner,
   ranked by sharpness percentile → face capture quality → `smile_fraction` → aesthetic
   percentile.
2. **Group into chapters** from EXIF time and GPS event clusters. Event boundary
   detection is a solved problem and needs no model.
3. **Recommend the SKU.** Comfortable density is 1–6 photos per spread:

   | Keepers | Recommendation |
   |---|---|
   | ≤ 60 | 20 pages (10 spreads) |
   | 60–120 | 40 pages (20 spreads) |
   | > 120 | 40 pages, surfacing how many the culling pass will drop |

4. **Pack** photos into spreads, avoiding chapter splits across a spread where possible.
5. **Assign templates** by scoring.
6. **Pace.** Vary density and energy so the book does not read flat — a sparse hero
   spread after a dense grid, energy percentiles rising toward chapter peaks.

The SKU table assumes **20 pages = 10 spreads**. This lives entirely in the product
profile, so if Pixajoy's page navigator says otherwise it is a one-value change.

### 7.5 Determinism

Same inputs produce the same book, always. Every stochastic step takes an explicit seed,
and "regenerate this spread" **advances the seed** rather than calling a random source.
This makes golden-file testing possible, makes bug reports reproducible, and makes
regeneration feel like browsing alternatives rather than rolling dice.

## 8. Editor

Operates at **preview resolution (~1400 × 556 per spread)** — 3.1 MB rather than 71.7 MB,
which avoids WKWebView canvas size limits and keeps interaction smooth. Full-resolution
rendering happens only at export.

Built from positioned DOM elements with CSS transforms rather than a canvas library:
photos are `<img>` with `object-fit` plus a transform for crop, guides are absolutely
positioned overlays. Pointer events, hit-testing, focus and accessibility come free, and
the geometry is inspectable in devtools.

- **Guides always visible**: trim rectangle, safe area, gutter dead band — the same
  predicates the engine enforces, so what is shown is what is validated
- **Snapping** to safe area, slot edges, gutter boundaries
- **Crop** by dragging within a slot; zoom by scroll/pinch; aspect never distorts
- **Per-spread controls**: regenerate, swap photos, lock, reject, change template
- Any manual edit sets `user_overridden` on the slot, and **regeneration never overwrites
  an overridden slot**

## 9. Agent layer

`ToolLoopAgent` (AI SDK v7) with `DirectChatTransport`, running in the webview.

**Read tools** (no approval): `get_book`, `get_spread`, `list_photos`, `search_photos`,
`get_templates`
**Write tools** (`needsApproval: true`): `set_template`, `swap_photos`, `move_photo`,
`reorder_spreads`, `set_text`, `regenerate_spread`, `lock_spread`

Four rules keep it honest:

1. **Every write tool returns the resulting state**, never `{ok: true}`, so the next turn
   is self-correcting and partial failures are visible.
2. **`needsApproval` turns a hallucinated edit into a rejectable proposal.**
3. **`pruneMessages` aggressively drops old `get_spread` results** — a five-turn-old
   layout snapshot is the single most likely thing the model will confidently misquote.
4. **Zod validation is the only enforcement.** DeepSeek supports no `json_schema`
   response format, and the AI SDK provider never enables its strict path against
   `api.deepseek.com`. Nothing is validated server-side.

Additionally: `stopWhen: [isStepCount(20), …]`; `activeTools`/`prepareStep` to narrow the
callable surface by phase; `repairToolCall` to catch malformed calls before they reach
Rust; every tool call logged locally so what the agent *did* can be compared with what it
*said*.

### 9.1 Privacy chokepoint

All DeepSeek-bound data is constructed by **one function, `buildAgentPayload()`**, with a
unit test asserting the output contains no GPS, no embeddings, no OCR text, no filenames,
and no absolute timestamps.

| Sent | Withheld |
|---|---|
| Photo index (1..N) | Filename, path |
| Relative day offsets ("day 3") | Absolute timestamps |
| Coarse location label ("location B") | GPS coordinates |
| `face_count`, `face_area_fraction` | Face boxes, embeddings, identity clusters |
| Top-3 scene tags | Full classification vector |
| Percentile ranks | Raw scores |
| Template IDs, slot counts | OCR text content |

Roughly 30–60 tokens per photo; ~15K tokens for a 300-photo book, comfortably inside a
single request, at approximately $0.002 per run.

This design exists because DeepSeek's published policy states data is stored in the PRC,
used to train their models with no API carve-out, and retained indefinitely, with opt-out
available only as a rights request. Derived metadata about a family is still personal
data, and face geometry is biometric data under GDPR Art. 9.

### 9.2 DeepSeek specifics

- Model ID `deepseek-flash` passed as an explicit string — the `@ai-sdk/deepseek`
  typed union still lists the retired `deepseek-chat`/`deepseek-reasoner`. DeepSeek serves
  V4.1 Flash under `deepseek-flash`; `deepseek-v4-flash` is still accepted but only as an
  alias of a retired model (checked against api-docs.deepseek.com, 2026-09-18)
- Thinking disabled for routine calls; reasoning tokens bill as output and thinking is on
  by default
- `reasoning_content` must round-trip on every tool-calling turn or the API returns 400
- Explicit retry for the documented "occasionally returns empty content" failure
- Queueing presents as a hang — the server holds the connection up to 10 minutes before
  inference begins, so the UI shows real progress and client timeouts are set accordingly
- No pinned model IDs exist; keep runnable regression tests

## 10. Export

The sidecar renders each spread with Core Graphics, colour-managed, writes to disk and
returns paths. Measured: ~194 ms per PDF page, ~4 s for a 20-spread book.

**Default output is sRGB with an embedded ICC profile.** Most photobook printers prefer
RGB and run their own conversion; converting to CMYK without Pixajoy's specific profile
is worse than not converting. A PDF/X-3 path with an explicit output intent is available
if they request one.

### 10.1 Pre-flight

| Check | Action |
|---|---|
| Text or face outside the safe area | **Block** |
| Photo resolves below 300 DPI at its slot size | **Warn**, reporting effective DPI |
| Salient content crossing the gutter | **Warn** |
| Full-bleed slot not extending past the canvas edge | **Block** |

Output is numbered spread files plus a `manifest.json` recording which photo went where,
making a book reproducible and diffable.

## 11. Error handling

- **Sidecar crash** → Rust respawns, marks the photo failed, continues. A per-photo
  timeout catches hangs.
- **Files moved or deleted** since analysis → flagged in the book; the slot shows a
  placeholder.
- **Malformed image** → the sidecar returns an error record, never crashes.
- **Disk full on export** → fails on the first spread, not after nineteen.
- **DeepSeek unavailable** → the app remains fully usable; the agent is additive.

## 12. Testing

Tests first at each layer, per project convention.

- **Rust** — golden-file tests on the layout engine; property tests on the three
  geometric predicates (e.g. a rect is never simultaneously in-safe-area and
  crossing-gutter); SKU recommendation boundaries.
- **Swift** — a fixture corpus of deliberately hostile inputs (truncated JPEG, malformed
  RAW, zero-byte file, 0×0 image, CMYK JPEG, HEIC with contradictory `irot` vs EXIF
  orientation, 48MP RAW), each of which must produce an error record and never a crash;
  golden-file tests on the render path comparing rasterised output within tolerance.
- **TypeScript** — vitest on `buildAgentPayload()` (asserting the withheld list) and on
  the editor's geometry helpers.

The sidecar is a standalone CLI reading NDJSON on stdin, so it is testable with no app
running (`cat fixtures.ndjson | ./photobook-engine`) and debuggable directly in lldb.

## 13. Delivery phases

| Phase | Ships | Rationale |
|---|---|---|
| **1** | Swift sidecar + analysis + SQLite cache | Proves ingest on real photos including RAW — the one path never benchmarked |
| **2** | Layout engine + export | **Produces an uploadable book.** Validates the margin spec against Pixajoy for real |
| **3** | Preview UI + spread-level controls | Usable without a full editor |
| **4** | Canvas editor | The large UI investment, on proven foundations |
| **5** | Agent layer | A thin skin over tools that already exist and are tested |

Phases 1–2 are the shortest path to a real printed book. Everything after improves
control over a pipeline already known to work.

**The first implementation plan covers phases 1–2 only.** Phases 3–5 each get their own
plan once the pipeline they build on is proven; writing them now would be planning
against unvalidated assumptions about RAW throughput, mood-signal quality, and the
Pixajoy upload round-trip.

## 14. To verify before or during implementation

| Item | Why |
|---|---|
| Pixajoy page navigator: does "20 pages" show 10 or 20 thumbnails? | Halves or doubles capacity; one value in the product profile |
| Camera RAW decode throughput | Never benchmarked; gates any ingest-time promise |
| Zero-shot mood accuracy on ~200 of your own photos | The one signal not to ship unmeasured |
| `reasoning_content` round-trip on turn 2 of a DeepSeek tool loop | One request; a 400 here breaks the agent entirely |
| `frontendDist` path in the packaged build | Tauri's guide says `../dist`; Nuxt 4 generates to `.output/public` with a root `dist` symlink |
| `tauri-plugin-http` chunk streaming on the pinned version | Behaviour differed across v2 point releases |
| Whether the red guide is trim or safe-margin | Cosmetic given the identical practical rule, but worth settling |

## 15. Known traps

Recorded so they are not rediscovered during implementation.

**Geometry**
- EXIF `Rotate90/270` swap width and height. Orient before computing any layout box.
- HEIC orientation lives in both container `irot`/`imir` properties and EXIF, and they
  can disagree. `CGImageSourceCreateThumbnailAtIndex` with
  `kCGImageSourceCreateThumbnailWithTransform` reconciles this.

**Analysis**
- `faceCaptureQuality` is comparable only within the same face. Never rank different
  people with it.
- Use `hasMinimumRecall(0.0, forPrecision: 0.8)` on classification, never a flat 0.5
  threshold — per-class calibration varies widely across 1,303 labels.
- Laplacian variance scores a sharp photo of a blank wall as blurry. Use a
  contrast-normalised tile-max over a 4×4 grid, which also answers the better question:
  is the *subject* in focus.
- HEIC decode barely parallelises (~1.84×) and thumbnailing does not help it (unlike
  JPEG's 2.3×). Cache decoded downscales aggressively; decode each HEIC once, ever.

**Licensing**
- InsightFace/ArcFace is non-commercial research-only, including auto-downloaded weights.
- Apple MobileCLIP/AIMv2 are `apple-amlr`, research-only and revocable.
- pyiqa/IQA-PyTorch is PolyForm Noncommercial; TOPIQ exists only inside it.
- aesthetic-predictor-v2.5 is AGPL; BRIA RMBG is CC-BY-NC.
- Staying on Apple Vision avoids all of the above.

**Packaging**
- `bundle.resources` binaries are not signed; use `externalBin`.
- Universal builds need a pre-lipo'd file named `<name>-universal-apple-darwin`; two
  per-arch files will not work. Shipping arm64-only avoids this entirely.
- One entitlements file applies to both app and sidecar, which breaks Mac App Store
  requirements. Not a concern for Developer ID distribution.
- `tauri dev` does not rebuild the Swift sidecar. Add a watchexec task.

**Agent**
- AI SDK v7 renames: `maxSteps`→`stopWhen`, `stepCountIs`→`isStepCount`,
  `onFinish`→`onEnd` (but `ChatInit.onFinish` on the client is unchanged),
  `system`→`instructions`, `addToolResult`→`addToolOutput`.
- `useObject` has no `transport` option and is unusable without a server.
- Do not `await addToolOutput` inside `onToolCall` when using `sendAutomaticallyWhen`.
- Dev and packaged webview origins differ (`http://localhost:3000` vs `tauri://localhost`).
