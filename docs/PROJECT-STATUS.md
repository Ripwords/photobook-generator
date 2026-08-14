# PhotobookGen — Project Status

**Last updated:** 2026-08-14
**Branch:** `feat/phase-2-layout-engine` (not yet merged)

This document exists so a new agent can pick the project up without re-deriving what
was already learned. Read it before touching code. The authoritative documents are:

- `docs/superpowers/specs/2026-08-12-photobook-generator-design.md` — the overall design
  and its constraints. Still accurate, **except** for the export design, which was
  deliberately revised mid-Phase-2 (see "Phase 2" below).
- `docs/superpowers/plans/2026-08-12-phase-1-analysis-pipeline.md` — the Phase 1 plan.
  **Sections of it are wrong and carry `⚠️ SUPERSEDED` warnings.** Heed them; following the
  original text reintroduces two real bugs.

A blow-by-blow record of every fix round, ruling, and deferred finding is in
`.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/progress.md` and
`.superpowers/sdd/2026-08-14-phase-2-export-revision/` (git-ignored, local only). They are
long but they are where the reasoning lives.

**If you read only one section, read "Phase 2 open items" — every entry there is a real,
deliberately-parked decision that this file is the only surviving record of.**

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
| Template library | `templates/` | Spread templates + validator at `tests/templates.test.ts`. Was 40 at end of Phase 1; **now 36** — see the Print geometry section. |

**Test counts at last run (2026-08-14, end of Phase 2):** 496 TypeScript, 304 Rust unit
(3 ignored — they read the real `templates/` directory) plus 2 harness'd and 4
`harness = false` integration binaries, 130 Swift. All green, lint clean.
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

---

## Current state: Phase 2 is complete

Built across two plans and sixteen reviewed tasks. The app now turns an analysed folder
into files you can upload to a printer.

**Working end to end:** analyse a folder → pick a page count (the app recommends the
smallest SKU that fits, and says how many keepers each length would drop) → generate →
pre-flight → export. Generating **saves the project**; exporting writes one cropped image
file per placement.

| Stage | Where | What it does |
|---|---|---|
| Cull | `book::cull::cull` | Drops utility images, keeps one winner per near-duplicate cluster. **The single authority** — see below. |
| Pack | `book::pack` | Chapter-aware grouping into buildable group sizes, **distributed over the available slots rather than front-loaded**; drops the lowest aesthetic percentiles when keepers exceed capacity. |
| Score | `book::score` | Scores each template against a group: aspect fit, saliency and face-area retention, hero match, resolution headroom, palette harmony, variety. Hard rejections for a face in the gutter, a face outside the safe margin, and sub-`MIN_DPI` resolution. |
| Crop | `book::crop::choose_crop` | Deterministic saliency- and face-aware crop window, normalised 0…1 of the photo's **oriented** frame. |
| Pace | `book::pace::assemble` | Lays groups into pages, keeps both halves of a spread on one template, falls back to a smaller page half rather than blanking a page. |
| Pre-flight | `book::preflight` | Blocks and warnings before anything is written. |
| Export | `export.rs` → `Sidecar` → `Exporter.swift` | Builds `ExportItem`s, ships them over NDJSON, the sidecar crops and re-encodes. |
| Persist | `project.rs`, `db.rs` | Saves and reopens a project. |

### The export design was revised mid-phase — read this before changing it

**The app exports the user's own photographs, cropped. It does not composite.**

The original design rendered a full-page canvas per page with photos positioned on it.
That was abandoned deliberately (`f0ecc20 docs: revise export design to ship the user's
own images`), and the reasoning is load-bearing: compositing re-encodes the photograph
into a flattened page, so the book prints **a picture of a picture**. A photobook's whole
value is that it reproduces the camera's own image. Decode, crop, re-encode, write —
nothing else. There are **no composites and no transparent PNGs** anywhere in the output.

**Format rule** (`Exporter.outputFormat`): a **lossy source (JPEG, HEIC, HEIF, WebP)
exports as JPEG at quality 0.95**; a **lossless source (PNG, TIFF, RAW) exports as PNG**.
Re-encoding a camera JPEG to PNG inflates it roughly fivefold without recovering quality
already lost; encoding a RAW-derived crop to JPEG introduces the first generation of loss
for no reason. An unrecognised container falls to PNG, because PNG never adds a generation
of loss the source did not already have. The container is sniffed from the file via
ImageIO, not guessed from the extension. Rust's `export::predicted_format` mirrors the
rule from the extension alone, for the manifest only — the `ExportRecord` the sidecar
returns is always the authority on what was actually written.

**Colour:** output is sRGB with an embedded ICC profile, converted (redrawn into an sRGB
context), never reinterpreted. `CGImage.copy(colorSpace:)` would be cheaper but it
reinterprets the samples, shifting every colour on a wide-gamut source.

**Orientation before cropping.** `ImageLoader.loadOriented` applies EXIF orientation during
the decode, so the crop window always resolves against the *displayed* image. Cropping
stored pixels and orienting afterwards crops the wrong region of every portrait photo, and
the result does not look broken — it is just the wrong part of the picture.

**Filename collisions fail, they do not overwrite or uniquify.** Names come from the layout
engine (`p{page:02}-z{z}-{hash8}`), so a collision is an upstream bug. A silently renamed
file in a print upload is worse than a reported failure, because it looks like it worked.

### Pixajoy's published limits, now enforced

| Rule | Where | Severity |
|---|---|---|
| **200 DPI floor** | `book::score::MIN_DPI`, imported (not restated) by `book::preflight` | Hard rejection in scoring; **Block** in pre-flight |
| 300 DPI target | `WARN_DPI_CEILING` | **Warn** band only — not a second floor |
| **0.125" safe margin** | `geometry::SAFE_MARGIN_IN` / `in_safe_margin` | A face outside it rejects the template |
| **No face in the gutter** | `geometry::clear_of_gutter`, `book::score` | Hard rejection |
| Source file still exists | `book::preflight` | **Block** |
| Free disk space | `book::preflight::available_bytes` (`statfs`) | **Block** |

### One culling authority (was two)

`book::cull::cull` decides which photo survives, and it is now the **only** implementation
of that rule anywhere. Two others existed and are gone: Rust's `count_keepers` hand-rolled
it (it delegates now), and TypeScript's `keepers()` re-derived it in the webview.

That second one was a genuine user-visible defect, not mere duplication: Rust breaks a
sharpness tie on **face capture quality** and only then on aesthetic, while TypeScript went
straight from sharpness to aesthetic. On any burst where two frames tied on sharpness, the
contact sheet could show one survivor and the exported book contain another — differing in
both count and identity.

The webview could not fix this by copying the rule more carefully, because `AnalyzedPhoto`
does not carry the `captureQuality` the tie-break reads. So Rust sends the **answer**:
`commands::stamp_kept` runs `cull` over the finalized records and stamps each with `kept`,
and `keepers()` is now a filter on that flag with no ranking of its own. **Do not
reintroduce a second copy of this rule; send this one's verdict instead.**

### What Phase 3 and 4 can assume already exists

- **Projects persist and reopen.** `projects`, `project_photos`, `project_exports` tables;
  `Project`, `ProjectSummary`, `ExportRecord` in `project.rs`; commands `recommend_book`,
  `generate_book`, `export_book`, `list_projects`, `open_project`, `reveal_in_finder`.
  Generating saves; you do not need to add a save step.
- **The photo set is persisted by content hash**, not by path, so a saved book stays
  exportable after files move (`62faefe`). Rows come back ordered by `position`, which
  `Placement::photo_index` indexes into.
- **`Book` is serialisable and round-trips** (`pace::Book`, with `seed` and `dropped`).
- **A manifest** (`book::manifest`) describes every page and placement, including blank
  pages, which are kept in the list rather than dropped.
- **Pre-flight is a pure core** (`preflight_core`) plus a thin I/O shell, so Phase 3 can
  re-run it on an edited book without touching the filesystem — with one caveat noted in
  the open items below.
- **Geometry predicates are property-tested** (`geometry.rs`), so a canvas editor can snap
  against `in_trim`, `in_safe_margin` and `clear_of_gutter` rather than inventing its own.

---

## Phase 2 open items

Every one of these is real. Each was found during Phase 2, deliberately parked with a
ruling rather than forgotten, and **this file is the only place it survives.** None of them
blocks the phase; several block a later one, and those say so.

### 1. The gutter-saliency penalty does not exist

The spec promises a **penalty** for saliency falling in the gutter. Only the other half is
implemented: a face in the gutter is a hard rejection (`book::score`, `clear_of_gutter`),
and there is a test — `score_does_not_reject_generic_saliency_in_the_gutter_strip` —
pinning that generic saliency is deliberately *not* a rejection. But no `Weights` field
implements the graded penalty either. `Weights` is exactly: `aspect_fit`,
`saliency_retention`, `face_area_retention`, `hero_match`, `resolution_headroom`,
`palette_harmony`, `variety`.

**Ruling: implement it or correct the spec — do not leave the spec claiming a term the
scorer does not have.** Either is defensible; the mismatch is not.

### 2. `palette_harmony` is effectively inert

It computes hue as `atan2(b, r)` over **sRGB**, not the Oklab hue the spec asks for. Its
circular-variance floor is about **0.707**, and at weight `0.2` it therefore contributes
roughly **1% of a spread's score** — it can essentially never change which template wins.

**Ruling: parked pending real-photo calibration.** Fixing the colour space without
measuring is guesswork; the term does no harm while inert. Do not spend effort here before
the real-photo run below.

### 3. The seed never reaches spread selection — **know this before planning Phase 3**

`assemble(photos, pages, lib, w, seed)` passes `seed` only to `single_page` → `best_single`
→ `tie_break`. **`best_spread` takes no seed at all.** The seed therefore influences only
the two single pages (the first and last), never any of the middle spreads.

The spec's "regenerate this spread advances the seed" is consequently **unimplementable for
middle spreads as the code stands** — advancing the seed would change nothing. `Book.seed`
is persisted, so the data model is ready; the plumbing is not.

**Ruling: parked, but it is a Phase 3 blocker, not a Phase 2 one.** Phase 3's spread-level
"regenerate" control must either thread the seed through `best_spread` or pick a different
regeneration mechanism. Decide before planning, not during.

### 4. `choose_group_size` front-loaded — **FIXED 2026-08-14**

The greedy packer took the largest buildable size that fit, guarded by a lookahead asking
whether the remainder stayed decomposable. Once the library covered every size 1–6 that
guard went **inert** — `1` is buildable, so `is_decomposable(rest)` is true for every
remainder — and the largest candidate always won. Photos were packed six to a spread until
they ran out and every remaining slot was left blank.

`pack` now sizes each group against **how many photos and slots remain**
(`remaining / slots_left`, rounded half up, snapped to the nearest buildable size), and
apportions the book-wide slot budget across chapters before cutting any of them
(`apportion_slots`: one slot each longest-chapter-first, then D'Hondt highest-averages for
the rest). Measured end to end through `pace::assemble` against the **real** `templates/`
library, 20 pages:

| photos | keepers | before: groups / placed / blank pages | after: groups / placed / blank pages |
|---|---|---|---|
| 14 | 12 | `[3,4,5]` / 11 / **16** | `[1,1,1,1,1,1,1,1,1,2,1]` / 12 / **0** |
| 24 | 20 | `[3,4,6,6,1]` / 20 / **12** | `[2,1,2,2,2,2,2,2,2,2,1]` / 20 / **0** |
| 30 | 25 | `[3,4,6,6,2,4]` / 25 / **10** | `[3,2,2,2,2,2,2,2,2,2,4]` / 25 / **0** |
| 40 | 34 | `[3,4,6,6,2,6,3,4]` / 34 / **6** | `[3,4,3,3,3,3,2,3,3,3,4]` / 34 / **0** |
| 60 | 51 | 11 groups / 51 / 0 | 11 groups / 51 / 0 |
| 120 | 102 | 11 groups / **43** / 0 | 11 groups / **52** / 0 |

Never worse on either axis, and at 120 photos it places 9 more of them. The pinned
regression is `pack_fills_every_slot_rather_than_front_loading_the_first_spreads` (30
photos, 9 spreads + 2 singles, `buildable = 1..=6`), which fails with
`11 slots but only 5 groups: [6, 6, 6, 6, 6]` against the old rule.

The golden fixture (`src-tauri/tests/fixtures/book-20.json`) went from **2 blank pages to
0**. Its placed count fell 24 → 21, which is an artefact of the *frozen five-template*
in-test library, not of the packer: every one of those five templates has a **1-slot left
half**, so the closing single page can hold exactly one photo however the groups are cut.
Against the real library the same 30-photo fixture places all 25 keepers (table above).

Sizes are not uniform — the half-up rounding interleaves the floor and ceiling values
(`[3,4,3,3,3,3,2,3,3,3,4]`) rather than clustering the large ones at the front — so
`pace::repace` still has density and energy variation to work with. See "the remaining
tension" note below.

**Still open, smaller:** `pack` sizes every group by SPREAD counts, but two of the slots it
sizes for are single pages holding only a page-half's worth. Evening out the distribution
does not cause this (it is why `best_single` takes the largest half that FITS rather than
an exact match) but it does make the two end groups more likely to overflow on a library
whose halves are small. Closing it properly means teaching `Capacity`/`apportion_slots`
that the first and last slots have a smaller photo capacity than a spread. Not attempted;
judge it at the real-photo run.

### 5. RAW is uncovered end to end, and cannot be closed synthetically

There is **no test anywhere** that exercises a RAW file through analysis or export. This is
not an oversight. macOS has **no writable RAW type** — nothing in ImageIO can author one —
so any fixture this repo could generate would be a renamed JPEG, which would test the
extension-matching path and nothing else while *looking* like RAW coverage. That is worse
than no coverage.

`Exporter.outputFormat` handles RAW by conformance to `public.camera-raw-image` (which
catches vendor subtypes such as `com.sony.arw-raw-image`), and the rule is unit-tested as a
pure function against `UTType`s. The decode path is not.

**Ruling: only closable at a real-photo run with actual camera RAW files.** Do not
manufacture a fixture.

### 6. Persistence is plumbed but only half-reachable from the UI

The backend supports exporting **any** saved project: `list_projects`, `open_project` and
`export_book` all exist and work. The UI calls `list_projects` (`useBook.ts`) but never
`open_project`, so in practice **Export is only offered for the book generated in the
current session.** Quit the app and the saved project cannot be exported again without
regenerating it.

**Ruling: parked as a Phase 3 UI concern.** The plumbing needs no work; only the screen
does.

### 7. Two constants are chosen, not measured

- **Crop-bounds tolerance `1e-6`** (`Exporter.cropRect`) — matched to Rust's containment
  epsilon loosened by three orders of magnitude, so an f64 round-trip landing at
  `1.0 + 2e-16` is absorbed while a real out-of-range value still reports. Reasoned, not
  measured.
- **`EXPORT_BATCH_SIZE = 8`** and **`timeout_for`** (`sidecar.rs`) — export reuses
  analysis's timeout scale (`10s + 3s/photo`) on the argument that an export decodes the
  same images. Nobody has timed an export.

**Ruling: fine until the real-photo run says otherwise.** Both are the kind of number that
only real data can justify.

### 8. The `ORDER BY position` guard is weaker than it looks

`db.rs` selects project photos with `ORDER BY position ASC` and a comment explaining that
it is not cosmetic, since `Placement::photo_index` indexes into that order. **But deleting
the clause is inert**: the composite primary key already causes SQLite to return the rows
in position order. The test therefore only catches a *wrong-column* substitution, not a
dropped `ORDER BY`.

**Ruling: documented rather than fixed.** The clause is still correct to keep — relying on
an index's incidental ordering is not a contract — but do not read the passing test as
proof that removing it would be caught.

### 9. `preflight_core`'s doc comment is wrong about I/O

It says "No I/O", and it performs one `Path::exists()` per placement (the moved-source-file
Block check). Minor, but it matters to Phase 3: the "pure core" is not actually callable in
a hot loop or off-disk. Either move the existence check out to the shell or correct the
comment.

### 10. Culling is fully reconciled — nothing remains

Recorded here because the ledger asked for whatever Step 2 concluded. The two culling
authorities are now one (see "One culling authority" above); `keepers()` carries no rule,
and Rust's verdict travels on the wire as `AnalyzedPhoto.kept`. **Nothing is outstanding on
this item.**

---

## The outstanding verification — NOT done, and not fakeable

**Nobody has generated a real book from real photos and uploaded a page to Pixajoy.**

This is the only step that would prove Phase 2 actually works. Everything above proves the
parts behave as specified against fixtures. It requires a human with a folder of their own
photographs and a Pixajoy account, and it was **not** performed. It was not simulated, and
no synthetic stand-in was substituted — doing so would produce a green tick over an
unverified pipeline, which is the exact failure mode this document exists to prevent.

**Exactly what closes it:**

1. Run the app against a real photo folder. Generate a book. Export it.
2. Upload one page's files to Pixajoy's editor.
3. **Confirm the picker accepts the exported format.** This is still an open question:
   Pixajoy's published guidance says nothing about accepted file formats, so whether it
   takes JPEG *and* PNG is unknown. The export rule sends PNG for every lossless source
   (including all RAW), so if the picker rejects PNG, the format rule needs revisiting —
   not the crop pipeline.
4. While there, settle the still-open Phase 1 items that need real photos: RAW/HEIC
   throughput, the whole face code path, and Pixajoy's page-count semantics.

Until that is done, treat Phase 2 as "complete and internally verified", not "proven".

---

## On tests in this project — the warning that matters most

Phase 1 shipped eight tests that passed under a broken implementation. **Phase 2 found and
fixed nine more decorative fixtures and four inert prescribed mutations** — tests written
into detailed plans by agents that believed they were sound, which would have passed with
the feature deleted.

That is thirteen more in one phase. It is the single most useful thing this document
carries forward, so treat it as a standing rule rather than a historical note:

- **Do not trust that a test tests what it says.** Break the thing deliberately and confirm
  the test notices.
- **Mutation-check anything load-bearing and paste the evidence** into the commit message.
  Every load-bearing test added in Phase 2 has such a proof attached.
- **A test that passes with the feature deleted is worse than no test**, because it reads
  as protection.

The recurring shapes, all of which appeared for real:

| Shape | Example |
|---|---|
| A fixture where the property under test is already true of the input | An ordering test whose input was already sorted |
| A fixture where an earlier key decides the outcome, so later keys are never reached | A culling tie-break test where sharpness already picked the winner, so the capture-quality and aesthetic arms were untested |
| A boundary test using a value nowhere near the boundary | |
| A symmetric fixture that survives a swap | A square image, which makes `height/width == 1` and hides every aspect bug |
| A round-trip that cannot detect a symmetric error | Serialise-then-deserialise with the *same* struct round-trips a wrong rename perfectly; only asserting literal wire bytes catches it |
| A degenerate value that "passes" a guard via NaN propagation | A zero-width box; use a negative width |
| Uniqueness that rides on the wrong axis | Filenames "unique by page+z" tested with a fixture that also varied the hash per photo |

**The single wire test.** `src-tauri/tests/export_roundtrip.rs` is the only test that drives
both sides of the Rust↔Swift export wire against each other. Every other export test judges
one side against a *copy* of the contract. Keep it that way, and keep its fixtures
non-square with asymmetric crops — a drifted key name otherwise degrades into "every photo
failed" with the cause reported nowhere, and the sidecar's own message is only the opaque
`malformed request`.

---

## What is NOT built

Phases 3 through 5 of the design. Each needs its own spec-then-plan cycle; do not start
implementing from the design doc alone.

| Phase | Scope | Blocked on |
|---|---|---|
| **3** | Spread preview UI + spread-level controls (regenerate, swap, lock, reject) | Nothing — but **read the seed item in "Phase 2 open items" first**, because "regenerate this spread" is not currently implementable for middle spreads. |
| **4** | Canvas editor: drag/resize/crop with snapping to the margin guides | Phase 3 |
| **5** | AI SDK v7 + DeepSeek chat agent driving the layout tools | Phases 3-4 |

### Requested, not yet specced: multiple source folders

The user has asked for the app to accept **more than one folder**. This is a design change,
not a flag, and it should be specced alongside Phase 2 rather than bolted on. What it
touches:

- **`analyze_folder(folder: String)`** becomes a list. The picker needs
  `open({ directory: true, multiple: true })`, and the chunked gather needs to walk several
  roots while still emitting one coherent progress stream.
- **Population semantics.** `percentiles` ranks each photo against "the book". With several
  folders the book is their union, so ranking across the union is correct — but that must be
  a deliberate decision, not an accident of implementation, because it means adding a folder
  silently rerankes every photo already on screen.
- **Event clustering across roots.** `event_clusters` works on timestamps and does not care
  about folders, so photos from two folders shot the same afternoon become one chapter. That
  is probably right, but it is a behaviour worth confirming rather than assuming.
- **Sort order.** `finalize_photos` sorts by path, which interleaves folders arbitrarily.
  With multiple roots the sheet likely wants grouping by folder, or by capture time.
- **Duplicates across folders** already resolve correctly — the cache is keyed by content
  hash, so the same photo in two folders is analysed once.
- **UI.** Photos probably need to show which folder they came from, and the empty and error
  states currently assume a single `folder` string.

Nothing about the analysis pipeline blocks this; it is entirely a question of what "the
book's population" means once there is more than one source.

Also explicitly deferred:

- **SigLIP 2 zero-shot mood axes.** Needs a Core ML conversion, a designed
  evidence-grounded vocabulary, and measurement against ~200 real photos. The design doc
  names it the weakest signal in the pipeline; everything works without it.
- **Palette harmony scoring** (Matsuda hue templates). A `palette_harmony` term now exists
  in the Phase 2 scorer, but it is **not** the Matsuda-template scoring this line meant and
  it is effectively inert — see Phase 2 open item 2.
- **GPS location clustering.** Only time-gap event clustering exists.
- **Same-person face clustering.** Blocked on licensing: InsightFace/ArcFace is
  non-commercial research-only including its auto-downloaded weights. Face *detection* is
  unaffected. Do not reach for InsightFace.
- **The cover.** Different geometry from the interior (~0.75" wrap band plus a spine whose
  width depends on page count and paper stock). Own spec.

### Deferred: smile detection calibration

`SmileProxy`'s threshold is miscalibrated, proven with real measured data, and finishing
it is being deferred so Phase 2 can start. The UI display is already suppressed (see
`app/components/PhotoTile.vue` — the smile badge is commented out at the suppression
point with a pointer back to this section); the pipeline still computes and stores
`smileFraction` end to end, so no data is lost and no work here needs redoing.

**Measured data**, via `scripts/calibrate-smile.sh`, 15 faces across 10 real photos,
**every face confirmed smiling by the user**:

```
lift          n=15  min=0.005  median=0.018  max=0.072
midpointLift  n=15  min=-0.081 median=0.063  max=0.089
```

Confidences ranged 0.31–0.44. `outerLips` returned 14 points on every face (see
"Genuinely verified" above — that part of the old unknown is closed).

**Three conclusions:**

1. `minOuterLipPoints = 6` is correct and needs no change — Vision reliably returns 14.
2. **The threshold produces a 100% false-negative rate.** `confidence > 0.5` requires
   `lift > 0.10`; the maximum lift observed on a genuine smile was 0.072. No real smile
   can pass the current threshold.
3. **`midpointLift` is disqualified — do not revisit it.** It had looked ~25% more
   sensitive than `lift` on synthetic fixtures, which is why `SmileGeometry` still emits
   it as a candidate. Real data killed that: the objectively strongest smile in the set
   (open mouth, teeth visible) scored the *most negative* midpointLift (-0.081), because
   an open mouth drags the lip contour's midpoint down. It is not a usable signal.

**What is still missing:** neutral-expression faces. Every sample above is one class
(confirmed smiling), so a threshold fit to this data alone would classify nearly
everything as smiling. A subtle genuine smile measured lift `0.005` — if neutral faces
cluster in the 0.00–0.01 range, the two classes overlap and lip-corner geometry may
simply not discriminate smiling from neutral at this landmark resolution.

**Exactly what finishes this:**

1. Run `scripts/calibrate-smile.sh` over 15-20 clearly **neutral**-expression faces (same
   script, same real-photo requirement — no synthetic fixtures).
2. Compare the neutral `lift` distribution against the smiling one above.
   - If they separate cleanly, set `confidence(for:)`'s threshold (currently the
     `[-0.15, 0.35]` map onto 0...1 in `SmileProxy.swift`, gated at `> 0.5`) at the point
     where the two distributions separate, and re-enable the badge in `PhotoTile.vue`.
   - If they do not separate — e.g. neutral faces also cluster near 0.00-0.01 — the
     correct outcome is to **drop `smile_fraction` entirely** (sidecar, wire format, and
     types) rather than ship a number that looks meaningful but isn't. Do not just lower
     the threshold to force separation; that produces a different wrong number, not a
     right one.

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
| **Minimum print DPI** | **200** (Pixajoy's published minimum; 300 is their recommended target, treated as a warn band, not a second floor). `book::score::MIN_DPI`, imported (not restated) by `book::preflight`. |
| **Safe margin** | **0.125"** (1/8") — Pixajoy's published guidance: keep anything important clear of the edge by this much, on top of the trim inset. `geometry::SAFE_MARGIN_IN` / `in_safe_margin`. **Do not confuse with the "Safe area" row above** — that row is the TRIM rectangle (the `BLEED_IN` inset alone); the safe margin is trim inset by a FURTHER 0.125" on every edge except the fold, where the gutter dead band already governs. |

### Page structure (confirmed by the user, 2026-08-13) — template gap now closed

A book is **not** simply N/2 spreads. The first and last pages are **single pages facing
the inside covers**, not halves of a spread:

```
[inside front cover] p1 | p2-p3 | p4-p5 | ... | p18-p19 | p20 [inside back cover]
                    single   9 full spreads (18 pages)      single
```

- **20 pages** = 2 single pages + **9 spreads**
- **40 pages** = 2 single pages + **19 spreads**
- Generally: `2 singles + (N - 2) / 2 spreads`

This corrects the earlier working assumption of "20 pages = 10 spreads".

**✅ Closed in Phase 2.** There is still no separate single-page template *file* set, and
none is needed: `templates.rs` **decomposes** each spread template into two `PageLayout`s
(a left half and a right half), and `pace::single_page` builds the first and last pages
from that half-pool. Fold-spanning slots are forbidden by the validator precisely so this
decomposition is always clean (`31afe4d` removed 21 templates that were unbuildable under
that rule). The packer does treat the first and last pages as structurally distinct.

**Template contract, validator-enforced:** `rect` values are normalised to the spread
canvas, but **`aspect_pref` is a real-world (inch) aspect ratio**, not the normalised rect
ratio. The canvas is 2.518:1, so a Phase 2 scorer that assumes normalised ratios will
mis-score every slot. Verified numerically: 102 of 103 slots have their real-world ratio
inside their declared range; zero have their normalised ratio inside it.

Every template is **exact-count** (`min_photos == max_photos == slots.length`), so the
packer selects templates by photo count rather than fitting a range.

**Coverage as of end of Phase 2 — the gaps are closed.** The library is now **36 spread
templates** plus `weights.json` and a `README.md`:

| Photos per template | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| Templates available | 8 | 6 | 6 | 7 | **4** | 5 |

Two commits got here: `31afe4d` removed 21 templates whose slots straddled the fold (they
could not decompose into page halves), and `77c5005` authored page-subdivision layouts
closing the 4/5/6-up gaps. The ignored test
`templates_real_library_covers_every_group_size_from_one_to_six` asserts this against the
real directory — run it with `cargo test -- --ignored`.

**Note the side effect, since fixed:** closing the coverage gap made `choose_group_size`
always take 6, because its decomposability guard is trivially true once `1` is buildable.
See open item 4 above — the packer now sizes groups against the remaining slots instead.

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
- **The export wire, driven end to end against the real sidecar binary**
  (`src-tauri/tests/export_roundtrip.rs`). A real `ExportRequest` goes down stdin as one
  NDJSON line; the cropped files are then read back **off disk** and their container and
  pixel dimensions parsed from their own bytes, not taken from the sidecar's own
  `ExportRecord`. Mutation-verified against four separate breakages: a renamed
  `sourcePath`, swapped `cropW`/`cropH` renames, a swapped width/height in
  `Exporter.cropRect`, and a forced-JPEG `outputFormat`. Note that `ExportItem` on the
  Swift side uses a *synthesised* `Codable`, unlike the analyze path — so a key-name typo
  there fails the whole request with the opaque message `malformed request`, and this test
  is the only thing that surfaces it.
- `analyze_batches` returns exactly one record per input path under retry, total failure,
  and wrong-count responses — asserted by *offset*, with the path encoded in each record.
- The tokio starvation fix, via an integration test on a **1-worker-thread runtime**
  against the real sidecar binary, mutation-verified.
- Contrast ratios for every accent-bearing element in both light and dark mode.
- **Vision's `outerLips` point count.** Measured via `scripts/calibrate-smile.sh` on 15
  faces across 10 real photos: `outerLips` returned **14 points on every single face**.
  `SmileProxy.minOuterLipPoints = 6` is comfortably below that and is correct as-is. This
  closes the "per-region point count" unknown that used to sit in the NOT-verified list
  below — do not re-open it. (The threshold `SmileProxy` uses on top of that point count
  is a separate, still-open problem — see "Deferred: smile detection calibration" under
  What is NOT built.)

### NOT verified — do these first when you have real photos
1. **RAW and HEIC throughput.** Never measured. The design's §14 flags it. Timeouts are
   deliberately generous (`10s + 3s/photo`) precisely because nobody knows.
2. **The whole face code path.** No fixture contains a face, so landmark seeding,
   `FaceObservation` construction and the landmark coordinate conversion are never
   executed by any test. A synthetic drawn face will not reliably trip Vision's detector,
   so this needs real photos.
3. **Pixajoy's page-count semantics.** Does "20 pages" mean 10 spreads or 20? Open the
   editor and count thumbnails in the Pages panel. It lives in a product profile so it is
   a one-value change, but the capacity math doubles or halves on it.
4. **Whether the red editor guide is the trim line or a safe-text margin.** The distance
   is 5 mm either way and the practical rule is identical, so this is cosmetic — but it is
   unresolved.
5. **CSP and asset protocol in a packaged bundle.** Verified in `tauri dev` only.
6. **The notification banner actually rendering.** macOS showed its genuine first-run
   permission prompt for `PhotobookGen`, but nobody could click Allow (no Accessibility
   access, and blind-clicking a live desktop was correctly refused). The gating logic and
   non-fatal failure path are unit-tested; the banner itself has never been seen. Grant
   permission on first run and confirm.
7. **Whether Pixajoy's picker accepts JPEG *and* PNG.** Their published guidance says
   nothing about accepted formats. The export rule sends PNG for every lossless source,
   including all RAW, so a picker that takes only JPEG would force the format rule to be
   revisited. See "The outstanding verification" above.
8. **An actual generated book, exported and uploaded.** Phase 2 has never produced output
   that a human has looked at or a printer has accepted. This is the big one — see "The
   outstanding verification" above.

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
- ~~`count_keepers` in Rust duplicates `keepers()` in TypeScript.~~ **Fixed in Phase 2** —
  `book::cull::cull` is the single authority and its verdict travels on the wire as
  `AnalyzedPhoto.kept`. See "One culling authority" above.
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
- `ImageLoader` pays two full decodes for any source whose long edge is under
  `analysisMaxPixel` (no embedded thumbnail, so pass 1 decodes fully, fails the floor
  check, and pass 2 decodes again). Hits PNGs, web-sized JPEGs and scans. Performance
  only, not correctness. Suggested guard: skip pass 2 when
  `min(sourceLongEdge, maxPixel) <= returnedLongEdge`.
- `Sidecar::benchmark` and `ResponseResult::Benchmarked` (Rust) are dead code —
  `scripts/benchmark.sh` drives the sidecar binary directly over stdin/stdout and never
  goes through Rust.

---

## Immediate next steps

1. **Generate a real book from your own photos and upload one page to Pixajoy.** This is
   the outstanding verification described above, and it is by far the highest-value thing
   remaining — it is the only thing that would turn "Phase 2 is internally verified" into
   "Phase 2 works". It settles, in one sitting: whether the picker accepts JPEG and PNG,
   RAW and HEIC throughput, the whole face code path, Pixajoy's page-count semantics, and
   whether the new even-distribution pacing (open item 4) reads well on real photographs --
   it is measured against fixtures only.
2. **Answer the Pixajoy page-count question** — 30 seconds in their editor, while you are
   there.
3. **Decide the seed question before planning Phase 3** (open item 3). "Regenerate this
   spread" cannot be built as specified until the seed reaches `best_spread`.
4. **Rule on the gutter-saliency penalty** (open item 1): implement it, or correct the
   spec. Do not leave the spec promising a term the scorer does not have.
