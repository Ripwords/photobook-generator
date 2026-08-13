# Phase 2 Export Revision — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace the abandoned transparent-PNG export with one that ships the user's own photographs, correct the engine to Pixajoy's published print limits, and persist a book as a reopenable project.

**Architecture:** The layout engine (cull → pack → crop → score → pace → assemble) is complete and unchanged. This revision touches only the numbers it enforces, what leaves the app, and where a book is stored.

**Tech Stack:** Rust (Tauri 2), Swift 6 + Core Graphics (sidecar), Nuxt 4 + Vue 3, SQLite via rusqlite, vitest, swift-testing, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md` (revised 2026-08-14, commit f0ecc20)

**Supersedes** in `docs/superpowers/plans/2026-08-13-phase-2-layout-engine.md`:
- **Task 0** (Pixajoy alpha test) — dead. The mechanic it gated no longer exists.
- **Task 9** (transparent-PNG Swift renderer) — replaced by Task 3 here.
- **Task 10** (render client) — replaced by Task 4 here.
- **Task 12** (generate UI) — replaced by Task 5 here.
- **Task 14** (end-to-end) — replaced by Task 6 here.

Tasks 1–8, 11 and 13 of that plan are complete, reviewed, and stand unchanged except where Task 1 below corrects their constants.

## Global Constraints

- **Images never leave the machine.** Only derived JSON may be sent anywhere.
- **arm64 only, macOS 15+.** Never configure a universal build.
- **`bun run sidecar` before any `cargo` command.** `tauri-build` validates the `externalBin` path at compile time.
- **All three suites must be green before every commit**, and you must run all three: `cargo test --manifest-path src-tauri/Cargo.toml`, `bun run test`, `bun run lint`. An earlier task shipped a red TypeScript suite for five tasks because only cargo was run.
- Bun, not npm. Conventional Commits. Never `git commit --no-verify`. Never `any` in TypeScript.
- **Print geometry, from Pixajoy's published guidance:** 200 DPI minimum, 300 recommended, 0.125" safe margin between important content and the edge, no face or notable feature in the gutter.
- Page canvas 11.197" × 8.894" = 3359 × 2668 px @ 300 DPI. Spread canvas 22.394" × 8.894". Bleed 0.197" on the three outer edges, none at the fold.
- **Extract pure functions.** The codebase does this repeatedly so core logic is testable without an `AppHandle` or a live sidecar.

### How tests are specified in this plan, and why

The plan this revises shipped **nine decorative fixtures** — tests that passed under a broken implementation. Examples: a property test that restated its own implementation; a tie-break test whose inputs never reached a tie-break; an aspect test that passed with its own term zeroed because a different term carried the ordering; two DPI boundary tests that sat fractionally off the boundary so the comparison could be flipped with the suite staying green.

So this plan **does not hand you fixture values.** For each behaviour it names:

- the **property** that must hold,
- the **mutation** that must make a test fail.

You construct inputs that actually exercise it, and you prove each test has teeth by applying the named mutation and pasting the failure. A test that survives its mutation is not finished, regardless of what it asserts.

Where exact constants matter they are given verbatim and must not be substituted.

---

## Task 1: Correct the engine to Pixajoy's published print limits

**Files:**
- Modify: `src-tauri/src/geometry.rs`, `src-tauri/src/book/score.rs`, `src-tauri/src/book/preflight.rs`
- Modify: `docs/PROJECT-STATUS.md`

**Interfaces:**
- Produces: `geometry::SAFE_MARGIN_IN = 0.125`, `geometry::SAFE_U`, `geometry::SAFE_V`, `geometry::in_safe_margin(rect, side) -> bool`
- Changes: `score::MIN_DPI` from `150.0` to `200.0`; `score::Rejection` gains `FaceInSafeMargin`; `preflight` gains the matching Block row.

**Why:** the engine currently accepts photos Pixajoy considers unprintable, and treats a face sitting just inside the trim line as safe when their guidance says to keep 1/8" clear of the edge.

- [ ] **Step 1: Add the safe-margin geometry, test-first**

`SAFE_MARGIN_IN = 0.125` inches, converted to page-normalised units on each axis exactly as `TRIM_U`/`TRIM_V` are. `in_safe_margin(rect, side)` is `in_trim` inset by a further `SAFE_*` on all four edges **except the fold edge**, where the gutter strip already governs.

Properties to pin:
- A rect inside the safe margin is necessarily inside trim; the converse must not hold. Construct a rect that proves the converse fails.
- The inset applies on the outer vertical edge only, matching `in_trim`'s asymmetry between left and right pages.

Mutation that must fail a test: make `in_safe_margin` delegate to `in_trim`.

- [ ] **Step 2: Run it and confirm it fails, then implement, then confirm it passes**

```bash
bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml geometry
```

- [ ] **Step 3: Raise the DPI floor**

Change `score::MIN_DPI` to `200.0`. Its doc comment must cite Pixajoy's published minimum rather than presenting the number as a judgement call.

The existing test `score_dpi_floor_admits_exactly_min_dpi_and_rejects_one_pixel_below` uses a 6.0"-wide slot where exact DPI is constructible (`6.0 / PAGE_W_IN * PAGE_W_IN == 6.0` in f64, and `N / 6.0` is an exact binary division). **Recompute its pixel values for 200 DPI** — do not assume the old ones still sit on the boundary. Confirm `effective_dpi == 200.0` exactly, then that one pixel fewer is rejected.

Mutation that must fail a test: flip `<` to `<=` at the floor comparison.

- [ ] **Step 4: Add the safe-margin hard constraint to the scorer**

Add `Rejection::FaceInSafeMargin`. A face mapped into page coordinates that violates `in_safe_margin` rejects the candidate, exactly as `FaceInGutter` does.

Order matters for diagnosis: a face outside trim entirely should report the trim violation, not the safe-margin one. Pin that with a test.

Mutation that must fail a test: delete the safe-margin check from `rejects`.

- [ ] **Step 5: Add the matching pre-flight Block row, and move the warn band**

Pre-flight gains "Face inside the 0.125" safe margin → Block". Its DPI warn band becomes **200–300**, not 150–300. Pre-flight must import `score::MIN_DPI` rather than restating 200, so the two can never drift.

Mutations that must each fail a test: delete the safe-margin block row; change the warn ceiling; make pre-flight define its own floor constant instead of importing the scorer's.

- [ ] **Step 6: Check what the new limits do to the real library**

Run the `#[ignore]`d real-library test and the golden. **The golden will likely change** — a 200 DPI floor rejects candidates 150 accepted. If it does, regenerate it with `UPDATE_GOLDEN=1`, then **read the new JSON and confirm it is still sane** before committing: 20 pages, sides alternating, no photo placed twice, placements inside their pages. Report what changed and why.

If the fixture photos are now too low-resolution to place at all, raise their dimensions rather than lowering the floor — the floor is Pixajoy's, not ours.

- [ ] **Step 7: All three suites, then commit**

```bash
cargo test --manifest-path src-tauri/Cargo.toml && bun run test && bun run lint
git commit -m "fix(book): adopt Pixajoy's 200 DPI floor and 0.125in safe margin"
```

---

## Task 2: Project persistence

**Files:**
- Create: `src-tauri/src/project.rs`
- Modify: `src-tauri/src/db.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct Project { pub id: i64, pub name: String, pub source_folder: String, pub created_at: i64, pub updated_at: i64, pub book: Book, pub exports: Vec<ExportRecord> }`
  - `pub struct ExportRecord { pub at: i64, pub output_dir: String, pub format: String, pub file_count: usize }`
  - `Db::save_project(&self, name: &str, source_folder: &str, book: &Book) -> rusqlite::Result<i64>`
  - `Db::load_project(&self, id: i64) -> rusqlite::Result<Option<Project>>`
  - `Db::list_projects(&self) -> rusqlite::Result<Vec<ProjectSummary>>`
  - `Db::record_export(&self, project_id: i64, rec: &ExportRecord) -> rusqlite::Result<()>`
  - `Db::delete_project(&self, id: i64) -> rusqlite::Result<()>`

**Why:** a book must survive quitting, and be reopenable, regenerable and re-exportable without re-analysing the folder. Phase 3 renders a project; Phase 4 edits one. Phase 2 only has to capture the data those phases need.

- [ ] **Step 1: Migration, test-first**

Add tables to `Db::migrate`'s `execute_batch`. Follow the existing style exactly — `CREATE TABLE IF NOT EXISTS`, an index where a lookup needs one.

`projects` holds id, name, source_folder, page_count, seed, dropped, created_at, updated_at. `project_pages` and `project_placements` hold the layout, or the whole `Book` serialises to one JSON column — **decide and justify in your report.** The tradeoff: normalised tables let Phase 4 update one placement without rewriting the book; a JSON column is far less code now. Phase 4 is two phases away and unspecced, so weigh accordingly.

`project_exports` holds the export history.

Migration must be additive and must not disturb the existing `features` table. Pin that with a test that writes a feature row, migrates, and reads it back.

- [ ] **Step 2: Round-trip, and make the test able to fail**

The property: `load_project(save_project(book))` reproduces a `Book` equal to the original — every page, every placement, the seed, the dropped count.

A round-trip test is easy to write and easy to write vacuously. Make it fail under each of these mutations, and paste each:
- drop the seed on save
- drop one placement's `z`
- round a `crop` rect to fewer decimal places
- swap `slot_rect` and `crop` on load

If any mutation leaves the test green, the test is comparing something looser than equality.

Use a `Book` fixture with **more than one page, more than one placement per page, and distinct non-round float values** in the rects — a book of one page with one placement at nice round numbers cannot detect most of these.

- [ ] **Step 3: Listing, export history, deletion**

`list_projects` returns summaries newest-updated first. `record_export` appends. `delete_project` removes the project and its pages, placements and export rows — pin with a test that counts rows in every table before and after.

Mutation that must fail a test: make deletion leave placements behind.

- [ ] **Step 4: All three suites, then commit**

```bash
git commit -m "feat(project): persist a book as a reopenable project"
```

---

## Task 3: Swift crop exporter

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/Exporter.swift`, `sidecar/Tests/PhotobookEngineTests/ExporterTests.swift`
- Modify: `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/Handler.swift`

**Interfaces:**
- Produces (Swift): `struct ExportItem: Codable { let sourcePath, filename: String; let cropX, cropY, cropW, cropH: Double }`, `struct ExportRequest: Codable { let outputDir: String; let items: [ExportItem] }`, `enum ExportRecord: Codable { case ok(path: String, width: Int, height: Int, bytes: Int); case failed(filename: String, message: String) }`, `RequestKind.export`, `ResponseResult.exported([ExportRecord])`

**Why:** this writes the files the user actually uploads. It crops the source photo and re-encodes it — it does not composite, synthesise, or place anything on a page canvas.

**Swift test names share one module namespace — prefix every `@Test func` with `exporter` or it is a compile error, not a test failure.**

- [ ] **Step 1: Write the failing tests**

Behaviours to pin, each with the mutation that must break it:

| Behaviour | Mutation that must fail a test |
|---|---|
| The crop window is applied in source pixels, top-left origin | Offset the crop by its own y (an origin-flip bug) |
| A lossy source (JPEG/HEIC) exports as JPEG at quality 95 | Force PNG for every source |
| A lossless source (PNG/TIFF/RAW) exports as PNG | Force JPEG for every source |
| Output carries an embedded sRGB ICC profile | Drop the profile |
| Output dimensions equal the crop window in source pixels | Rescale to a fixed size |
| A missing or undecodable source yields a `failed` record, never a crash | Force-unwrap the decode |
| EXIF orientation is applied before cropping | Decode without the orientation transform |

That last one is a real trap in this codebase: orientations 5–8 swap width and height, and cropping before orienting silently crops the wrong region of every portrait photo. `ImageLoader` already reconciles EXIF against HEIC's container `irot`/`imir` via `kCGImageSourceCreateThumbnailWithTransform` — follow it rather than inventing a second path.

**Fixtures must never be square.** A square source makes `height/width == 1` and hides every aspect and orientation bug — a documented real failure in this project. Build a fixture with a visually asymmetric pattern (e.g. distinct colours in each quadrant) so a wrong crop region is detectable by sampling a pixel, not merely by dimensions.

- [ ] **Step 2: Run, confirm failure, implement, confirm pass**

```bash
bun run test:swift
```

Decode at full resolution with orientation applied, crop with `CGImage.cropping(to:)` on an `.integral` rect, encode via `CGImageDestination` with `kCGImageDestinationLossyCompressionQuality = 0.95` for JPEG. Wrap the decode in an `autoreleasepool` — it must enclose the decode, not just the analysis, because full decompression happens there.

- [ ] **Step 3: Wire the protocol**

Add `case export` to `RequestKind`, `case exported([ExportRecord])` to `ResponseResult` with encode/decode arms following the existing pattern exactly, and `var export: ExportRequest?` to `Request`. Dispatch in `Handler`.

- [ ] **Step 4: Run all four suites (Swift too), then commit**

```bash
git commit -m "feat(sidecar): export cropped source photos as JPEG or PNG"
```

---

## Task 4: Rust export client and manifest

**Files:**
- Create: `src-tauri/src/export.rs`, `src-tauri/src/book/manifest.rs`
- Modify: `src-tauri/src/protocol.rs`, `src-tauri/src/book/mod.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `RequestKind::Export`, `ResponseResult::Exported(Vec<serde_json::Value>)`, `Request.export: Option<ExportRequest>`, `pub struct ExportRequest`, `pub struct ExportItem`, `pub fn build_items(book: &Book, photos: &[Photo]) -> Vec<ExportItem>`, `pub struct Manifest`, `pub fn manifest(book: &Book, photos: &[Photo], project_id: i64) -> Manifest`

- [ ] **Step 1: Pin the wire format both ways, test-first**

Every field name Rust serialises must match Swift's `ExportItem`/`ExportRequest` exactly. There is no blanket `rename_all` on these structs by convention here — spell each rename out per field, matching how `thumbnailDir` is already handled, and assert in a test that no snake_case leaks onto the wire.

This is the failure mode that degrades into "every photo failed" with no cause reported anywhere, so the test asserts the presence of each camelCase key by name.

- [ ] **Step 2: Filenames sort into placement order**

`p04-z1-<hash prefix>.jpg`. Pin: sorting the emitted filenames lexicographically reproduces page-then-z order. Mutation that must fail it: drop the zero-padding on the page number.

- [ ] **Step 3: The manifest**

Records per photo: source path, content hash, crop rect, destination rect **in inches**, page number, z-order, output filename, and the exported format. Plus book length, seed, template id per spread, and the project id.

Pin that a page with zero placements still appears with an empty photo list, so page counts reconcile.

- [ ] **Step 4: All three suites, then commit**

---

## Task 5: Generate, export, and save — commands and UI

**Files:**
- Create: `app/types/book.ts`, `app/composables/useBook.ts`, `app/components/GenerateBook.vue`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `app/pages/index.vue`

**Interfaces:**
- Produces: `recommend_book`, `generate_book`, `export_book`, `list_projects`, `open_project` Tauri commands; TypeScript `BookRecommendation`, `PreflightFinding`, `ExportResult`, `ProjectSummary`.

- [ ] **Step 1: Pin the Rust↔TypeScript key names**

There is no shared schema across this boundary — `AnalysisSummary.photos` is `Vec<serde_json::Value>` and a rename on either side is a silent `undefined`. Assert every key the TypeScript reads, by name, against what Rust serialises, mirroring how `protocol.rs` pins the Swift wire format.

- [ ] **Step 2: Commands**

`generate_book` assembles and **saves a project**, returning its id — generation is not something the user can lose by quitting. `export_book` runs pre-flight first and **returns early with findings if any are Block**, writing nothing; otherwise it sends the export request through the existing `SidecarPool` inside `spawn_blocking` (`Sidecar::request` blocks and will otherwise starve the tokio worker its own stdout-drain task needs), writes `manifest.json`, and records the export against the project.

- [ ] **Step 3: UI**

Recommended length with the drop count, an override, an output-folder picker, progress, the pre-flight report with Blocks listed separately from Warns, and Reveal in Finder. A project list showing saved books with their last export.

Vue conventions here: prop shorthand when the name matches, `useTemplateRef()` over manually typed refs, destructuring defaults on `defineProps` rather than `withDefaults`. No `any`.

- [ ] **Step 4: All three suites, then commit**

---

## Task 6: End-to-end verification

**Files:**
- Create: `src-tauri/tests/export_roundtrip.rs`
- Modify: `docs/PROJECT-STATUS.md`

- [ ] **Step 1: Round-trip against the real sidecar binary**

Build a real export request, pipe it to the real `photobook-engine` binary over stdin as one NDJSON line, and assert a cropped file lands on disk with the expected dimensions and format. Every other test exercises one side of the wire against a fixture of the other; this is the only one where a key-name disagreement actually fails.

- [ ] **Step 2: Full verification**

```bash
bun run sidecar && bun run test && bun run test:rust && bun run test:swift && bun run lint
bun tauri build --bundles app
```

Paste the counts. Any regression against the current baseline (205 Rust, 470 TypeScript, 71+ Swift) must be explained before proceeding.

- [ ] **Step 3: Generate a real book**

Run the app against a real photo folder, generate, export, and upload one page's files to Pixajoy. **This is the only step that proves Phase 2 works**; everything above proves only that the parts behave as specified. Confirm the picker accepts the exported format (spec §5.2's open item).

- [ ] **Step 4: Update `PROJECT-STATUS.md`**

Record: Phase 2 complete; the revised export design and why; the 200 DPI floor and 0.125" safe margin; project persistence and what Phase 3/4 can now assume; and every parked item from the ledger — the missing gutter-saliency penalty term, `palette_harmony`'s sRGB weakness, the seed not reaching spread selection, and the packer's front-loading blank spread.

---

## Carried forward from the ledger — must be recorded, not lost

| Item | Status |
|---|---|
| Spec promises a gutter-saliency **penalty** that no `Weights` field implements | Parked. Implement or correct the spec before merge. |
| `palette_harmony` uses `atan2(b, r)` over sRGB, floor ≈0.707, ~1% influence — not the Oklab hue the spec asks for | Parked pending real-photo calibration. |
| The seed never reaches spread selection, so "regenerate this spread" would do nothing | Parked. Must be known before Phase 3 is planned. |
| `choose_group_size` front-loads, leaving an avoidable blank spread | Parked. Re-check now that the library covers 1–6. |
| `preflight_core`'s doc claims "No I/O" but contains a `Path::exists()` | Minor, deferred. |
