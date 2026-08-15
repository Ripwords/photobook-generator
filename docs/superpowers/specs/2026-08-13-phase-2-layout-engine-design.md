# Phase 2 — Layout Engine and Export

**Date:** 2026-08-13
**Status:** Design approved, plan not yet written
**Supersedes:** §7 and §10 of `2026-08-12-photobook-generator-design.md`, which assumed a
layout may span the fold. It may not. Where the two disagree, this document wins.

Phase 2 is the phase that first produces an uploadable book. It takes the analysed photos
from Phase 1, decides which survive, groups them into chapters and pages, scores the
template library to pick a layout, chooses crops, renders print-resolution files and
writes a manifest.

---

## 1. What changed, and why it changes the architecture

Pixajoy's editor was inspected live on 2026-08-13. Three facts, all verified by the user
against a real project:

1. The editor is **box-based per page** — `Add Picture Box` / `Add Text Box`, freely
   positioned. It is not template-locked, so the app can reproduce any layout it designs.
2. A picture box **can be dragged out to the bleed edge**. Full-bleed slots are achievable.
3. A picture box **cannot span the fold**, and the user does not want cross-fold images.

Fact 3 is load-bearing and invalidates part of the authored library. Measured across all
40 templates: **15 have a slot that spans the fold**, and none merely graze the dead band —
every crossing slot spans it outright, most spanning the full canvas width.

| Group | Templates |
|---|---|
| Full-bleed 1-ups | `01`, `02` |
| Panoramas | `31`, `32`, `33`, `40` |
| Full-bleed backgrounds | `26`, `27` |
| Title spread | `29` |
| Asymmetric heroes | `09`, `10`, `15`, `16`, `17`, `37` |

These are **removed**, not re-authored. Most are unsalvageable by definition — a panorama's
purpose is to span the fold, and `01-full-bleed-1up` is one photo across the whole spread,
which is now impossible.

A fourth fact was established by measurement rather than inspection, and is documented in
§6: most of the surviving multi-photo templates are shaped for photos nobody owns.

---

## 2. Geometry and the template model

### 2.1 The decision

Three options were considered:

- **A — page as the authoring unit.** Discard the spread format; every template is one
  page. Rejected: independently choosing a left and right page destroys cross-fold
  alignment, which seven templates exist for (grid rows, the filmstrip footer).
- **B — spread stays the unit; re-author the 15.** Rejected: at least seven of the 15 are
  not re-authorable, and it requires maintaining a second template library for the single
  pages 1 and N.
- **C — spread is the authoring *format*; the page is the engine's *atom*.** Chosen.

Under C the spread JSON, the validator and the surviving authored files are all kept.
Spreads still score as spreads, so cross-fold alignment survives exactly as authored. The
engine decomposes each template into a pair of page layouts at load, and pages 1 and N draw
from the pool of page-halves — a half is by construction a valid page layout, so the
single-page gap noted in `PROJECT-STATUS.md` dissolves without authoring a second library.

### 2.2 Canvases

| | Value |
|---|---|
| Spread canvas (authoring) | 22.394" × 8.894" = 6718 × 2668 px @ 300 DPI |
| **Page canvas (engine, render)** | **11.197" × 8.894" = 3359 × 2668 px @ 300 DPI** |
| Left page | spread x ∈ [0, 0.5] |
| Right page | spread x ∈ [0.5, 1] |
| Page-normalised conversion | left `u = x / 0.5`; right `u = (x − 0.5) / 0.5` |
| Trim inset | 0.197" (5 mm) on the three outer edges; **no bleed at the fold edge** |
| Gutter dead strip | innermost 0.197" — `u > 0.982406` (left page), `u < 0.017594` (right page) |

Two pages of 3359 px sum to 6718 px, matching the spread canvas exactly. No rounding drift
accumulates.

**Page aspect is 1.259:1**, which is close to 4:3. This single number drives the authoring
rules in §6 and is the reason option C is better than it first appears.

### 2.3 The new validator rule

> No slot and no text zone may span the fold: `x < 0.5 < x + w` is rejected.

This is narrower than "must not touch the gutter dead band". A slot running **flush to** the
fold edge is legal and desirable — it loses 5 mm into the binding, which is a *content*
question, not a geometry one. Forbidding it would make full-bleed pages impossible, which
is the opposite of what is wanted.

### 2.4 Predicates

Page-local, and note which kind each is:

- **`in_trim(rect)`** — geometry. Faces must satisfy it.
- **`bleeds_correctly(rect, edges)`** — geometry. A slot declaring bleed must extend *past*
  its declared outer edges, never stop short, or trimming leaves a white sliver.
- **`clear_of_gutter(content_rect)`** — **content**, not geometry. Faces and salient regions
  stay 0.197" clear of the fold. A scoring term and a pre-flight check, never a slot
  rejection.

### 2.5 `aspect_pref` needs no change

`aspect_pref` is a **real-world inch ratio**, not the normalised rect ratio, and is already
validator-enforced as such (102 of 103 original slots comply; zero comply under the
normalised reading). Because it is already in inch space, it survives the page
decomposition untouched.

The Phase 2 risk is not the templates but the **scorer**: it must convert a slot rect to
inches before deriving any ratio, and must never compare against the normalised ratio. The
spread canvas is 2.518:1, so conflating the two mis-scores every slot. No existing test
would catch this.

---

## 3. The engine pipeline

Seven steps, each a pure function testable without an `AppHandle` — following the pattern
the codebase already uses five times (`finalize_photos`, `analyze_batches`, `lookup_cache`,
`percentiles`, `imageNormalizedTopLeft`).

### 3.1 Cull

Drop `is_utility`. Within each near-duplicate cluster keep one winner, ranked:

> sharpness percentile → face capture quality → aesthetic percentile

**`smile_fraction` is deliberately excluded**, unlike §7.4 of the original design. It is
miscalibrated with a proven 100% false-negative rate (see `PROJECT-STATUS.md`); including
it adds noise, not signal.

This step also **collapses the `count_keepers` (Rust) / `keepers()` (TypeScript)
duplication** into one authority. Two implementations of the culling rule in two languages
already disagree silently; Phase 2 is what makes the disagreement visible in output rather
than only in a notification count.

### 3.2 Chapter

Reuse the existing time-gap event clusters. No new work.

### 3.3 Size the book

A book is **2 single pages + S spreads**, confirmed against the editor's page navigator
(`Cover · 1 · 2-3 · 4-5 · …`).

| Pages | Structure | Photo capacity |
|---|---|---|
| 20 | 2 singles + 9 spreads | 11 – 60 |
| 40 | 2 singles + 19 spreads | 21 – 120 |

The engine proposes a length from the keeper count, reports how many photos it will drop,
and waits for the user to accept or override before anything is rendered.

### 3.4 Pack

Walk chapters in time order, cutting each into per-spread groups of 1–6 photos. A chapter
should not open mid-spread where avoidable — a new chapter starting on a right-hand page
reads as an accident.

Group sizes must match what the library can actually build. Templates are **exact-count**
(`min_photos == max_photos == slots.length`), so a group of five is unbuildable until
5-photo templates exist. Until §6's authoring lands, the packer must not emit groups it
cannot fill.

### 3.5 Assign templates

Score every eligible template against each group; take the winner. See §4.

### 3.6 Pace

Sweep the assigned book once and swap where the rhythm is flat: dense following dense, the
same template twice within reach, three margined pages in a row.

Pacing runs on **three axes**, not two. `density` and `energy` are authored per template;
**edge treatment** (does this page run to the edge, or sit in a margin?) is derived for free
from the existing per-slot `bleed` arrays. The user explicitly wants both looks present and
alternating, not a book that commits to either.

### 3.7 Choose crops

Deterministic, no search, no seed. Given a photo and a target aspect:

1. Take the largest rectangle of that aspect fitting inside the photo.
2. Slide it along its free axis to the weighted centroid of faces ∪ saliency box, clamped to
   stay inside the photo.
3. If that clips a face, shift to uncover it. If two faces cannot both fit, keep the one
   with higher capture quality.

Never upscales, never distorts. The same photo in the same slot crops identically every
time, which is what makes golden files stable.

### 3.8 Ordering is deliberate

Cull before sizing (a book cannot be sized before it is culled), pack before scoring
(exact-count templates mean group size decides eligibility), pace after scoring (a rhythm
cannot be varied before it exists).

### 3.9 Determinism

Every step that could break a tie randomly takes an explicit seed. "Regenerate this spread"
**advances the seed** rather than calling a random source. This makes golden-file testing
possible, bug reports reproducible, and regeneration feel like browsing alternatives rather
than rolling dice.

---

## 4. Scoring

### 4.1 Candidate space

Eligible templates are those with exactly N slots for a group of N photos. A candidate is a
template plus an assignment of photos to slots — worst case 6! = 720 assignments per
template, over box arithmetic already computed in Phase 1, no pixels touched. Brute force is
correct here and simpler to test. If profiling ever disagrees, the escape hatch is a
pairwise cost matrix plus Hungarian assignment; it is not expected to be needed.

### 4.2 Hard constraints — reject, do not down-weight

| Constraint | Why hard |
|---|---|
| A face box clipped by a slot edge | A half-face is ruined, not slightly worse |
| A face inside the gutter dead strip | It disappears into the crease |
| A face inside the 0.125" safe margin | Pixajoy: leave 1/8" between anything important and the edge, or it can be trimmed off |
| Effective resolution below 200 DPI at slot size | Pixajoy's published minimum; below it, print is visibly soft |

**These numbers come from Pixajoy's own published guidance** (`8-things-to-avoid-when-designing-a-photo-book`), read 2026-08-14, and supersede the 150 DPI floor and trim-only margin this document originally specified:

- **200 DPI minimum, 300 DPI recommended.** 150 was too permissive and would have passed photos Pixajoy considers unprintable.
- **1/8" (0.125") safe margin** between important content and the edge. This is *inside* the trim line and is additional to the 0.197" bleed — a face sitting just inside trim is not safe, which the original design missed.
- **No face or notable feature in the gutter**, and on a spread, subjects positioned off to the side rather than centred. Already enforced.

The page states nothing about accepted file formats, colour space, or file size limits.

Note the split from §2.4: a **face** in the dead strip is a rejection; generic salient
content there is only a penalty. Otherwise no slot could ever run to the fold.

### 4.3 Soft terms

Weights live in `templates/weights.json`, hot-reloadable beside the templates so taste is
tunable without a rebuild.

| Term | Measures |
|---|---|
| Aspect fit | How much of the photo is discarded to fill the slot |
| Saliency retention | Fraction of the saliency box surviving the crop |
| Face area retention | Same for faces, weighted above generic saliency |
| Hero match | Highest aesthetic percentile should land in the `hero` role |
| Resolution headroom | Effective DPI at slot size, above the 200 floor (§4.2), saturating at 300 |
| Palette harmony | Oklab hue spread across the spread's photos — deferred out of Phase 1 |
| Variety | Penalise reusing or mirroring the previous spread's template |
| Gutter saliency | Fraction of the surviving saliency box falling in the gutter dead band (§4.2's penalty half) |
| Face quality | Vision's blur/exposure/pose score for the best face — the nearest thing to an expression signal (§4.2a) |
| Spread diversity | How unlike each other the photos on one spread are: scene tags, palette, capture-time gap |
| Hero prominence | Rewards a dominant hero slot when the group holds a standout photo |

**The last four ship at weight `0.0`.** `face_quality`, `spread_diversity`, `gutter_saliency`
and `hero_prominence` are implemented, unit-tested and wired into `score_spread`, but every
one of them is inert in `templates/weights.json` until it has been tuned against real
photographs. See PROJECT-STATUS.md § "The four new scoring terms are DORMANT".

**Text room is removed** — Phase 2 renders no text (§5.4).

---

## 5. Render, export and the manifest

### 5.1 Delivery mechanic

**One cropped copy of the user's own photo per slot.** Nothing is composited, nothing is
synthesised — each exported file is that photo, cropped to the window the engine chose,
colour-managed, at full resolution.

This is a deliberate reversal of two earlier designs, both now rejected:

- **A composite image per page** was rejected because it re-encodes the user's photograph
  into a flattened page. The book would print a picture of a picture. As the user put it,
  the photobook uses their camera's images directly, as a memory.
- **A transparent PNG per photo** — a full page canvas with the photo at its exact slot rect
  and alpha 0 elsewhere — was designed to carry geometry through an editor with no numeric
  position entry. It is dead for the same reason, and independently would have been
  unverifiable: Pixajoy publishes nothing about accepted formats.

**The consequence, stated plainly:** Pixajoy's boxes have no numeric X/Y/W/H entry, so the
user positions by eye. The engine's geometry is therefore *advisory* at placement time, not
guaranteed. This is why §4.2 now enforces Pixajoy's 0.125" safe margin and 200 DPI floor
rather than the more permissive originals — the margins must absorb hand-placement error.

**Encoding matches the source.** A lossy source (JPEG, HEIC) exports as JPEG at quality 95;
a lossless source (RAW, PNG, TIFF) exports as PNG. Re-encoding a camera JPEG to PNG inflates
it roughly fivefold without recovering quality already lost, and encoding a RAW-derived crop
to JPEG introduces the first generation of loss for no reason.

Colour is sRGB with an embedded ICC profile, converted at export from whatever the source is.

Filenames sort into placement order: `p04-z1-a3f2.jpg`, `p04-z2-8b19.png`.

**Z-order** is the order the boxes are stacked in the editor, defaulting to slot order within
the page layout. It matters only where slots overlap.

**A page with no placements exports no files.** A deliberately blank page is a legitimate
pacing device; the manifest records the page with an empty photo list so page counts
reconcile.

### 5.2 Format verification — the one open item

Pixajoy's published guidance states nothing about accepted upload formats. JPEG and PNG are
near-universal for photobook uploaders and both are ImageIO-native, so this is low risk — but
it is unverified. Confirm by opening the editor's add-photo picker and reading its accepted
types before the first real book is exported.

WebP was requested and withdrawn: macOS cannot encode it (`CGImageDestinationCopyTypeIdentifiers()`
lists no WebP type; `UTType.webP` is decode-only), so it would require linking libwebp into a
codesigned sidecar — for a format their uploader may not accept.

### 5.3 Where it runs

The Swift sidecar, over the existing NDJSON channel — an `export` request alongside
`analyze`, inheriting the pool, timeouts and crash-respawn path.

The sidecar composites nothing. It decodes each source photograph, crops it, and writes the
crop at the source's own resolution; the page layout travels separately, in `manifest.json`.
So the request carries no page canvas, no destination rect and no z-order — an earlier
revision of this section specified all three, for a transparent-PNG mechanic that was
abandoned before it was built (see §5.1).

- **Request:** `{ outputDir, items: [{ sourcePath, filename, cropX, cropY, cropW, cropH }] }`,
  where the crop rect is in the source image's own normalised coordinates.
- **Response:** one record per item, in input order — a written path with its pixel
  dimensions and byte size, or a per-file error.

Colour is **sRGB with an embedded ICC profile**, converted at render from whatever the
source is (Display P3 and AdobeRGB are both common). Not CMYK: without Pixajoy's specific
profile, converting is worse than not converting.

### 5.4 Text

**Phase 2 renders no text.** Text zones are treated as whitespace. Templates carrying them
remain usable; their text areas stay empty.

### 5.4a Projects

The app persists a book as a **project**, so progress survives quitting and a book can be
reopened, regenerated or re-exported without re-analysing the folder. This replaces the
earlier idea of exporting reference images alongside the crops: the layout is something the
user consults *in the app*, not a file they keep beside the photos.

Scope for Phase 2 is **persistence only** — the viewer is Phase 3. Concretely, a project
stores everything needed to reconstruct a book exactly:

| Stored | Why |
|---|---|
| Source folder path and the analysis run it came from | Reopening must not require re-analysis |
| The culled photo set, by content hash | The book is defined over keepers, not the raw folder |
| Page count, and the pages with their template ids | The layout itself |
| Every placement: photo, slot rect, crop rect, z-order | Reproduces the book byte-identically |
| The seed | Determinism — the same project regenerates the same book |
| Export history: when, where, which format | So a re-export can be compared against what was uploaded |

Storage goes in the existing SQLite database beside the feature cache rather than a new
file format — the cache already keys on content hash, which is what a project references.

Phase 3 renders a project; Phase 4 edits one; a Phase 4 edit sets `user_overridden` on a
slot and regeneration must not overwrite it. Phase 2 only has to make sure the data needed
for those is captured now, so those phases are not blocked on a migration.

### 5.5 The manifest

`manifest.json` in the output folder records, per photo: source path, source content hash,
crop rect, destination rect in inches, page number, z-order, output filename. Plus the
book's length, the seed, and the template chosen per spread.

This makes a book reproducible and diffable, and is what Phase 3's preview UI will read
rather than re-deriving.

### 5.6 Pre-flight

Runs before a single file is written.

| Check | Action |
|---|---|
| Face outside the trim rectangle | **Block** |
| Face inside the 0.125" safe margin | **Block** |
| Face inside the gutter dead strip | **Block** |
| Bleed slot not extending past the canvas edge | **Block** |
| Photo below **200** DPI at slot size | **Block** |
| Source file moved or deleted since analysis | **Block** |
| Estimated output size exceeds free disk space | **Block** |
| Photo between **200** and 300 DPI | **Warn**, reporting effective DPI |
| Salient non-face content in the dead strip | **Warn** |

The disk-space check is why export fails on page one rather than after nineteen.

### 5.7 UI

Deliberately minimal — the preview is Phase 3. After analysis, a **Generate book** step
shows the recommended length and the number of photos it will drop; the user accepts or
overrides; picks an output folder; watches progress. The pre-flight report appears before
anything is written, blocks listed separately from warnings. Then **Reveal in Finder**.

### 5.8 A debt this forces

`AnalysisSummary.photos` is `Vec<serde_json::Value>` with no compile-time link to the
TypeScript `AnalyzedPhoto`; a field rename on either side is a silent `undefined`.
`PROJECT-STATUS.md` already recommends generated types "before Phase 2 widens this
boundary". Phase 2 widens it considerably — the book model, the manifest and the render
request all cross it. Fix it here.

---

## 6. Template authoring

### 6.1 The measured problem

Every surviving slot was checked against the shapes real cameras produce (4:3, 3:2, and
their portrait forms):

| | Templates |
|---|---|
| Every slot fits a common photo shape | **12** |
| Some slots do | 7 — `13`, `14`, `20`, `21`, `22`, `25`, `35` |
| **No slot does** | **6** — `12`, `18`, `19`, `23`, `24`, `39` |

Slot coverage across the 71 surviving slots:

| Photo aspect | Slots that accept it |
|---|---|
| 1.00 (square) | 2 |
| 1.33 (4:3) | 14 |
| 1.50 (3:2) | 17 |
| 1.78 (16:9) | 8 |
| 0.75 (3:4) | 9 |
| 0.67 (2:3) | 8 |

The 12 fully-usable templates are **seven 1-ups, four 2-ups, one 3-up**. Nothing for 4, 5 or
6 photos. By density: seven sparse, five medium, **zero dense**.

**Why this happened.** The spread canvas is 2.518:1 — wide and short. `23-six-up-grid-2x3`
is two columns by three rows, making each cell 8.96" × 2.49" = **3.6:1**. A 4:3 photo loses
about 65% of its frame, and since face-clipping is a hard reject (§4.2), any such candidate
containing a person is thrown out entirely rather than merely penalised. In a family
photobook that is most photos. The 4-ups and 6-ups are not weak; they are inert.

Consequence: until authoring lands, a 20-page book tops out near **30 photos** against the
design's target of 60, and a handful of templates repeat.

**The 6 aspect-dead templates are removed**, alongside the 15 from §1. They are not
revisable in place: a 2×2 or 2×3 subdivision *of the spread* produces wide cells no matter
how the rects are nudged, because the spread canvas is 2.518:1. Their intent — a four-up
grid, a filmstrip band — is re-authored per §6.2 as page subdivisions instead.

**Final library state going into Phase 2:** 40 authored → 15 removed as fold-spanning → 6
removed as aspect-dead → **19 remaining**, of which 12 are usable as-is and 7 need slot-rect
revision (§6.3).

### 6.2 The authoring rule

A **page** is 1.259:1, close to 4:3. Cells matching real photos come from subdividing a
page, not the spread:

| Subdivision | Cell aspect | Suits |
|---|---|---|
| 1 photo per page | 1.26 | 4:3 and 3:2 landscape — ideal |
| 2×2 on one page | ~1.29 | 4:3 landscape — ideal |
| 2 side by side on a page | 0.63 | 3:4 and 2:3 portrait |
| 3×3 on one page | ~1.26 | 4:3, dense |
| 2 stacked on a page | 2.52 | genuinely wide photos only |

### 6.3 The work

1. **4-photo spreads** — zero usable today.
2. **5-photo spreads** — zero exist at all; the packer cannot emit a group of five.
3. **6-photo spreads** — zero usable today.
4. **Dense templates** — zero usable; all four dense ones fail the aspect check.
5. **Fold-flush and full-bleed layouts** — zero slots touch the fold, and only **10 of 71**
   declare bleed at all. The library is 86% margin; the user wants both looks available.
6. **Lively energy** — the usable 12 skew calm.

Roughly 15–20 new templates. The 7 partial templates are likely salvageable by adjusting
slot rects and should be attempted before being replaced.

### 6.4 Review process

Templates are generated to the §6.2 rule, then presented as a **visual contact sheet** — a
single page with every layout drawn to scale, slot shapes and bleed edges marked — for
approval on sight rather than by reading JSON. Rejected layouts are redrawn. Taste stays
with the user; transcription does not.

### 6.5 The page-half pool

Measured over the 25 fold-safe templates: **33 distinct page layouts** from 50 halves —
0-slot ×1, 1-slot ×14, 2-slot ×11, 3-slot ×7. Ample for pages 1 and N with no dedicated
authoring. This shrinks when the 6 aspect-dead templates are removed and grows again with
§6.3, and is recomputed at load rather than stored.

---

## 7. Testing

The governing fact is this project's own history: Phase 1 shipped **eight tests that passed
under a broken implementation**, each written into a detailed plan by an agent that believed
they were sound. The anti-patterns below are therefore requirements, not advice.

**Rust — the engine.** Golden-file tests on full book assembly; property tests on the
predicates (a rect can never be both `in_trim` and fold-spanning); SKU boundary tests; a
mutation check per soft scoring term.

Goldens run against a **frozen fixture template set** at `tests/fixtures/templates/`, never
the live library. Otherwise every template authored in §6 churns every golden and the tests
degrade into noise that gets regenerated without being read.

**Swift — the exporter.** Assertions a crop-and-write path cannot fake: the written file's
pixel dimensions really are the requested fraction of the SOURCE image's own dimensions (not
a page canvas size — nothing is scaled to one), the crop really is taken from the requested
corner of the frame rather than the centre, the encoded format follows the source (JPEG for
a lossy source, PNG for a lossless one) so a JPEG is never re-encoded as a bloated PNG, and
the ICC profile really is embedded. An earlier revision specified alpha-0 and exactly
3359 × 2668 assertions here; both belonged to the abandoned transparent-PNG mechanic.

**TypeScript.** The validator gains the fold-spanning rule and the page-decomposition check
(every half must be a valid page layout). Manifest schema tests.

**Five rules, each one a real Phase 1 bug:**

1. **No square fixtures** for anything aspect-dependent — `height/width == 1` hides the
   entire class.
2. **Aspect fixtures must have differing normalised and inch ratios**, or the test passes
   under the exact `aspect_pref` trap it exists to catch.
3. **Boundary tests use values at the boundary.**
4. **Ordering tests use unsorted input.**
5. **Every load-bearing test is mutation-checked** — break the implementation deliberately,
   confirm the test fails, paste the output.

**One end-to-end test** drives the real sidecar binary over stdin with a genuine render
request. The sidecar is a standalone CLI, so this costs almost nothing.

---

## 8. Open items

Carried as explicit unknowns, not assumptions.

| # | Item | Blocks | How to settle |
|---|---|---|---|
| 1 | ~~**Pixajoy alpha behaviour**~~ — **CLOSED 2026-08-14, no longer relevant.** The transparent-PNG mechanic was abandoned (§5.1): the book must be the user's own images, not synthesised pages. Nothing now depends on alpha. | — | — |
| 1a | **Accepted upload formats** — does Pixajoy's picker take JPEG and PNG? | Nothing structurally; both are near-universal and ImageIO-native. Verify before the first real export. | Open the editor's add-photo picker and read its accepted types (§5.2). |
| 2 | ~~**Post-orientation dimensions**~~ — **RESOLVED 2026-08-13 during planning.** `ExifReader.swift:40-41` already swaps width and height for orientations 5–8 before building `PhotoFeatures`, so `width`/`height` are post-orientation and the scorer can use them directly. No work needed. | — | — |

One further fact established while planning, worth recording because it
shaped the plan: **the full Swift feature record is already available to
Rust.** `ResponseResult::Analyzed` carries it as `serde_json::Value` and the
SQLite cache stores it whole; only `partial_photo` narrows it for the
webview. So the scorer reads face boxes, the saliency box and the palette
without any change to the analysis pipeline or the wire format.

A gap found the same way: **`templates/` is not in `bundle.resources`**, so a
packaged `.app` ships no template library at all and would produce a
zero-page book. The plan fixes this in its Task 3.

---

## 9. Explicitly out of scope

- **Multiple source folders.** Requested and flagged in `PROJECT-STATUS.md`; it is a design
  change to ranking population semantics, not a flag. Its own spec.
- **The cover.** Different geometry — a wrap band plus a page-count-dependent spine.
- **Text rendering** (§5.4), **the preview UI** (Phase 3), **the canvas editor** (Phase 4),
  **the agent layer** (Phase 5).
- **Smile calibration.** Still deferred; `smile_fraction` is excluded from culling (§3.1).
