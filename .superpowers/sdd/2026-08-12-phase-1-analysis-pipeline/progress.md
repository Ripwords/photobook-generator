# SDD ledger — plan: docs/superpowers/plans/2026-08-12-phase-1-analysis-pipeline.md

Branch: feat/phase-1-analysis-pipeline
Base: e49fa8f (docs: add phase 1 analysis pipeline implementation plan)

## Pre-flight

- Fixed in plan: Task 3 Files block named `sidecar.rs` as the test location while
  Step 1 appends the tests to `protocol.rs`. Corrected to `protocol.rs`.

## Rulings (human partner, pre-flight)

- Task 10 truncated-JPEG test: PLAN GOVERNS. `#expect(records.count == 1)` is
  deliberately loose — a truncated JPEG may legitimately fail cleanly or decode
  partially, and pinning either outcome makes the test brittle against ImageIO
  changes. The real assertion is that the process did not trap. Reviewers:
  this is not a defect, do not re-raise.
- phash precision: FIXED IN PLAN. Task 15 now strips `phash` from the payload
  after clustering, so the webview never receives a value JavaScript would
  silently corrupt above 2^53.

Task 14: complete (commits f6354d8..e479340, review clean — spec ✅, 2 fix rounds).
Task 14: `analyze_batches` extracted as a PURE function taking a closure, so retry and
  backfill are testable without an AppHandle. Covers: retry-then-succeed, always-fail,
  offsets preserved around a failed batch, and the wrong-count case (2 records returned
  for 3 paths must still yield 3). Offsets asserted with the path encoded in each record,
  not just totals — a totals-only check would pass while everything after a short batch
  shifted by one.
Task 14: SEVENTH vacuous test found — `timeout_has_a_floor_for_a_single_photo` passes even
  if `timeout_for` ignores its argument (10s >= 10s), saved only by a sibling test.
  `timeout_matches_exact_formula` now pins 10s + 3s/photo literally.
Task 14: `Sidecar` gained `Drop` (child is `Option<CommandChild>`, killed on drop, which
  also unblocks the drain task). `sidecar_ping.rs` now drains stderr.
Task 14: deferred by design — `SidecarPool::analyze_all`'s real respawn-on-crash path
  needs a live AppHandle and is untested. A trait abstraction purely to fake one call was
  judged not worth it.

SECURITY: `.env` was NOT gitignored while the user had put DEEPSEEK_API_KEY in it. Never
  committed (verified via `git log -S`), fixed at that point. Implementer agents run
  `git add -A` routinely, so the next commit would have captured it. For Phase 5 the key
  belongs in the OS keychain per the design — `.env` also cannot work in a packaged app,
  since envPrefix is ["VITE_","TAURI_"] and a shipped .app has no .env beside it.

Task 13: complete (commits a8e65dd..f6354d8, review clean — no Critical/Important).
Task 13: FIRST TASK WHERE THE BRIEF'S OWN TESTS SURVIVED MUTATION. The implementer built
  three real mutants (sorted-order output, position-broken ties, `<` vs `<=`) and all
  three were caught.
Task 13: NaN contained inside `percentiles` rather than delegated to callers. It did not
  panic — it silently corrupted the ranks of OTHER values, because
  `partial_cmp(..).unwrap_or(Equal)` made the sort order incoherent and broke
  `partition_point`. Now NaN is filtered before sorting; NaN gets 0, others get exactly
  the ranks they would have had absent it. Mutation-proved that a "NaN gets 0" test alone
  PASSES with the bug present — only the non-perturbation test catches it.
Task 13: SEAM FOR TASK 15 — percentile 0 is overloaded: it means both "genuine bottom of
  a large population" and "this input was NaN". Task 15 cannot distinguish them from the
  returned Vec<u8>. If it ever needs to exclude unrankable photos rather than merely rank
  them last, it must filter NaN itself before calling.

Task 12: complete (commits fa2a91a..a8e65dd, review clean, 2 fix rounds).
Task 12: THREE vacuous/wrong things in one task, all mine. (a) the brief's
  `events_split_on_a_large_time_gap` uses already-sorted input, so it passes even if the
  function returns ids in SORTED order instead of input order — caught by the implementer,
  replaced with genuinely out-of-order input. (b) `event_clusters` broke its dense-id
  property when ALL timestamps are None: `current` stays 0 but `undated = current + 1`
  gave 1, so id 0 was unused and Task 15 would see a phantom empty chapter — caught by the
  reviewer. (c) `tied_timestamps_do_not_split` used a 0-second gap against an 86400s
  threshold, so `>` and `>=` both evaluate false and it could not pin the operator — the
  reviewer proved it by mutating to `>=` and watching all 12 tests still pass. Fixed with
  a gap exactly equal to the threshold.
Task 12: CONTRACT for Task 15 — `event_clusters` returns ids in INPUT order, not sorted
  order; callers index positionally. Undated photos form their own cluster (id 0 when
  nothing is dated, otherwise the id after the dated groups). Ids are dense.

Task 11: complete (commits 5c44e71..fa2a91a, review clean — spec ✅, quality approved,
  no Critical findings). Closed a real gap before review: `hashes_needing_analysis` was
  correct only by delegating to `get_features`. A version-blind reimplementation would
  pass every original test and, after an ANALYZER_VERSION bump, report nothing needs
  re-analysis — serving stale features forever with no error. Mutation-checked.
Task 11: RESOLVED by controller — reviewer flagged that `rusqlite::Connection` is not
  `Sync`, so `Db` as Tauri managed state would need `Mutex<Db>`. Task 15 opens the
  connection locally inside the command rather than registering it, so this never arises.
Task 11: deferred (minor) — `hashes_needing_analysis` is N queries rather than one
  batched `WHERE hash IN (...)`. Brief-verbatim, fine at hundreds of photos.
Task 11: deferred (minor) — `idx_features_version` indexes a column no query filters on
  alone; speculative. And `Db::open(path)` against a real file is untested (only
  `open_in_memory`), so file-path handling is uncovered.

Task 10: complete (commits 177af08..5c44e71, review clean, 3 fix rounds).
Task 10: PRODUCTION FIX SHIPPED — `Analyzer.visionSemaphore`, cap 4, guarding ONLY
  `VisionAnalyzer.analyze`. Metrics run unguarded at full width; narrowing that scope was
  worth ~a third of batch time (300 photos: 4.39s -> 2.84s at cap=4). cap=4 vs cap=2 is
  2.84s vs 3.57s, no stability difference.
Task 10: `@Suite(.serialized)` on HostileInputTests is KEPT and is a SEPARATE fix from the
  semaphore. With the semaphore alone the suite still deadlocked — blocked `wait()` threads
  starve the same pool. The semaphore fixes production (one `analyze` call); `.serialized`
  fixes a variant only swift-testing's own test parallelism produces. Do not remove either
  thinking it is redundant.
Task 10: THE WATCHDOG TOOK THREE ATTEMPTS TO ACTUALLY WORK. (1) `@Test(.timeLimit)` cannot
  fire on a synchronous body — cooperative cancellation needs a suspension point.
  (2) `DispatchQueue.global()` shares the very libdispatch pool the deadlock exhausts, so
  the watchdog block was never scheduled. (3) A raw `Thread` is kernel-scheduled and works
  — VERIFIED by reintroducing the bug: `.timedOut` at 120.023s, run continued. Each failure
  was found by actually reintroducing the deadlock, never by assuming the mechanism worked.
Task 10: honest scope note — an unbounded SINGLE `Analyzer.analyze` call over 300 photos
  did NOT deadlock in isolation (4.8s). The deadlock only appeared under cross-test
  aggregate load. So the semaphore is defence-in-depth for higher-core machines, not a fix
  for a reproduced single-batch failure. Disclosed rather than oversold.
Task 10: deferred — the semaphore is process-global/static. Safe because the sidecar's
  stdin loop is strictly sequential (verified). Would need rework if it ever handles
  overlapping batches in one process.

Task 10 origin: spec ✅, corpus judged genuinely distinct (8 fixtures: zero bytes, truncation,
  no header, corrupt Huffman scan data, 1x1, CMYK, no extension, directory path,
  permission denied). Pre-existing fixtures confirmed byte-identical. NO FIXTURE CRASHED
  — the crash-isolation premise holds.
Task 10: 🚨 CRITICAL PRODUCTION DEFECT SURFACED, fix in flight. Vision deadlocks on
  `VNControlledCapacityTasksQueue` under many simultaneous `VNImageRequestHandler` calls.
  Reproduced with pre-existing non-hostile tests, so it is NOT about fixture bytes.
  `Analyzer.analyze` uses `concurrentPerform` with up to 2 blocking `handler.perform`
  calls per iteration — the same pattern — and Task 15 runs it over 100-300 photos.
  A deadlock there hangs the app forever at ZERO CPU with no error, which is worse than a
  crash (a crash terminates and Rust can retry). `@Suite(.serialized)` bounds one test
  file and does nothing to production. Fix: DispatchSemaphore capping in-flight Vision
  work to 2-4, signalled on every exit path via `defer`, decode/metrics left at full
  width. Plus a 200-300 path stress test with a hard timeout.
Task 10: note — the agent disabled its Bash sandbox to run tests, having found Vision
  hangs under it (zero CPU growth, true deadlock) even on non-hostile tests. Sandbox
  artifact, not app code.

Task 9: fix round 1/5 (1 Critical + 1 minor addressed; commits 1d320bd..177af08)
Task 9: complete (commits 37bd094..177af08, review clean)
Task 9: THE TRAP WORTH REMEMBERING — the `autoreleasepool` was PRESENT and wrapped the
  WRONG SPAN. `loadThumbnail` forces full JPEG/HEIC decompression via
  `kCGImageSourceShouldCacheImmediately`, and it sat one line ABOVE the pool, so the pool
  bounded the cheap post-decode work and left the largest allocation unbounded across the
  batch on threads with no pool of their own. Passes a casual read. Only caught because
  the review brief explicitly said "verify it wraps the actual image work, not just a
  trivial region".
Task 9: note — the JSON contract tests decode via `JSONSerialization`, NOT Swift's own
  `Decodable`. A Swift-to-Swift round-trip would pass even with wrong key names since both
  sides share them. Keep the independent decode path; it is the entire point.
Task 9: accepted — `#SendableClosureCaptures` warning on the locked array write is
  cosmetic: every write is inside the NSLock and `concurrentPerform` blocks until all
  iterations finish, giving a happens-before edge before the final read. Opting the
  package into full Swift 6 language mode would make it a hard error needing `Mutex` or
  `@unchecked Sendable`.
Task 9: deferred — the pool-placement fix has no automated regression test; verified by
  inspection only. Not practical without a memory profiler.

Task 8: complete (commits fd1bcee..f448fef, review clean — 3 findings addressed).
  Regression test reproduces the original bug: contaminated full-face points INVERT the
  signal (smiling 0.462 < neutral 0.477); lips-only gives 0.530 > 0.263. Reviewer
  hand-recomputed the geometry and confirmed the numbers are a genuine consequence, not
  reverse-engineered. Removing the box-count guard CRASHES rather than failing — stronger
  confirmation it is load-bearing.
Task 8: 🚨 MUST VERIFY AT THE REAL-PHOTO RUN — `SmileProxy` requires at least 6 outer-lip
  points, but nobody can determine how many Vision's `outerLips` region actually returns.
  The SDK headers document only the TOTAL constellation (65 or 76 points across all
  regions); no per-region count is published anywhere available. If Vision returns fewer
  than 6, `confidence` is nil for every face and `smile_fraction` is nil for the entire
  library — silently, with every test still green. Check the actual point count first
  thing when real photos are available.
Task 8: PLAN DOC CORRECTED (Tasks 7 and 8 now carry SUPERSEDED warnings). Without them a
  future reader following the plan would reintroduce both the allPoints contamination and
  the coordinate-space bug.

Task 8: fix round 1/5 origin — CRITICAL, spans Tasks 7 and 8. `FaceObservation.landmarks`
  is Vision's `allPoints` (65-76 points: jaw contour, brows, eyes, nose, lips), but
  `SmileProxy.confidence` takes min/max x across ALL of them as "mouth corners" and the
  mean y as the mouth centre. On a real face those extremes are jaw/ear contour points,
  so the lift/width ratio is not a smile signal and would be near-constant regardless of
  expression. ALL 11 TESTS PASS because every fixture hand-builds exactly 4 mouth-only
  points — the suite is green and the feature is non-functional on real input.
  Fix: populate `outerLips` from `VNFaceLandmarks2D.outerLips` and REPLACE `landmarks`
  with it (nothing else consumes landmarks, and 65-76 points/face would be serialised to
  Rust and stored in SQLite for data nothing reads). Also: gate nil yaw/pitch to nil
  confidence (unreported pose is a cannot-tell case; scoring a possibly-profile face as
  frontal feeds culling a wrong signal), and cover `box.count != 4`.
  NOTE FOR TASK 9: `FaceObservation` changes shape — `landmarks` becomes `outerLips`.
Task 8: note — implementer's mutation checks caught two subtleties worth remembering: a
  ZERO-width box was accidentally "passing" via NaN propagation (only a NEGATIVE width
  exposed the missing guard), and a SQUARE test box hides the aspect distortion entirely
  since box.height/box.width = 1. Non-square fixtures are mandatory for that class of test.

Task 7: fix round 1/5 (1 CRITICAL addressed — landmark coordinate space; commits
  a1bb4b7..fd1bcee). Reviewer independently recomputed all three test vectors from the
  spec formula and confirmed they match, so the tests are not circular.
Task 7: complete (commits 2298e0d..fd1bcee, review clean)
Task 7: THE BUG WORTH REMEMBERING — `VNFaceLandmarkRegion2D.normalizedPoints` is
  normalised to the FACE'S OWN BOUNDING BOX, not the image. My brief only flipped y, so
  `landmarks` and `box` were silently in different coordinate spaces. Zero tests touched
  it (no fixture has a face). Caught by reading the SDK headers and noticing Vision ships
  a separate FAILABLE `pointsInImageOfSize:` — which only makes sense if the conversion
  is non-trivial.
Task 7: CONTRACT — `FaceObservation.box` AND `.landmarks` are both image-normalised with
  a top-left origin. Conversion uses the Vision-space (bottom-left) box for offset/scale,
  then flips. Do not mix the flipped box into the offset.
Task 7: CARRY TO TASK 8 — the smile proxy computes lift/width ratios. Now that landmarks
  are image-normalised, that ratio is scaled by box.height/box.width versus the
  bbox-relative quantity the thresholds assume. Task 8 MUST normalise back using `box`.
Task 7: deferred — the face path (seeding, FaceObservation construction, landmark
  conversion in situ) is never executed by any test, because no fixture contains a face.
  A synthetic drawn face will not reliably trip Vision's detector, so this waits for the
  Phase 1 exit-criteria run against real photos. Known and accepted, not hidden.

Task 6: complete (commits cc3b44a..d1c22fd, review clean — spec ✅, quality approved,
  4 fix rounds). Round 4 measured shift=0/64, exposure=0/64, unrelated=30/64 on realistic
  multi-scale content; bound set at <5 calibrated to measurement. FNV-1a stand-in fails
  all four assertions.
Task 6: deferred (minor) — the round-4 test was mutation-checked against an FNV-1a
  content hash but never against a revert to the old direct-8x8 point-sampling. Plausible
  it would catch that regression (round 3's sweep showed coarse-detail distances dropping
  20->2 and 16->7) but not directly demonstrated.

TEMPLATES MERGED at 87d04a7 (--no-ff). 40 templates + validator now on the feature branch.

TAILWIND DEFECT FIXED at 2298e0d. `app/assets/css/main.css` created with only the two
  imports (deliberately no theme block — inheriting Luna's Geist/monochrome brand by
  accident would be worse than plain defaults), `css:` wired into nuxt.config.
  `tests/scaffold.test.ts` replaced. NOTE: the implementer first tried the preferred
  component-mount test with @nuxt/test-utils, found it passed identically with and
  without the fix (happy-dom never injects Vite CSS), and DISCARDED it rather than ship
  a test that cannot fail — reverting the added deps. The shipped test is a weaker but
  genuine cross-check: css entry declared -> file exists -> file imports both packages.
  Red/green verified by removing the `css:` line. Known blind spot: cannot detect a
  Tailwind content-glob misconfiguration.

## Resolved defect (was queued)

TASK 1 DEFECT (found by the user running `bun run dev`, missed by Task 1's review):
  Tailwind/@nuxt/ui styling never loads. There is no `app/assets/css/main.css` and no
  `css:` entry in `nuxt.config.ts`, so Tailwind generates no utilities and every class
  in the templates is inert — the scaffold page renders in serif with no spacing.
  `.nuxt/ui.css` IS generated by the module; nothing imports it.
  Fix: create `app/assets/css/main.css` with `@import "tailwindcss";` and
  `@import "@nuxt/ui";` (see ~/Documents/luna-ultra-desktop/app/assets/css/main.css),
  add `css: ["~/assets/css/main.css"]` to nuxt.config.
  ROOT CAUSE OF THE MISS: `tests/scaffold.test.ts` only reads the config back to itself
  (asserts `ssr === false` and an `ignore` string), both hardcoded in the same commit.
  Task 1's reviewer flagged it as a weak test and I logged it Minor. Replace it with a
  test that renders and asserts a Tailwind-computed style. Until then every UI check
  before Task 15 is misleading, and @nuxt/ui components will render unstyled.

## Progress

Task 1: complete (commits c677839..40889c4, review clean — spec ✅, quality approved)
Task 1: deviations accepted — `defineNuxtConfig` import (Vitest plain-import needs it;
  the brief's own Step 4 is unreachable without it), placeholder `src-tauri/icons/icon.png`
  (`tauri::generate_context!()` will not compile without an icon on disk), `.gitignore`
  += `src-tauri/gen`.
Task 1: deferred — placeholder icon must be replaced via `bun run tauri icon` before
  `tauri build`. Blocks the "signed, launchable .app" exit criterion, not any task before it.
Task 1: deferred — nothing pins the build to `aarch64-apple-darwin`. No universal config
  was added so the constraint is not violated, but it is not enforced either. Belongs in
  the packaging step that first runs `tauri build`.
PARALLEL STREAM — COMPLETE, AWAITING MERGE. Branch `worktree-agent-ad93233282ef72a95`,
  commits c373cba, 32973f0, f85e38a, ee506ae. 40 templates in `templates/`, validator at
  `tests/templates.test.ts` (404 assertions), `templates/README.md`, `TEMPLATES-REPORT.md`.
  Controller-verified independently: ZERO geometry violations across all 40 on
  bleed-reaches-edge, text-inside-safe-area, text-clear-of-gutter-band.
  CONTRACT FOR PHASE 2: `aspect_pref` is a REAL-WORLD (inch) aspect ratio, not the
  normalized rect ratio. Verified numerically — 102/103 slots have their real-world ratio
  inside their declared range, 0/103 for normalized. The canvas is 2.518:1, so a scoring
  engine assuming normalized ratios would mis-score every slot. Now validator-enforced.
  Also: every template is exact-count (`min_photos == max_photos == slots.length`), so the
  Phase 2 packer selects templates by photo count rather than fitting a range.
  MERGE BLOCKED ONLY BY: concurrent git index use while Task 6's fix round runs.

Task 6: fix round 1/5 (1 addressed, 0 open — `sharpSubjectOnBlankBackgroundIsNotScoredBlurry`
  was VACUOUS: a perfectly uniform baseline has zero Laplacian variance under any
  implementation, so the assertion collapsed to `local > 0`. Replaced with
  `metricsTileMaxIgnoresEmptyAreaAroundASharpSubject`, which compares confined detail
  against full-frame detail — mutation-checked at 1/17.2 under naive whole-image
  variance. Commits 0fb064d..88444bf.) PLAN DEFECT, not an implementer error.
Task 6: fix round 4/5 IN FLIGHT — round 3 landed the palette determinism fix (commit
  6f71eec, mutation-verified across 10 separate process launches: 3 pass / 7 fail before,
  5/5 after) and the pHash 32x32 + box-average + `.high` interpolation fix. The implementer
  correctly REFUSED to add my shake-robustness test, measuring 28-32/64 bits against my
  requested <10 and reporting rather than loosening the bound.
  CONTROLLER RULING: my test design was wrong, not the implementation. A 2px checkerboard
  at 1536px is Nyquist-limit content; a 1px shift inverts the phase of the highest
  frequency in the frame, so any downsampling hash scrambles. That is information theory.
  The implementer's sweep showing the fix helps from ~24px detail upward is the real
  result, and that is where real photographic structure lives. Round 4 replaces the test
  with realistic multi-scale content (gradient + soft blobs + texture) under shift and
  exposure variation, with the bound calibrated to measured values and its provenance
  recorded. Round 4 resumes the SAME implementer deliberately — the round-4 escalation
  rule is for loops that are not converging; this one is, and the open item was a decision
  I owed it, not a failure it could not solve.
Task 6: accepted limitation — average-hash does not survive sub-pixel shifts of
  near-Nyquist detail. Inherent, not a defect. Being recorded as a comment on
  `perceptualHash` so it is not "rediscovered" as a bug later.

Task 6: fix round 3 origin — full task review (spec ✅, quality approved) surfaced
  two defects in the brief: (a) `perceptualHash` downscales 1536px straight to 8x8 with
  no interpolation quality, contradicting the brief's own documented 32x32 intermediate
  and point-sampling instead of averaging — aliases on real photos and weakens Task 12's
  burst grouping; (b) `palette` breaks ties by `Dictionary.values` iteration order, which
  Swift varies per launch, violating the design's determinism guarantee. Base 7227f17.
Task 6: RESOLVED by controller, not a gap — reviewer could not verify whether the fixed
  internal working resolutions (256/128) might upscale their input. Task 9 passes
  `analysisMaxPixel = 1536`, so these always downscale. No action.
Task 6: deferred, MEASURE AT EXIT CRITERIA — `contrast` and the per-tile sharpness
  normalisation both use raw min/max luma, which one hot pixel or specular highlight can
  saturate. Real degradation on real photographs, invisible to the synthetic fixtures.
  The right fix (percentile-based robust range) depends on what real noise looks like;
  decide at the Phase 1 run against the user's own photos.
Task 6: deferred (minor) — `rgbaBuffer` silently yields an all-zero buffer if `CGContext`
  init fails, so a render failure reads as a flat black image rather than an error.
  Inputs come from a validated `loadThumbnail`, so low likelihood.

Task 6: fix round 2/5 (2 addressed, 0 open — palette test was vacuous; pHash untested for
  the Hamming-distance property Task 12 needs. Mutation-checked: palette stub fails; the
  content-hash stub fails the absolute bound at 27 vs <10 required. Real implementation
  measures distNear=0, distUnrelated=64. Commits 88444bf..7227f17.)
Task 6: implementer audit of the remaining four tests found
  `paletteReturnsRequestedCountAndWeightsSumToOne` vacuous (a stub ignoring its input
  passes) and pHash untested for the property Task 12 depends on (that Hamming distance
  is perceptually meaningful — a plain content hash would pass both existing tests and
  make Task 12's clustering silently never group anything). Base 88444bf.
Task 6: ruling — `sharpEdgesScoreHigherThanFlatField` (passes under a contrast-only
  implementation) and `identicalImagesHaveIdenticalHash` (weak alone) are left as-is.
  The new tests pin the real behaviour; these remain cheap sanity checks.

Task 5: fix round 1/5 (2 addressed, 0 open — DateFormatter had no explicit locale so
  captureDate silently nils on non-Gregorian-calendar machines; GPS negation and date
  parsing had no test; commits 735409c..cc3b44a)
Task 5: complete (commits 6129af0..cc3b44a, review clean — spec ✅, quality approved)
Task 5: note — swift-testing's top-level `@Test func`s share the module namespace, so
  test names must be unique repo-wide. Task 5 hit a real collision with Task 4's
  `throwsOnMissingFile`. Tell every later implementer to prefix test names by area.
Task 5: note — `sidecar/Fixtures/gps-south-west.jpg` asserts sign AND magnitude, and the
  date assertion compares `timeIntervalSince1970` numerically. Mutation-checked: flipping
  the S/W negation fails the tests. Do not weaken these to non-nil checks.
Task 5: deferred (minor) — no positive-hemisphere ("N"/"E") GPS test, so only the
  negation path is exercised. Add if GPS coverage matters more later.
Task 5: deferred (minor) — `writeWithGpsAndDate` duplicates `write`'s CGContext
  boilerplate in the fixture generator. Cosmetic, test-only script.

Task 4: complete (commits 8712178..6129af0, review clean — spec ✅, quality approved)
Task 4: plan amended before dispatch — ImageMagick and exiftool are NOT installed on this
  machine. Fixtures now come from `scripts/make-fixtures.swift` via ImageIO, so the suite
  is reproducible from a fresh clone with no external tooling. Task 10's hostile corpus
  was amended the same way (commit 8712178).
Task 4: verified — reviewer independently confirmed `portrait-rot90.jpg` is tag-only
  (stored 1200x800, EXIF orientation 6, NOT pixel-rotated) and reproduced the negative
  control: without `kCGImageSourceCreateThumbnailWithTransform` it decodes 512x341
  landscape, with it 341x512 portrait. The test has real teeth.
Task 4: deferred (minor) — no automated check ties the committed fixture JPEGs to
  `make-fixtures.swift`. Editing the script without rerunning it would let them drift.
  Task 10 touches this script; add a note or check there.
Task 4: deferred (minor) — `ImageLoader.LoadError` is not `Equatable`. Fine for the
  current type-based `#expect(throws:)`; needs it if a later task asserts a specific case.
Task 4: deferred — no HEIC fixture exists, so the `irot`/`imir` vs EXIF reconciliation
  path is untested. Only JPEG orientation is covered. The Phase 1 exit criteria include a
  run against real HEIC photos, which is where this gets exercised.

Task 3: fix round 1/5 (2 addressed, 0 open — build-ordering trap; missing cross-language
  round-trip test; commits ba4865f..078aec5)
Task 3: complete (commits 6205658..078aec5, review clean — spec ✅, quality approved)
Task 3: note — `src-tauri/tests/sidecar_ping.rs` spawns the REAL Swift binary, path
  resolved from `CARGO_MANIFEST_DIR` + `TAURI_ENV_TARGET_TRIPLE` (set by tauri-build),
  with a real 5s `recv_timeout`. It genuinely fails if either side's wire format drifts.
  This is the guard against Swift/Rust divergence — do not weaken it.
Task 3: deferred (minor) — `sidecar_ping.rs` pipes the child's stderr but never reads it.
  Harmless for ping, but if a later `analyze` path writes more than the pipe buffer to
  stderr the child could block while the timeout only watches stdout. Revisit in Task 14.
Task 3: deferred (minor) — `build-sidecar.sh` takes the filename triple from
  `rustc --print host-tuple` while forcing the Swift target to arm64. Divergent on a
  non-arm64 host. Project is arm64-only so low risk; hardcoding would be clearer.
Task 3: deferred (minor) — `Sidecar` has no `Drop`, so dropping one leaks the child
  process and the drain task. Task 14 owns lifecycle; fold it in there.
Task 3: deferred (minor) — `shell:allow-execute` is correctly shaped but inert for the
  direct-from-Rust path: `Shell::sidecar()` → `Command::new_sidecar` performs no ACL
  check. Scope checks only apply to the JS-invoked IPC bridge. Not a defect; know it is
  not yet a real security boundary.

Task 2: fix round 1/5 (2 addressed, 0 open — malformed requests lost their `id`; encode
  failures were silent; commits 3f77a28..6205658)
Task 2: complete (commits 40889c4..6205658, review clean — spec ✅, quality approved)
Task 2: accepted — line handling extracted to `Handler.swift` (not in the brief's file
  list). `@testable import` of an executable target's `main.swift` segfaults under SwiftPM,
  so the extraction was the only route to unit-testable line handling. Pre-authorised.
Task 2: deferred — `emit()`'s last-resort encode-failure branch is inspection-verified,
  not test-covered. `Response`'s fields are all `String`, so encoding cannot be made to
  fail without adding an injectable-encoder seam. Fair call, not worth the seam.
Task 2: deferred (minor) — `JSONEncoder` does not use `.sortedKeys`, so response key order
  varies between lines. Pre-existing, and serde is key-order-independent, so no impact.

Task 1: deferred (plan-mandated) — `.oxlintrc.json` enables only `correctness`/`suspicious`,
  so oxlint's `no-explicit-any` is off and the "never use `any`" constraint rests on
  discipline. Config came verbatim from the brief. Ruling: not a Task 1 defect; fold the
  TypeScript category into the lint config at the first task that touches `.oxlintrc.json`.
