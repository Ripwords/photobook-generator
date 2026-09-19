# PhotobookGen — Project Status

**Last updated:** 2026-09-19
**Branch:** `master`

This document exists so a new agent can pick the project up without re-deriving what
was already learned. Read it before touching code. The authoritative documents are:

- `docs/superpowers/specs/2026-08-12-photobook-generator-design.md` — the overall design
  and its constraints. Still accurate, **except** for the export design, which was
  deliberately revised mid-Phase-2 (see "Phase 2" below).
- `docs/superpowers/plans/2026-08-12-phase-1-analysis-pipeline.md` — the Phase 1 plan.
  **Sections of it are wrong and carry `⚠️ SUPERSEDED` warnings.** Heed them; following the
  original text reintroduces two real bugs.

A blow-by-blow record of every fix round, ruling, and deferred finding is in
`.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/progress.md`,
`.superpowers/sdd/2026-08-14-phase-2-export-revision/` and
`.superpowers/sdd/2026-08-15-phase-2-completion/progress.md` (git-ignored, local only). They
are long but they are where the reasoning lives.

**If you read only one section, read "Phase 2 open items" — every entry there is a real,
deliberately-parked decision that this file is the only surviving record of.**

---

## What changed on 2026-09-18/19

Drafts: a book is named first, and its analysis outlives the screen it started on. 845
TypeScript tests, `bun run lint` and `check:build` clean; a Playwright run against the mock
harness (`bun run ui:mock`) covers the flow end to end in light and dark.

1. **Several analyses at once, in Rust** (`350d6be`). The analysed-photo cache is a map by
   `run_id`, not one slot, so a second analysis no longer makes the first draft's toggles
   fail with "comes from a different analysis". `forget_run` drops a set. The sidecar is
   locked per batch (`with_sidecar`, tokio's FIFO mutex), so two runs alternate batches; on
   std's mutex the fairness test saw one run hold it for all 20 batches.
2. **Jobs live in a store, not in the select screen.** `app/composables/useAnalysisJobs.ts`
   replaces `useAnalysis.ts`. A job owns its folders, name, streamed state and the user's
   include/exclude decisions; `usePhotoOverrides` takes the job's `overrides` ref and, on
   remount, has Rust re-stamp them. A per-job attempt counter drops a superseded run's late
   events, and a run whose job was removed mid-flight is forgotten when it lands (Rust cannot
   be stopped mid-gather). The leave guard and "Discard this selection?" modal are gone:
   leaving loses nothing. `Screen` is `{ kind: "select"; jobId }`.
3. **UI.** `NewBookDialog.vue` asks the name before the folders. The sidebar has a Drafts
   group (spinner and `processed/total`, Ready, Failed; `aria-busy`), a toast with **Open**
   when a draft finishes off-screen.

   **Drafts are saved (2026-09-19).** They were memory-only, with a quit confirmation; a
   user lost a draft to a restart and asked for them to persist. `drafts (id, json,
   updated_at)` holds each draft as JSON that Rust never reads (`list_drafts`,
   `save_draft`, `delete_draft`). `useAnalysisJobs` watches every draft's
   `savedDraft(job)` JSON and writes only the ones that changed, one write at a time;
   `restoreDrafts()` on mount re-adds them and re-analyses. A draft quit mid-run saves the
   decisions it was restored with, not an empty set, because its live overrides are not
   filled until `done`. The quit confirmation is gone; `onCloseRequested` awaits
   `flushDrafts()` and the window then closes itself (Tauri's handler calls `destroy`, so
   `core:window:allow-destroy` stays). Mutation-checked in `tests/jobs.test.ts`: always
   saving live overrides, dropping restored decisions, keeping a colliding id, never
   deleting.

4. **Speed, measured on a real folder** (27 camera JPEGs, 83 MB, M-series with 4P+6E cores,
   release build, median of runs from `cargo run --release --example analyze_bench`).
   Each row is one commit; a change is kept only if it wins here.

   | Change | Cold | Warm (all cached) | Hash + lookup |
   |---|---|---|---|
   | Baseline | 544 ms | 161 ms | 174 ms |
   | `sha2` `asm` feature (ARMv8 SHA instructions) | 419 ms | 44 ms | 44 ms |
   | Hash each chunk's files on every core (`hash_files`) | 381 ms | 10 ms | 9 ms |

   Hashing was a third of a cold run and all of a warm one; the sidecar's share (~365 ms
   cold) is Vision and decoding. Overlapping a chunk's hashing with the previous chunk's
   sidecar call (planned) was dropped: hashing is now ~9 ms of a cold run, the most it
   could save.

   **An unchanged file is not read again (2026-09-19).** On a real external drive the
   table above did not hold: 283 Sony ARW files on `/Volumes/Universal` (exFAT, USB),
   every one already analysed, took **72.96 s** to reopen, all of it `hash+lookup`. The
   SSD folder above was fast because it was small and already in the OS file cache.
   `file_stamps (path, size, modified_ns, hash)` now records each file's size and
   modified time when it is hashed, and `lookup_cache` reuses the hash when both still
   match, without opening the file. Same folder, `PBG_BENCH_SEED_DB` = a copy of the
   app's database: first run 72.96 s (no stamps yet, so every file is read once), then
   **17.9 ms** and 6.2 ms. An edit that keeps a file's size and modified time is not
   noticed; tests pin that a new size or a new time is (`a_file_with_a_new_*`), and
   comparing either alone fails one of them.

   Export, same folder, 4 sidecar calls of 8 items as `export_book` sends them, median of 5
   (the harness pipes `export` requests straight into the built sidecar):

   | Change | Export time | Peak memory |
   |---|---|---|
   | Baseline, one item after another | 1625 ms | 990 MB |
   | `Exporter.export` runs 4 items at once (`concurrencyLimit`) | 555 ms | 1189 MB |

   A limit of 8 (the whole batch at once) measured 453 ms at 1372 MB. It was not taken:
   every extra item in flight holds a full-resolution decode and its sRGB copy, about
   45 MB each for these 24 MP files and several times that for a large RAW. Paths are
   claimed in request order before anything is decoded, so a filename collision is still
   won by the first item in the request, not the first to finish
   (`exporterCollisionIsWonByRequestOrderNotByWhoFinishesFirst`; claiming on completion
   fails it). One consequence: an item whose source is unreadable still holds its path.

   Measured and not done:
   - **Passing Rust's hash to Swift.** Swift's own hash averages 2 ms of the 54 ms a photo
     costs in the sidecar (`scripts/benchmark.sh`), and it runs across cores. Removing it
     saves a few ms per cold run, less than the run-to-run noise, for a protocol change.
   - **The batch barrier.** Analysing the folder as one chunk, with no barrier between the
     4, 8 and 15-photo ramp chunks, took 346 ms against 381 ms. The ramp is what puts the
     first tiles on screen after 4 photos, and 35 ms does not pay for losing that.

Not done: the "Discard draft…" item in the sidebar row's menu (discard is on the draft's own
toolbar only).

## What changed on 2026-09-16

One session on `master`, thirteen commits, every suite green at the end (633 TypeScript, 443
Rust unit tests plus the sweep and five live-sidecar binaries, 130 Swift, lint and
`nuxt generate` clean, and the packaged app builds). In the order it happened:

1. **The packer was redesigned, closing open items 12 and 14** (`23d757f`). `Buildable` is per
   side (`single_left`, `single_right`), so the opening page is measured against right halves
   and the closing page against left ones; `Capacity::from_library(20)` reads the true **63**,
   not 64. `apportion_slots` measures each chapter's `need` and `cap` against the kinds of slot
   it will actually occupy (`chapter_slot_bounds`) and settles whether the book fills before
   crediting the last chapter with the closing page. New: `merge_chapters_the_slots_cannot_seat`
   folds adjacent chapters while the slots cannot seat every photo, or cannot all be filled
   although the photos could fill them — a third defect the sweep surfaced (26 tiny chapters
   into 11 slots used to drop 15 chapters whole). The include-priority ranking in
   `apportion_slots` and the `seatable` trim in `pack` were removed: with `sum(need) <= slots`
   guaranteed, neither could change an outcome. **Measured with the new
   `src-tauri/tests/pack_sweep.rs`** (real library, 20 pages, 10..=65 keepers, four chapter
   shapes, three seeds, 672 books): photos lost **2640 -> 36** (all 36 at 64 and 65 keepers,
   above capacity), blank pages **894 -> 840**, every remaining blank page in a book with
   fewer photos than the slots' minimum fill. The sweep asserts both properties and a
   blank-page ceiling, and takes ~24 s; it is the instrument to run after any packer change.
   Golden `book-20.json` moved 24 -> 25 placed, 6 -> 5 dropped (the overflow on page 1 is gone).
2. **Open item 13 closed** (`2bd4e66`): `tests/templates.test.ts` fails on a slot overlapping
   a text zone.
3. **The scan walks subfolders** (`847243f`), skipping hidden directories and directory
   symlinks. The live-sidecar tests analyse an isolated top-level copy of the fixtures because
   the walk now reaches `sidecar/Fixtures/hostile/` (`693a78d`).
4. **Phase 3 is built** (`b8fa700`): `book::edit` and the `edit_book` command. See "Phase 3:
   spread-level controls" below.
5. **A browser harness** (`574df3c`): `bun run ui:mock` serves the webview with the Tauri bridge
   stubbed from the wire fixtures, so a UI change can be rasterised and driven with a browser
   automation tool. The spread controls were verified that way — selection ring, swap alert, the
   swap exchanging two photo indices, the locked badge with the other controls disabled, the
   layout menu — not only by tests.
6. **Several folders are one analysed set, and the `AppState` collision is fixed** (`a84ba97`).
   See "Multiple source folders" below; the old "Requested, not yet specced" section is gone.
7. `no-explicit-any` is a lint error; the dead `Sidecar::benchmark` is removed (`4e69085`).
8. **Phase 4, first two steps** (`168d853`, `a9d8324`): hand cropping (drag the photo, scroll
   to zoom) and moving/resizing the boxes with snap-to-guide, both through `edit_book` under
   the same hard constraints as every other edit. See the Phase 4 row under "What is NOT
   built" for exactly what is and is not there.
9. `bun tauri build --bundles app` still produces `PhotobookGen.app` (run after item 6).
10. **The UI was restructured into three screens.** The whole app used to be one page:
    `index.vue` derived a `ViewState` and long-scrolled the contact sheet, the generate panel,
    the book preview, the export report and a saved-books list into a single column. An open
    project had no exit, the saved-book list was reachable only before the first analysis, and
    "Edit the selection" was gated on the project carrying overrides, so a book generated with
    none had no path back to its photos. Navigation is now one discriminated union,
    `Screen` in `app/types/navigation.ts`, held in a single ref in `index.vue` (88 lines, a
    router and nothing else). The screens are `ProjectLibrary`, `SelectPhotos` and
    `BookEditor`, sharing `AppHeader`; `GenerateBook` is now only the "pick a length and
    generate" panel. Two behaviours are new rather than moved: the editor states that every
    change is already on disk (it always was, nothing said so), and because `generate_book`
    always INSERTs, re-editing a selection now offers **Update** (which deletes the superseded
    row) beside **Save as a new photobook**, instead of silently leaving two identically-named
    books in the list. **The proper fix for that is Rust-side**: an update path that rewrites
    the project in place would keep the export history the Update button currently destroys.
    The browser harness (`dev/tauri-mock/`) now answers every command the UI can reach, so all
    three screens can be driven end to end; that is how this was verified.

**Still not done, and not fakeable:** the real-photo run and Pixajoy upload (see "The
outstanding verification"). Nothing in this session changed that. The four scoring terms still
ship at weight `0.0`. Phases 4 and 5 are not built.

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

**Working end to end:** point the app at a folder → it hashes each new or changed file, consults a SQLite
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
| Nuxt UI | `app/` | `pages/index.vue` routes over `types/navigation.ts`'s `Screen`; the screens are `components/ProjectLibrary.vue`, `SelectPhotos.vue`, `BookEditor.vue` |
| Template library | `templates/` | Spread templates + validator at `tests/templates.test.ts`. Was 40 at end of Phase 1; **now 36** — see the Print geometry section. |

**Test counts at last run (2026-09-18, `master`):** 834 TypeScript, **485 Rust lib tests, 0
failed, 0 ignored**, plus the `pack_sweep` binary, 2 other harness'd and 4 `harness = false`
integration binaries, 130 Swift. All green, lint clean.
**Release build works:** `bun tauri build --bundles app` produces `PhotobookGen.app`.

### Unplanned additions beyond the plan

- **Thumbnails.** The sidecar writes a 400px JPEG per photo keyed by content hash and
  returns its path; the webview loads it via `convertFileSrc` and the asset protocol.
  Without this the UI showed a table of file paths, which is useless for judging photos.
- **Desktop shell, monochrome theme.** A sidebar (library, every book, settings), a
  per-screen toolbar under an overlay title bar, a Settings dialog (appearance, shortcuts,
  API keys, about) and one shortcut table (`app/types/shortcuts.ts`) that drives the key
  bindings, the tooltips and the list in Settings. Neutral black and white throughout
  (`primary: "neutral"`); colour is kept for print guides and errors. The contact sheet
  shows photos, not a table of file paths.
- **App icon.** `app-icon.png` is masked to Apple's macOS squircle (superellipse n=5,
  824px inside a 1024px canvas). Regenerate the set with `bun tauri icon app-icon.png`.
- **Completion notification** for long analysis runs, posted from Rust so no JS is
  involved. Gated on two conditions: elapsed ≥ `NOTIFY_MIN_ELAPSED` (10s) **and** the
  window is unfocused. Failure is non-fatal — a user who declines notifications still
  gets their analysis.

---

---

## Current state: Phase 2 is complete

Built across three plans and thirty reviewed tasks — the last fourteen on
`feat/phase-2-completion`, which closed open items 1, 3, 9 and 11 plus item 4's remaining
page-half defect, corrected item 6 (already closed by `3e934e9`, the entry was stale), and
added four scoring terms that all ship **inert at weight `0.0`**. The app now turns an
analysed folder into files you can upload to a printer.

**Working end to end:** analyse a folder → pick a page count (the app recommends the
smallest SKU that fits, and says how many keepers each length would drop) → generate →
pre-flight → export. Generating **saves the project**; exporting writes one cropped image
file per placement.

| Stage | Where | What it does |
|---|---|---|
| Cull | `book::cull::cull` | Drops utility images, keeps one winner per near-duplicate cluster. **The single authority** — see below. |
| Pack | `book::pack` | Chapter-aware grouping into buildable group sizes, **distributed over the available slots rather than front-loaded** and **sized against the kind of slot each group will land on** (a single page holds a page half's worth, a spread does not); drops the lowest aesthetic percentiles when keepers exceed capacity. |
| Score | `book::score` | Scores each template against a group: aspect fit, saliency and face-area retention, hero match, resolution headroom, palette harmony, variety — plus `face_quality`, `spread_diversity`, `gutter_saliency` and `hero_prominence`, **all four at weight `0.0`**. Hard rejections for a face in the gutter, a face outside the safe margin, and sub-`MIN_DPI` resolution. |
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

**Format rule** (`Exporter.outputFormat`): a **lossy source (JPEG, HEIC, HEIF) exports as
JPEG at quality 0.95**; a **lossless source (PNG, TIFF, RAW) exports as PNG**.

The rule covers WebP and TIFF, but **the app cannot ingest either**: `commands::SUPPORTED`
accepts only `jpg jpeg png heic heif cr2 cr3 nef arw dng raf orf`. So a WebP or TIFF branch
in the format rule is dead code today, not a supported input path. (macOS cannot encode
WebP at all — see the export-revision spec §5.2.)
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

### User-controlled selection (built after Phase 2, on top of that one authority)

The engine no longer decides alone. Every photo carries one of three states,
keyed by **content hash**: `Auto` (the engine decides, the default), `Include`
(in the book whatever the engine thinks) and `Exclude` (out of it).

**Cull proposes; the user disposes.** `book::cull::cull(photos, overrides)` is
still the single authority — it now takes the decisions and honours them:

| Rule | Where |
|---|---|
| `Exclude` never survives, and its burst promotes the runner-up | `cull` (excluded before the cluster contest) |
| `Include` always survives — a lost near-duplicate, an `is_utility` image, anything | `cull` |
| An `Include` is **additive**: it does NOT displace the `Auto` winner of its own cluster | `cull` |
| An `Include` is never trimmed to capacity; the weakest `Auto` photo goes instead | `book::pack::pack` |
| A crowded chapter strands an `Auto` photo, and a chapter holding an `Include` is apportioned a slot first | `pack`, `apportion_slots` |
| `Include` photos that **alone** exceed capacity are REPORTED, never cut | `pack` -> `IncludeOverflow` |
| A book that lost an `Include` by ANY other route is refused | `pace::assemble`'s post-condition -> `BookError::IncludedNotPlaced` |

**On the cluster ruling.** Including a photo whose cluster winner is `Auto`
keeps BOTH. An `Include` is a statement about that photo only; making it
displace the winner would mean ticking one photo silently removes a different
photo the user never touched — the same class of surprise the feature exists
to remove — and `Exclude` already exists for saying "not that one".

**The post-condition is the load-bearing guard, not the per-site ones.** There
are four places a photo can be lost (capacity trim, chapter apportionment,
page-half overflow, a template rejecting it outright) and the last two are not
individually preventable. `assemble` therefore checks the FINISHED book and
refuses to return one that is missing a photo the user asked for. Proven live:
with the capacity check in `pack` deleted, the post-condition still catches the
loss — it degrades from a precise message to a vaguer one, never to a silent
drop.

**Do not reintroduce a second frontend rule.** An override toggled in the UI
goes DOWN to Rust (`commands::apply_photo_overrides`, which re-runs
`stamp_kept`) and the records come back with `kept` re-stamped.
`usePhotoOverrides` is the only place that happens, and
`tests/overrides.test.ts` pins it with a mock that returns a verdict
*disagreeing* with the override, so a composable applying the decision itself
produces different output from one that forwarded it.

**Persistence.** `project_photo_overrides (project_id, hash, state)`, written in
the same transaction as the project row and its photo list, returned on
`ProjectDetail.overrides`. `Auto` is never stored — it is the absence of a
decision, on both sides of the wire and in SQLite — and an unrecognised state
token fails the load rather than degrading to `Auto`, because a silently
forgotten decision is invisible.

**Two things the earlier docs got wrong, corrected here:**

- The contact sheet did **not** "render every photo with a keeper marker" — it
  rendered `groupByEvent(keepers(...))`, i.e. only the survivors. A photo the
  engine dropped was invisible, and an invisible photo cannot be asked for, so
  the include control had nothing to act on. `index.vue` now renders the whole
  analysed set with the left-out ones dimmed, behind a "Show the N left out"
  switch that defaults to ON.
- `vitest.config.ts` now aliases `~` to `app/`. Without it, no file under
  `app/` that imports `~/types/...` could be unit-tested at all.

**The toggle is cheap, and `AppState` is why.** `AppState.photos` holds the
parsed `Vec<Photo>` from the end of `analyze_folder`, so
`apply_photo_overrides` and `recommend_book` take only the override map --
the first returns the surviving PATHS, not re-stamped records. Sending the
records instead was ~2.5 KB per photo, three uploads per click (the toggle
plus two `recommend_book` watchers that both fired), i.e. ~10 MB per click on
a 1000-photo folder. Measured after: 10.9 KB at 200 photos, 26.6 KB at 500,
52.8 KB at 1000 — roughly 180x smaller and, unlike before, near-independent of
what a feature record carries.

*Overrides are hash-keyed; the verdict is path-keyed.* A decision is about the
photograph, so it follows the bytes. The verdict is about the file, and a
content hash is **not** unique within one analysis — two byte-identical files
share one, and `cull` keeps only one of them, so a hash-keyed verdict would
mark both copies kept. `stamp_kept` and `pace::assemble` key on path for
exactly this reason.

*An empty cache is reported, never papered over.* Its lifetime is tied to the
webview's own copy rather than managed: both live in this process, and a
reload loses `useAnalysisJobs`' summaries at the same moment it would invalidate
the cache.

*It used to be a single slot, keyed since 2026-09-16 and a map by run since 2026-09-18,
so several drafts can each be overridden.* Before that it was UNKEYED, a real forward hazard.
**Fixed 2026-09-16:** the cached set carries the `run_id` of the analysis that
produced it, every reader hands back the id its own summary carried, and a
mismatch is refused (`commands::select_run`). See **"Multiple source folders
(built 2026-09-16)"** for the whole decision.

**The contact sheet is virtualized (2026-09-19).** `ContactSheet.vue` renders only
the rows in view (`@tanstack/vue-virtual`); `types/sheet.ts` cuts the photos into rows.
Row heights are computed, not measured. With 3,000 photos in the browser mock,
opening the draft went from 5,984 ms and 56,668 DOM nodes to 134 ms and 524.
The left-out photos are therefore shown by default at every size. The
`showsLeftOutByDefault` threshold of 200 existed only because the grid was not
virtualized, and it is gone.

**A reopened project's selection is visible and editable.** The panel states
it (`selectionLabel`) and offers "Edit the selection", which re-analyses the
project's own folder (all features-cache hits, no Vision work) and restores
the saved decisions through the same Rust round trip a fresh click uses.
Returning `overrides` over the wire and rendering nothing was the same
"persisted but unreachable from the UI" defect that started this line of work.

**Watch `photoSetId`, never the photos array.** `usePhotoOverrides` replaces
`photos` on every toggle. `GenerateBook`'s reset branch discards the generated
book, the opened project, the output directory, a typed name and a chosen page
length — keyed on the array, it did all five on every click. `photoSetId`
bumps once per analysis and is the identity to watch.

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
  re-run it on an edited book without touching the filesystem. **This is now literally
  true** — the last `Path::exists()` moved out to the shell on 2026-08-15, which hands the
  core a set of already-known-missing paths. See open item 9.
- **`best_spread` takes a seed**, so spread selection is seedable and "regenerate" is
  implementable. **One caveat, and Phase 3 must plan around it:** a single global seed feeds
  every call, so every 2-candidate tie in the book resolves the same way and changing the
  seed re-rolls the whole book together. Per-spread regeneration needs a per-spread seed
  mixed in at the call site. See open item 3.
- **Geometry predicates are property-tested** (`geometry.rs`), so a canvas editor can snap
  against `in_trim`, `in_safe_margin` and `clear_of_gutter` rather than inventing its own.

---

## Phase 2 open items

Every one of these is real. Each was found during Phase 2, deliberately parked with a
ruling rather than forgotten, and **this file is the only place it survives.** None of them
blocks the phase; several block a later one, and those say so.

**Status as of 2026-08-15.** Closed: **1**, **3**, **9**, **11**, and item **4**'s remaining
"Still open, smaller" paragraph. Item **6** was never open — it was closed by `3e934e9` and
this ledger simply had not caught up. Item **10** was already nothing. **Still open, rulings
intact: 2, 5, 7, 8.** Two new items, **12** and **13**, are added at the end; item 12 is the
highest-value follow-up in the project.

### 1. The gutter-saliency penalty does not exist — **FIXED 2026-08-15**

The spec promised a **penalty** for saliency falling in the gutter, and only the other half
was implemented: a face in the gutter is a hard rejection (`book::score`,
`clear_of_gutter`), with `score_does_not_reject_generic_saliency_in_the_gutter_strip`
pinning that generic saliency is deliberately *not* a rejection. No `Weights` field
implemented the graded penalty.

It exists now, as the `gutter_saliency` weight — the fraction of the surviving saliency box
that falls in the dead band, penalised. The layout-engine design's §4.3 table carries the
row, so the §4.2-vs-§4.3 mismatch that this item was really about is gone.

**It ships at weight `0.0`.** See "The four new scoring terms are DORMANT" below before
reading this as a change to how books look.

### 2. `palette_harmony` is effectively inert

It computes hue as `atan2(b, r)` over **sRGB**, not the Oklab hue the spec asks for. Its
circular-variance floor is about **0.707**, and at weight `0.2` it therefore contributes
roughly **1% of a spread's score** — it can essentially never change which template wins.

**Ruling: parked pending real-photo calibration.** Fixing the colour space without
measuring is guesswork; the term does no harm while inert. Do not spend effort here before
the real-photo run below.

**Added 2026-08-15:** `spread_diversity` now supersedes the *need* for this term. Complaint
3 below is that a spread holds too many similar photos, and `palette_harmony` rewards
exactly that similarity — so the two terms pull against each other by construction. Trading
one against the other (lower `palette_harmony`, raise `spread_diversity`) is the obvious
first move of the tuning session, and it costs nothing but a `weights.json` edit. The item
stays open because neither number has been measured on real photographs.

### 3. The seed never reaches spread selection — **FIXED 2026-08-15**

`assemble(photos, pages, lib, w, seed)` used to pass `seed` only to `single_page` →
`best_single` → `tie_break`; `best_spread` took no seed at all, so the seed influenced only
the first and last pages and never a middle spread. `best_spread` now takes a seed and
breaks its ties with it. This is no longer a Phase 3 blocker.

**But it does not by itself make one spread re-rollable, and Phase 3 must know that.** ONE
global seed feeds every `best_spread` call, so `tie_break(seed, 2)` resolves identically at
every 2-candidate tie in the book. It is observable in the golden: re-baselining it after
this change flipped **all six** symmetric spreads together, not one. "Regenerate THIS
spread" therefore needs a per-spread seed — `seed ^ spread_index`, or similar — mixed in at
the call sites. The signature accepts a seed, so the work is a call-site change rather than
a plumbing change, but it is still work Phase 3 has to do.

### 4. `choose_group_size` front-loaded — **FIXED 2026-08-14**

The greedy packer took the largest buildable size that fit, guarded by a lookahead asking
whether the remainder stayed decomposable. Once the library covered every size 1–6 that
guard went **inert** — `1` is buildable, so `is_decomposable(rest)` is true for every
remainder — and the largest candidate always won. Photos were packed six to a spread until
they ran out and every remaining slot was left blank.

`pack` now does three things:

* **sizes each group against how many photos and slots remain** (`remaining / slots_left`,
  rounded half up) rather than against the largest size that fits;
* **apportions the book-wide slot budget across chapters before cutting any of them**
  (`apportion_slots`): one slot each longest-chapter-first so no chapter vanishes, then
  every chapter up to the `ceil(count / largest)` it *needs* to seat all its photos, then
  D'Hondt highest averages for whatever is left;
* **bounds every choice by a feasible band** (`feasible_band`) — take less than `lo` and a
  photo is stranded, take more than `hi` and a later slot comes out blank. The band
  overrides every stylistic preference, which is what makes "every slot filled and every
  photo placed" hold by induction down a chapter.

Measured end to end through `pace::assemble` against the **real** `templates/` library,
20 pages:

| photos | keepers | before: groups / placed / blank pages | after: groups / placed / blank pages |
|---|---|---|---|
| 14 | 12 | `[3,4,5]` / 11 / **16** | 11 groups / 12 / **0** |
| 24 | 20 | `[3,4,6,6,1]` / 20 / **12** | `[1,2,1,3,1,4,1,3,1,2,1]` / 20 / **0** |
| 30 | 25 | `[3,4,6,6,2,4]` / 25 / **10** | `[3,3,1,3,1,2,1,3,1,3,4]` / 25 / **0** |
| 40 | 34 | `[3,4,6,6,2,6,3,4]` / 34 / **6** | `[3,4,2,4,2,4,2,4,2,3,4]` / 34 / **0** |
| 60 | 51 | 11 groups / 51 / 0 | `[3,4,6,5,3,6,3,6,6,6,3]` / 51 / 0 |
| 120 | 102 | 11 groups / **43** / 0 | 11 groups / **52** / 0 |

Re-measured against `eb56f35` on 2026-08-14: placed = 12 / 20 / 25 / 34 / 51 / 52 and blank
spread slots = 0 for every row, reproducing the table exactly. Never worse on either axis,
and at 120 photos it places 9 more of them. (The seed does not reach the packer, so all
three of seeds 1, 9 and 1234 give identical figures — see the open item on that.)

**"0 blank pages" here means "no spread slot went unfilled". It does not mean every printed
page carries a photo, and the difference is large.** That gap is what
`feat/phase-2-completion` closed — see "Blank printed pages: what the completion branch
actually did" below. The historical finding, kept because it is the reasoning behind that
branch: ten of the 36 templates put all their slots on ONE page half and left the other
empty — `03`, `04`, `05`, `06`, `20`, `28`, `30`, `36`, `41`, `45` — so every time the
packer picked one, a full printed page came out white. Measured on the same runs above, the
count of printed pages with zero photos was **8 / 4 / 5 / 1 / 2 / 1** for 14 / 24 / 30 / 40
/ 60 / 120 photos. The golden did it twice, both from `03-hero-left-text-right`.

**Both causes are gone.** Size 1 is banned from the spread region, and the two MULTI-photo
offenders (`20`, `45`) were re-authored to span both halves. Eight templates still have an
empty half — `03`, `04`, `05`, `06`, `28`, `30`, `36`, `41` — but all eight hold exactly one
photo and are therefore only ever drawn as page halves now, never as a spread. The golden is
at 0 pages with empty placements, and
`pace_no_printed_page_names_a_real_template_and_holds_nothing_in_the_real_library` asserts
the property against the shipping library rather than a fixture.

The pinned
regression is `pack_fills_every_slot_rather_than_front_loading_the_first_spreads` (30
photos, 9 spreads + 2 singles, `buildable = 1..=6`), which fails with
`11 slots but only 5 groups: [6, 6, 6, 6, 6]` against the old rule.

The golden fixture (`src-tauri/tests/fixtures/book-20.json`) went from **2 blank pages to
0** with **no other regression**: 24 placements and 6 dropped, both identical to before,
across the same 7 distinct templates. The blank spread at pages 18-19 is simply filled now.

**Density deliberately varies rather than converging.** Aiming every slot at exactly the
fair share makes every group the same size, and density is a strict function of slot count
across this library (1 -> sparse, 2..4 -> medium, 6 -> dense) — so a book of equal groups
has ONE density and `pace::repace` cannot vary an axis the packer has already flattened.
Measured on the real library at 30 photos, aiming at the bare share gives every spread 2
photos; the packer therefore aims one photo either side of the share, alternating across
the whole book, which yields spreads of `[3,1,3,1,2,1,3,1,3]`. The swing is a preference
only — the feasible band overrides it — so it costs neither a slot nor a photo. Guarded by
`pack_varies_group_size_across_a_book_instead_of_converging_on_the_average` and, at the
book level, `pace_spread_density_varies_across_a_book_rather_than_converging`, which
asserts on the library's declared `density` rather than on photo counts (2, 3 and 4 are all
`medium`, so a count-based assertion passes while the axis is still flat).

**What that density test now covers — 2026-08-15, and do not mistake it for more.** It was
`#[ignore]`d during the completion branch (with size 1 banned from spreads, every buildable
spread in its library was `medium`, so the axis was flat by construction) and is now
un-ignored and green. But it runs against a **purpose-built synthetic library whose declared
density is one-to-one with slot count**, not against the real `templates/` directory and not
against the frozen fixtures it used before. That bijection is asserted, so "density varied"
provably means "group size varied" — which makes it a sound guard on a `pack` property. It
is **not** real-library density coverage, and must not later be read as such. The prior
plan's assumption that rebalancing real templates would restore it was simply wrong: the
test never loaded the real library.

**Still open, smaller — FIXED 2026-08-15.** `pack` used to size every group by SPREAD
counts even though two of the eleven slots are single pages holding only a page half's
worth, so a group landing on one was trimmed by `pace::strongest`; in the golden that cost
one photo on the opening page. Groups are now tagged by slot kind (`slot_kind_at`) and sized
against the capacity of the kind of slot they will land on, and `Capacity` was split to
match. The golden is back to 24 placements and 6 dropped with 0 pages holding no placements.

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

### 6. Persistence half-reachable from the UI — **STALE ENTRY, already closed by `3e934e9`**

The claim was that the UI calls `list_projects` but never `open_project`, so Export was only
offered for the book generated in the current session. **That has not been true since
`3e934e9`.** `useBook.ts:162` invokes `open_project` and applies the result
(`applyBookState(withOpenedProject(project))`, then `loadLayout`), and a reopened project's
selection is visible and editable — see "A reopened project's selection is visible and
editable" above, which was written about that very fix.

Recorded as a correction rather than deleted, because the failure mode matters: this entry
sat here open for a day after the code closed it, and a reader trusting the ledger would
have re-implemented a working feature. **`feat/phase-2-completion` did not fix this; it
found the entry lying.**

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

### 9. `preflight_core`'s doc comment is wrong about I/O — **FIXED 2026-08-15**

It said "No I/O" while performing one `Path::exists()` per placement (the moved-source-file
Block check), so the "pure core" was not callable in a hot loop or off-disk.

The check moved out to the shell, which now gathers the missing paths into a set and hands
that set to the core. `preflight_core` is genuinely pure and re-runnable off-disk: Phase 3
can call it on every edit of a book without touching the filesystem, and re-stat only when
it wants to. The pre-existing `preflight_blocks_a_source_file_that_no_longer_exists` was
mutation-checked after the refactor to confirm it still reaches the rule rather than having
gone inert — deleting the rule fails both it and the new core-level test.

### 10. Culling is fully reconciled — nothing remains

Recorded here because the ledger asked for whatever Step 2 concluded. The two culling
authorities are now one (see "One culling authority" above); `keepers()` carries no rule,
and Rust's verdict travels on the wire as `AnalyzedPhoto.kept`. **Nothing is outstanding on
this item.**

### 11. `from_features`' new strictness fails silently on two paths — **FIXED 2026-08-15**

`from_features` refuses a feature record missing a field the book depends on, and
`photos_from_records` and `resolve_photos` surfaced that loudly — but `stamp_kept` and
`count_keepers` consumed it through `filter_map`, so a malformed record was silently
skipped: the photo came back `kept: false`, or the completion notification undercounted,
with no error anywhere.

Both now **return the count of refused records alongside their result, and their callers log
it.** A refusal is no longer invisible on any path. The related cosmetic error — a fixture
doc attributing the `filter_map` to `from_features` rather than to its caller — was
corrected at the same time.

### 12. `apportion_slots` is slot-kind-blind, and it is the cause of the remaining blank pages — **FIXED 2026-09-16**

**Closed by `23d757f`**; see "What changed on 2026-09-16" for the measurement. `apportion_slots`
now measures every chapter against the kinds of slot it will occupy, and chapters that
cannot fill the slots between them are folded together. The history below is kept because
the mechanism it describes is why the fix has the shape it has.

Added 2026-08-15. **This was the highest-value remaining follow-up in the project.**

`apportion_slots` divides the book-wide slot budget across chapters. To decide how many
slots a chapter could possibly need it uses a floor taken from `Buildable::union_bounds()`,
which is **`1`** — because the SINGLE set starts at 1. But nine of a 20-page book's eleven
slots are spreads, whose floor is **`2`**. So a chapter of 8 photos is apportioned 5 slots
that between them need 9 photos, and the slots it cannot fill come out white. **That is the
direct cause of the 45 remaining `blank` pages** measured below. Making `apportion_slots`
slot-kind-aware would plausibly take the count back toward the pre-branch 6.

It was deliberately **not** done in `feat/phase-2-completion`, and the reasoning is worth
keeping: `apportion_slots` is the most delicate function in the packer — it carries the
`Include`-protection guarantees and the D'Hondt fairness that `pace::assemble`'s
post-condition depends on — and `pack`'s contract had already been changed twice in that
branch, each change surfacing a defect that only a whole-library sweep caught.

**Corrected 2026-08-15 — the reasoning above used to end "Every photo is placed today, so
what remains is pacing, not data loss." That is false and the correction matters.** It is
true only over the keeper range the 18-book sweep below actually measured (10–51
keepers). From about **41 keepers upward in a 20-page book, photos ARE lost** — up to 6 of
64 — and blank pages stay at zero throughout, so this is a *different* mechanism from the
one item 12 describes: every slot is filled and the photos are stranded *inside* chapters.
The full 40..=64 sweep is **open item 14**. What remains here is still pacing; data loss is
item 14's problem, not this one's.

**The same floor already cost three photos once, in this branch.** The first sweep measured
471 of 474 keepers placed against a pre-branch 474. Mechanism: the band collapses to
`hi = 1`, every candidate ties on `outside`, the tie falls through to `aim = share + swing`,
and a remainder of 1 lands on a Spread slot the packer was just taught it cannot build — so
the chapter is abandoned without ever reaching the closing Single that holds exactly one.
Fixed by an `overshoot` term in `choose_group_size`'s sort key, ranked **after** `strands`
and provably inert unless every candidate is already outside the band (measured: 168 books,
148 byte-identical, all 20 that changed placed one photo MORE). 474/474 again. **So the
symptom is treated and the root cause is not.** Anyone attacking the blank pages is
attacking the same floor.

### 13. Nothing in the test suite checks a slot against a text zone — **FIXED 2026-09-16**

**Closed by `2bd4e66`**: `tests/templates.test.ts` now fails on any slot/text-zone overlap
(mutation-checked by moving `20`'s first text zone onto a slot). The gutter-epsilon note at the
end of this item is still open.

Added 2026-08-15. There is **no overlap check between a template's photo slots and its
`text_zones` anywhere in the suite** — not in `tests/templates.test.ts`, not in Rust.

This is not hypothetical. Both template edits in `feat/phase-2-completion` (`20` and `45`)
produced a text zone that would have landed on a moved slot, and **both were caught by
hand** — once by the implementer, once by the reviewer computing the clearances pairwise.
Two for two on the only two occasions the geometry has moved. The validator already walks
every template and already knows both rectangles; this is a cheap guard that does not exist.

Related, from the same review and deliberately left alone: template `20`'s four panes sit
exactly on `GUTTER_X0`/`GUTTER_X1` with zero margin, passing only via `clear_of_gutter`'s
epsilon. A higher-precision re-derivation of the gutter constants would flip them.

### 14. Photos are silently lost from ~41 keepers upward, and the UI reports zero dropped — **FIXED 2026-09-16**

**Closed by `23d757f`**, with exactly the per-side single set this item prescribed: `single_left`
and `single_right`, `Capacity` summing slot by slot, `max_photos` reading the true 63. The
40..=64 table below is the pre-fix state; after the fix `tests/pack_sweep.rs` asserts that no
photo is lost below capacity over 10..=65 keepers and four chapter shapes.

Added 2026-08-15. **Read this before trusting any "every photo is placed" statement in this
file.** It is a sibling of item 12, not the same defect: item 12 produces BLANK PAGES in
sparse books; this one produces LOST PHOTOS in crowded books, with **zero blank pages
throughout**. Every slot is filled; the photos are stranded *inside* chapters.

**Measured** — real `templates/` library, 20 pages, `pace::assemble`, seed 7, one synthetic
photo per keeper (no utility photos, unique near-dup cluster, so keepers == input):

| keepers | placed | lost | blank pages |   | keepers | placed | lost | blank pages |
|---:|---:|---:|---:|---|---:|---:|---:|---:|
| 40 | 40 | 0 | 0 | | 53 | 52 | **1** | 0 |
| 41 | 40 | **1** | 0 | | 54 | 52 | **2** | 0 |
| 42 | 40 | **2** | 0 | | 55 | 52 | **3** | 0 |
| 43 | 43 | 0 | 0 | | 56 | 52 | **4** | 0 |
| 44 | 44 | 0 | 0 | | 57 | 52 | **5** | 0 |
| 45 | 45 | 0 | 0 | | 58 | 56 | **2** | 0 |
| 46 | 46 | 0 | 0 | | 59 | 57 | **2** | 0 |
| 47 | 46 | **1** | 0 | | 60 | 57 | **3** | 0 |
| 48 | 48 | 0 | 0 | | 61 | 57 | **4** | 0 |
| 49 | 49 | 0 | 0 | | 62 | 57 | **5** | 0 |
| 50 | 50 | 0 | 0 | | 63 | 58 | **5** | 0 |
| 51 | 50 | **1** | 0 | | 64 | 58 | **6** | 0 |
| 52 | 51 | **1** | 0 | |    |    |   |   |

**Nothing in the 2026-08-15 fix wave changed a single figure in that table.** It is the
state of `feat/phase-2-completion` as it stands. Do not read it as a post-fix improvement;
there was no improvement, and the reason is below.

**Two mechanisms, and they compound.**

1. **`Buildable::from_library`'s `single` set is side-blind.** It reads
   `Library::page_half_pool()`, which pools BOTH sides, while `pace::best_single` filters by
   `l.side == side` and `pace::assemble` places the opening single on the RIGHT and the
   closing single on the LEFT. On the real library LEFT halves offer `{1,2,3,4}` and RIGHT
   `{1,2,3,4,5}`, so the union's `5` is a size the CLOSING slot cannot build: `pack` cuts a
   `(5, Single)` closing group, `best_single` falls back to a 4-slot left half, and
   `pace::strongest` silently trims one photo. Knock-on:
   `Capacity::from_buildable` reads `single_hi = 5`, so
   `Capacity::from_library(20, lib).max_photos` reports **64** when the true ceiling is
   **63** (9 × 6 spread + a right-hand 5 opening + a left-hand 4 closing). `commands.rs`'
   `dropped_photos` is `keeper_count.saturating_sub(max_photos)` — so **at 64 keepers the UI
   reports 0 dropped for a book that drops 6.**
2. **The item-12 floor, in the other direction.** In the rows above where `pack`'s group
   sum is already below the keeper count (52–57, 60–62), the loss is chapters arriving at a
   slot they cannot fill. Same slot-kind-blind `union_bounds()` floor as item 12.

**A fix was implemented, measured, and REVERTED. Do not re-attempt it.** The candidate was
to derive `single` from the INTERSECTION of the two sides (`{1,2,3,4}`) rather than the
union, on the theory that it is strictly conservative. **It is not**, because the union is
not uniformly an overclaim — `5` is unbuildable for the *closing* slot but genuinely
buildable for the *opening* one. Measured on the real library:

* page 1 really does lay out five photos on `25-six-up-mosaic-hero-five:right`. Under the
  intersection it drops to four — a real photo lost that the library could print.
* `max_photos` falls to **62**, an UNDERclaim against the true 63, so `pack`'s capacity trim
  pre-emptively discards keepers at 63 and 64 that previously reached the page.
* over the 40..=64 table above: 24 counts unchanged, **1 worse** (63: 5 lost → 6), 0 better.
* over 4 synthetic chapter shapes × 3 seeds at 60..=64 keepers: **3 of the 4 shapes lose 1–2
  photos per book**, none gains one.

Its one real merit was accounting integrity — under the intersection, `pack`'s group sum
equals what reaches the page, so no loss is hidden in `pace::strongest`. That is not worth a
net loss of photos.

**The correct fix is a per-SIDE single set:** `Buildable::single_left` and `single_right`,
each single slot measured against the side it actually prints on, with `Capacity` summing
`right_hi + left_hi` instead of `2 × single_hi`. That removes the unbuildable closing `5`,
keeps the buildable opening `5`, and makes `max_photos` read the true **63** — all three,
which no single-set variant can do. It is a change to `pack`'s contract and wants its own
sweep. The warning block on `Buildable::from_library` in `src-tauri/src/book/pack.rs` carries
the same conclusion at the code, so the next reader cannot re-run the rejected experiment by
accident.

---

## Layout quality: the first real-photo run, 2026-08-14

The user generated a book from their own photographs and looked at it in the preview.
Verdict: **"not bad, functional"** — the pipeline works end to end. Four quality
complaints, in their words, each traced to a cause here. **These are the first evidence
this project has about whether the engine produces a good book, as opposed to a valid
one.** Nothing below is a bug; every one is a design gap.

**Updated 2026-08-15.** `feat/phase-2-completion` addressed all four. Complaint 1 is
genuinely improved and measured. Complaints 2, 3 and 4 got machinery that **ships at weight
`0.0` and changes nothing yet** — read "The four new scoring terms are DORMANT" at the end
of this section before reading any of them as an improvement.

### 1. "Some pages are left blank not sure why"

Two distinct causes, distinguishable on screen because the preview shows template ids.

- **STRUCTURAL: a template with a page half holding zero photo slots.** Ten of the 36
  templates were in that state, and those pages printed white by construction under a real
  template id such as `03-hero-left-text-right`. **Gone.** Eight remain, all of them 1-photo
  templates, and size 1 is now banned from the spread region — so those eight are only ever
  drawn as page halves, where they fill the page.
- **UNDER-PRODUCTION: `assemble` emits a page with the id `blank`** when a slot gets no
  group — either because the packer produced fewer groups than there are slots, or because
  every candidate template was rejected by a hard constraint (face clipped, face in the
  gutter or safe margin, below 200 DPI). The ruling behind it is that a short book cannot be
  uploaded against a fixed-page SKU. **This is now the only cause of a white page, and the
  first of the two reasons is the one that fires** — see open item 12.

**Blank printed pages: what the completion branch actually did (measured 2026-08-15).**
Real-library sweep, 6 photo counts × 3 seeds = **18 books × 20 pages = 360 printed pages**,
comparing the pre-branch commit `6a40c63` against the branch:

| | pre-branch `6a40c63` | after the branch |
|---|---|---|
| pages naming a REAL template and holding zero placements | **78** | **0** |
| pages named `blank` | 6 | **45** |
| **total white pages** | **84** | **45** |
| photos placed / keepers | 474 / 474 | **474 / 474** (over THIS sweep's range only — see below) |

**The `474 / 474` row is range-limited, and reading it as a book-wide invariant is wrong.**
Those six photo counts (12, 20, 25, 30, 40, 60 input photos) cull to **10, 17, 21, 25, 34
and 51 keepers** — 158 per seed, 474 across the three. Over that range every keeper is placed, and that is all the row licenses. Swept
independently at 40..=64 keepers the same engine loses up to **6 of 64**, with zero blank
pages the whole way — see **open item 14**, which carries the per-count table. A 20-page
book with 42–64 keepers is an ordinary case, and the real-photo run that motivated this
branch was in it.

**Read that precisely.** The STRUCTURAL blank page is eliminated entirely, 78 → 0: size 1 is
banned from the spread region and the two multi-photo templates with an empty half were
re-authored. It was **traded** for 45 new `blank`-template pages, which are the documented
and accepted consequence of the ban: a 20-page book's minimum rises from 11 × 1 = 11 photos
to 2 × 1 + 9 × 2 = 20, so books with 11–19 keepers now blank the slots they cannot fill.
Before the branch those same books were filled with 1-photo spread templates — each of which
printed a blank half anyway, which is why the net still falls. **This is not "blank pages
eliminated."** It is 84 → 45 white pages out of 360, 23% → 12.5%, with every keeper still
placed **at the 10–51 keeper counts this sweep covers** (photos ARE lost from ~41 keepers
upward — open item 14) and one known remaining cause of white pages.

That cause is **open item 12**: `apportion_slots` hands a chapter more slots than it can
fill, because it sizes against a floor of 1 taken from the single-page set. Fixing it would
plausibly take the 45 back toward the pre-branch 6. It is the highest-value follow-up
available and it is deliberately not done — the reasoning, and the mechanism, are in item 12.

### 2. "Some facial features are too candid/unprepared/unflattering"

**The engine has no expression signal whatsoever.** Vision has no expression classifier
at any macOS version; the geometric smile proxy was measured against real faces and has
a proven 100% false-negative rate, so it is computed and deliberately unused.

**Built 2026-08-15: the `face_quality` scoring term, at weight `0.0`.** Vision's face
capture quality — blur, exposure and pose for the best face in the slot — was previously
only a tie-break inside `cull`. It is now a per-slot soft term in `score.rs`, unit-tested,
and inert until its weight is raised.

**It is still not an expression signal, and raising its weight will not make it one.**
Vision has no expression classifier at any macOS version. Capture quality is a proxy for
*sharp, well-exposed and front-facing*; it does not know a smile from a grimace, and a
perfectly-captured unflattering face scores well on it. What it should reliably improve is
the subset of complaint 2 that is really "this face is soft or badly lit". The rest of the
complaint has no signal behind it and this term does not change that.

Eyes-closed/mid-blink detection is derivable from the landmark geometry already carried,
but it is the same shape as the smile proxy and must be measured against real faces
before being trusted — see the smile calibration section for how that went last time.

### 3. "There are too many images that are similar (too little variety) in some pages"

Near-duplicate clustering is perceptual-hash based, so it only collapses near-identical
frames. Photos of the same moment from slightly different angles are not near-duplicates
and all survive. **There is no within-spread diversity term at all.**

Worse: `palette_harmony` **rewards** hue agreement between the photos on a spread, so the
one aesthetic term that exists actively pushes toward the sameness being complained
about. It is already recorded as effectively inert (open item 2), which makes
repurposing it into a diversity term cheap rather than disruptive.

Signals available today with no new analysis: `phash` distance, palette distance, scene
tags, and capture-time proximity.

**Built 2026-08-15: the `spread_diversity` term, at weight `0.0`.** A spread-global term
combining three components, **equal-weighted**: scene-tag Jaccard distance, palette
distance, and capture-time gap (saturating at `SATURATE_SECONDS = 3600.0`). `Photo` gained
`scene_tags` and `captured_at` to carry the first and third. Each component has its own
isolation test varying exactly one axis with the other two held byte-identical, and the full
3 × 3 mutation matrix was run: stubbing any one component fails only its own test. So all
three are individually load-bearing — which matters, because the term's whole claim is
"three signals, equal weight".

**Equal weighting is a starting point, not a claim.** Nobody has measured whether scene
tags, palette and time deserve the same share, or whether one of them dominates on real
photographs. That is a tuning question, and the tuning session below is where it gets
answered. Note also that `palette_harmony` pulls the opposite way — see open item 2.

### 4. "Some interesting features should be highlighted rather than mixed with other images"

`hero_match` places the highest-aesthetic photo in a `hero` slot, but **only within a
group that has already been formed**. `pack` sizes groups from chapter boundaries and
capacity alone — it knows nothing about photo merit — so a standout image can be dealt
into a six-up.

Two fixes, different sizes: bias `best_spread` toward templates with a dominant hero slot
when a group contains a high-percentile photo (cheap, no packing change); or let merit
influence group size in `pack`, so an exceptional photo gets a small group or a page to
itself (the real fix, and a genuine change to the packer's contract).

**Built 2026-08-15: the first of those two, as `hero_prominence`, at weight `0.0`.** It
rewards a template whose hero slot dominates the spread when the group holds a standout
photo, scaling with how far above the group the standout sits. The `standout` factor gates
the function twice — an early return and a multiplier — which is documented as deliberate
redundancy so nobody deletes one half as dead code; the early return is a compute
short-circuit, not an independent safety net.

**The packer half is deliberately deferred.** `pack` still knows nothing about photo merit,
so a standout image can still be dealt into a six-up; only the template *choice* for the
group it lands in is biased. Letting merit influence group size is a change to `pack`'s
contract, and `pack`'s contract was already changed twice in this branch with each change
surfacing a defect that only a sweep caught. Do it after measurement, not before.

One gap to know before raising this weight: **the graduated scaling is untested.** Both
tests compare a fully-flat group against a fully-standout one; no test exercises a mid-range
standout's proportional effect, which is the term's whole selling point.

### The four new scoring terms are DORMANT — read this before claiming the book got better

`face_quality`, `spread_diversity`, `gutter_saliency` and `hero_prominence` all exist, are
unit-tested, are wired into `score_spread`, and **ship at weight `0.0` in
`templates/weights.json`.** At 0.0 they cannot change which template wins any spread. That
is why the golden fixture did not move when any of them landed, and it is the reason they
could be shipped without a real-photo run.

**Until they are tuned against real photographs, the honest description of this work is "the
machinery exists and is unproven" — not "the book is better."** No book has been generated
with any of these weights above zero and looked at by a human. Do not write, or repeat, the
stronger claim.

`weights.json` is **hot-reloadable beside the templates**, so raising a weight needs no
rebuild — edit the file and regenerate. That is what makes a tuning session cheap: it is a
sitting with real photographs and a text editor, not a development task.

**The tuning session fills this table in.** Fill the last column with what you settle on,
and record what you were looking at when you decided:

| Term | Complaint it addresses | Ships at | Settled weight |
|---|---|---|---|
| `face_quality` | 2 — candid/unflattering faces. **Sharp/well-exposed/front-facing only; there is still NO expression signal.** | `0.0` | |
| `spread_diversity` | 3 — too many similar photos on a spread. Scene tags + palette + capture-time gap, equal-weighted. | `0.0` | |
| `gutter_saliency` | Closes open item 1 — the graded penalty §4.3 promised. Not one of the four complaints. | `0.0` | |
| `hero_prominence` | 4 — standout photos buried in a group. **Scorer half only; the packer half is deferred.** | `0.0` | |

Two things to try first, both free: **trade `palette_harmony` (currently `0.2`) down against
`spread_diversity` up** — they pull in opposite directions on complaint 3, and open item 2
records why `palette_harmony` is close to inert anyway — and raise `face_quality` alone
before touching anything else, since it is the term most likely to show a visible difference
per unit of weight.

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

That is thirteen more in one phase. **`feat/phase-2-completion` caught six more (2026-08-15),
every one of them found by mutation discipline rather than by review reading:**

- A `deny_unknown_fields` test whose typo (`pallete_harmony`) was simultaneously an unknown
  key **and** an omission of the required `palette_harmony`, so the file was rejected on the
  missing field whether or not the attribute existed. **This one was written into the plan by
  the controller whose job was to police for exactly this shape.** Nobody is immune.
- The new real-library "no page names a real template and holds nothing" assertion stayed
  **green** when template `20` was reverted to its broken form, because the packer never
  selects `20` at the swept photo counts. A structural whole-library test was added to
  catch it; the assemble-level one cannot.
- The un-ignored density test passed for the wrong reason: at 25 keepers in odd chapters a
  size-3 group is arithmetically unavoidable, so deleting `pack`'s density swing left it
  green. Rebuilt on 22 keepers in chapters of 8/8/6 — all even, so an all-2s book is exactly
  buildable — and the mutation now turns it red.
- `spread_diversity`'s separation test over-determined: its fixture maximised all three
  component distances at once, so it would have stayed green with any one component dead.
  Split into three isolation tests, each varying one axis with the other two held identical.
- `hero_prominence`'s obvious assertion (`varied > flat`) still passed with the term stubbed
  out of the total, because `hero_match` alone separates those fixtures. Only a delta
  assertion does the work.
- The prescribed `hero_prominence` mutation was inert, because `standout` gates the function
  twice. The intended mutation had to be derived rather than taken from the brief.

It is the single most useful thing this document carries forward, so treat it as a standing
rule rather than a historical note:

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

**The one guard that is missing outright:** nothing anywhere checks a template's photo slots
against its text zones. Two template edits, two zones that would have landed on a moved
slot, both caught by hand. See open item 13.

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
| **3** | Spread preview UI + spread-level controls (regenerate, swap, lock, reject) | **Built 2026-09-16** — see "Phase 3: spread-level controls" below. |
| **4** | Canvas editor: drag/resize/crop with snapping to the margin guides | **First step built 2026-09-16:** hand cropping. Drag a photo inside its slot to move the window, scroll to zoom; `BookEdit::SetCrop { placement, x, y, w }` re-derives the height from the slot's aspect (so a hand crop cannot distort), keeps the window inside the photo, and re-runs `score::rejects`. Live preview is local (`cropMoved`/`cropZoomed`, unit-tested), one edit is sent on release. A regenerate or swap of that placement recomputes the crop, documented in the README. **Second step, same day:** moving and resizing the slot itself. `BookEdit::SetSlot { placement, rect }` keeps the rect on the page (reaching the canvas edge prints as bleed; passing it is refused), at least 5% each way, and non-overlapping with the page's other slots; the photo is re-cropped with `choose_crop` for the new shape and re-checked with `rejects`, with bleed edges read off the rect. `placement_slot` now resolves every constraint check from the PLACEMENT's rect (role from the template when it still exists, `Support` otherwise), so a moved slot governs later swaps and crops. In the UI a "Move and resize boxes" switch turns drag into move and adds corner handles; edges snap (`slotMoved`/`slotResized`/`pageGuides`, unit-tested) to the canvas edge, trim, safe and gutter lines and the other slots. Verified in the harness: move, resize, and a refused overlapping resize that snaps back with the reason. **Not built:** adding or removing slots, text zones, and a per-edit `preflight_core` run (a slot stopping just short of the trim line is legal and only export's pre-flight comments on it). |
| **5** | AI SDK v7 + DeepSeek chat agent driving the layout tools | Phase 4, an API key in the keychain (see "Build, run, test"), and the `buildAgentPayload()` privacy chokepoint the design specifies. `book::edit::BookEdit` is the tool surface the agent would drive. |

### Phase 3: spread-level controls (built 2026-09-16)

`src-tauri/src/book/edit.rs` is the engine half, `edit_book` the one command, `BookPreview.vue`
the UI. The unit of editing is the **opening** — page 1 alone, each pair of facing pages, the
last page alone — numbered the way `toSpreads` draws them (`0 ..= S+1`), because a spread's two
pages come from one template and cannot change separately.

| Edit | What it does | Refused when |
|---|---|---|
| `regenerate` | Lays the same photos out on the best template the opening has **not shown**. `best_spread`'s seed only breaks exact ties, so a fresh seed alone returns the same layout; excluding the current one is what makes the button browse. | locked; no alternative left (`NoAlternative`) |
| `rejectTemplate` | Records the current template on the opening (`OpeningControls.rejected`), then regenerates. Checked first, so an opening with nowhere else to go keeps its template. | locked; no alternative |
| `setTemplate` | Exactly the named template (a spread id, or `"<id>:left"`/`"<id>:right"` for a single page), and forgives a rejection of it. | locked; unknown; wrong photo count; every assignment breaks a hard constraint |
| `setLocked` | Locked openings refuse every other edit and are skipped by shuffle. | never |
| `shuffle` | `regenerate` over every unlocked opening; openings with no alternative are left alone. | never |
| `swapPhotos` | Exchanges two placements anywhere in the book, re-crops both, and re-runs `score::rejects` on both **before** writing either. | same slot; missing slot; either opening locked; a face clipped / in the gutter / in the margin, or below `MIN_DPI` at the new slot |

`Book` gained `controls: BTreeMap<usize, OpeningControls>` (`locked`, `rejected`, `rerolls`),
omitted from the JSON when empty, so the golden and every saved project are unchanged.
`rerolls` feeds the per-opening seed (`opening_seed`) and is persisted so reopening a project and
clicking again continues the sequence. `BookLayout` gained `openings` (locked, rejected,
alternatives) so the buttons are disabled, not failing, when there is nothing to do.

**Every write returns the whole `BookLayout`, never `{ok: true}`** — the design's rule 1, applied
before the agent exists. `useBook.editBook` replaces the layout with what came back and never
patches locally; a refused edit surfaces its reason and the screen still shows exactly the saved
book. `tests/fixtures/wire/book-edits.json` pins the tagged shape from both sides.

Not built here, on purpose: any change to `slot_rect` or `crop` by hand (that is Phase 4), and
"regenerate the whole book with a new seed" — shuffle re-lays templates over fixed groups
instead, because a fresh `assemble` would re-pack the groups and discard every lock.

### Multiple source folders (built 2026-09-16)

`analyze_folders(folders: Vec<String>)` replaces `analyze_folder`; the picker takes several
folders. Decisions, each deliberate:

- **The union is one population.** Percentiles rank every photo against all the others, and
  time-gap event clustering does not care which folder a photo came from. A book is what it
  draws from; adding a folder re-ranks everything on screen, and that is correct.
- **Sort order is still by path**, which interleaves folders; the contact sheet groups by event,
  so the interleave is only visible within an event.
- **Duplicates across folders** were already handled by the hash-keyed cache; the walk lists a
  path once even when one root sits inside another.
- **Projects remember every folder** (`project_folders`, ordered). `projects.source_folder` keeps
  the first for the list label and for rows saved before the table existed; `load_project` falls
  back to it when a project has no folder rows. "Edit the selection" re-analyses exactly the
  saved list. `sourceFolders` is on both project wire types and in both fixtures.
- **The `AppState.photos` collision is fixed by identity, not capacity.** Every analysis mints a
  `run_id` (never 0); `AnalysisSummary` carries it; `apply_photo_overrides` and `recommend_book`
  hand it back; a mismatch is refused (`select_run`, pure and tested) rather than answered from
  whatever set is cached. A run that finishes after a newer one started does not overwrite the
  newer set.

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
ratio. The canvas is 2.518:1, so the two are never interchangeable. Verified numerically:
102 of 103 slots have their real-world ratio inside their declared range; zero have their
normalised ratio inside it.

**`aspect_pref` is validator-only — the scorer never reads it.** It is parsed into
`templates::Slot` and stored, and nothing else. `score::aspect_fit` derives the slot's
target aspect from the RECT (`slot_aspect`, which applies the same inch conversion) and
compares that to the photo. So a wrong `aspect_pref` fails `tests/templates.test.ts`; it
cannot mis-score a layout. The unit-confusion trap is real and still worth guarding — it
lives in `slot_aspect`, and `geometry::Rect::aspect_in` plus
`score_slot_aspect_uses_page_inches_not_the_normalised_ratio` are what pin it — but treat
`aspect_pref` itself as a declared-intent cross-check on the rect, not as an input to
template selection.

Every template is **exact-count** (`min_photos == max_photos == slots.length`), so the
packer selects templates by photo count rather than fitting a range.

**Coverage as of end of Phase 2 — the gaps are closed.** The library is now **36 spread
templates** plus `weights.json` and a `README.md`:

| Photos per template | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| Templates available | 8 | 6 | 6 | 7 | **4** | 5 |

Two commits got here: `31afe4d` removed 21 templates whose slots straddled the fold (they
could not decompose into page halves), and `77c5005` authored page-subdivision layouts
closing the 4/5/6-up gaps. `templates_real_library_covers_every_group_size_from_one_to_six`
asserts this against the real directory.

**That test, and the two beside it, are no longer `#[ignore]`d (2026-08-15).** They had been
ignored "so the unit tests stay hermetic" — but `bun run test:rust` passes no `--ignored`
and there is no CI, so the only guards on the authored library, including the 1..=6 coverage
the whole packer rests on, ran nowhere at all. Measured before un-ignoring: all three
together finish in under 10 ms. There is now **nothing `#[ignore]`d in the crate.**

**Two templates were re-authored on 2026-08-15, and both changed identity or shape:**

- `45-three-up-mosaic-margin-left` is now **`45-three-up-mosaic-hero-right`** — file name
  and `id` renamed together. Grep for the old name if a doc or fixture still cites it.
- `20-four-up-windowpane` was re-authored as a true 2 × 2 grid **spanning both pages**.

Both previously put every slot on one page half, so both printed a white page every time
they were chosen. The per-slot-count coverage table above is unaffected — 45 is still a
3-up, 20 still a 4-up — but the count of templates with an empty page half fell from ten to
**eight, and all eight now hold exactly one photo**, which is why banning size 1 from the
spread region removes the structural blank page entirely.

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
  `AnalyzedPhoto.kept`. See "One culling authority" above, and
  "User-controlled selection" for the include/exclude states layered on top of it.
- ~~`.oxlintrc.json` leaves `no-explicit-any` off.~~ **Enabled 2026-09-16** as an error.
- No `typecheck` script; `nuxi typecheck` and `vue-tsc` both fail on environment issues.
- Nothing pins the build to `aarch64-apple-darwin`. No universal config exists, so the
  constraint is not violated, but it is not enforced either.
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
- `ResponseResult::Benchmarked` and `RequestKind::Benchmark` (Rust) have no Rust caller —
  `scripts/benchmark.sh` drives the sidecar binary directly. They stay as the pin on the
  Swift wire the script relies on; the unused `Sidecar::benchmark` method itself is gone.

---

## Immediate next steps

0. **Run the app on a real folder and use the new controls.** Everything on 2026-09-16 was
   verified by tests, the sweep and the browser harness with stubbed data — not in the packaged
   app against real photographs. In particular: pick two folders at once, regenerate and swap
   on a real spread, reopen the project and check the lock survived.
1. **Generate a real book from your own photos and upload one page to Pixajoy.** This is
   the outstanding verification described above, and it is by far the highest-value thing
   remaining — it is the only thing that would turn "Phase 2 is internally verified" into
   "Phase 2 works". It settles, in one sitting: whether the picker accepts JPEG and PNG,
   RAW and HEIC throughput, the whole face code path, Pixajoy's page-count semantics, and
   whether the new even-distribution pacing (open item 4) reads well on real photographs --
   it is measured against fixtures only.
2. **Answer the Pixajoy page-count question** — 30 seconds in their editor, while you are
   there.
3. **Tune the four dormant scoring terms** against those same real photographs, and fill in
   the settled-weight table in "The four new scoring terms are DORMANT". Nothing in
   `feat/phase-2-completion` improves a book until this happens; it is a `weights.json` edit
   and a regenerate, no rebuild. Cheapest first move: `palette_harmony` down against
   `spread_diversity` up.
4. ~~Decide whether 45 white pages of 360 is acceptable (open item 12).~~ **Done 2026-09-16**:
   every remaining blank page in the sweep is in a book with fewer photos than the slots'
   minimum fill. What is worth judging on real photographs is the **chapter folding**: a book
   whose chapters cannot fill the slots now folds adjacent chapters together rather than
   printing a white page or dropping a photo, which puts a chapter boundary mid-spread. If
   that reads badly, the knob is `merge_chapters_the_slots_cannot_seat`'s second rule.
5. **Phase 4.** Start with a crop-drag inside a slot; the table under "What is NOT built" says
   what it can lean on.

Items 1 and 3 of the previous list are closed: the seed reaches `best_spread`, and the
gutter-saliency penalty exists. Both have successor caveats — see open items 3 and 1.
