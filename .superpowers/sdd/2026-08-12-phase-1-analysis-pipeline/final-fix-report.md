# Final fix wave — pre-merge review findings

Branch: `feat/phase-1-analysis-pipeline`. This is the single fix wave for the
whole-branch review before merge. All items below were fixed except none
skipped — the four "cheap" items were all straightforward and included.

## C1 — `smileFraction` renders as literal "NaN%"

**Root cause confirmed:** Swift's synthesized `Codable` uses `encodeIfPresent`
for optionals, so a `nil` `smileFraction` never reaches the wire as `null` —
the key is omitted entirely. `JSON.parse` then leaves it `undefined`.
`app/components/PhotoTile.vue` compared `photo.smileFraction === null`, which
is `false` for `undefined`, so it fell through to `Math.round(undefined *
100)` → `NaN`, and the `smilePct !== null` render guard let `NaN` through
(`NaN !== null` is `true`).

**Fix:**
- Added `smilePercent()` to `app/types/features.ts` — a single, testable
  choke point using `== null` (loose equality, catches both `null` and
  `undefined`), replacing the inline `=== null` logic in `PhotoTile.vue`.
- Corrected `AnalyzedPhoto.smileFraction` to `smileFraction?: number | null`
  and `AnalyzedPhoto.thumbnailPath` to `thumbnailPath?: string | null` (the
  audited "safe by accident" field — behavior was already correct via a
  truthy check, but the type was a lie; fixed for correctness per project
  convention).
- Audited every other optional crossing the Swift→TS boundary
  (`saliencyBox`, `horizonTiltDeg`, `captureDate`, `latitude`, `longitude`,
  `FaceObservation`'s optional fields): none of them are part of the
  `AnalyzedPhoto` TS interface or read anywhere in `app/`, so there was
  nothing to misbehave. On the Rust side, `finalize_photos` in
  `commands.rs` only ever reads through `serde_json::Value`'s `.as_f64()` /
  `.as_str()` / `.as_bool()` accessors, and `serde_json::Value`'s `Index`
  impl returns `Value::Null` for a missing key exactly as it would for an
  explicit `null` — so a missing key and an explicit null are already
  indistinguishable there, and safe.
- New test in `tests/features.test.ts`: constructs the record via
  `JSON.parse('{"path":"/p/a.jpg","aestheticPct":50}')` (no `smileFraction`
  key at all — the actual backend shape) and asserts `smilePercent(...)`
  returns `null`, not `NaN`. Also flagged in the diff: the existing test
  helper `photo()` still defaults `smileFraction: null`, a shape the
  backend never produces on its own — left in place since it's still a
  valid input the type allows, but the new test closes the actual gap.

**Files:** `app/types/features.ts`, `app/components/PhotoTile.vue`,
`tests/features.test.ts`.

## C2 — Cache hits carry a stale `path`

**Fix:** In `lookup_cache` (`src-tauri/src/commands.rs`), a cache hit now
overwrites `features["path"]` with the path passed to *this* call before
pushing it into `hits`, instead of trusting whatever path was baked into the
cached JSON blob at first-analysis time.

**Tests added:**
1. `commands::tests::a_cache_hit_carries_the_current_path_not_the_stale_cached_one`
   — unit test at the `lookup_cache` level (no live `AppHandle` needed):
   writes a cache row under `path_a`, then looks up a byte-identical file at
   `path_b`, and asserts the returned `path` is `path_b`.
2. `src-tauri/tests/cache_hit.rs` — new `harness = false` integration test
   (same pattern as `sidecar_worker_pool.rs`, needed because `analyze_folder`
   requires a live `AppHandle`/`Wry` app built on the real main thread).
   Runs `analyze_folder` twice over the same folder with a persistent fake
   `HOME`, and asserts `summary.cached > 0` on the second run plus that
   every returned photo path points into the current fixture directory.
   This is the exact end-to-end path the bug lived on — the only prior
   end-to-end test (`sidecar_worker_pool.rs`) redirects `HOME` to a *fresh*
   temp dir every run, so the cache-hit branch had never executed
   end-to-end before.

**Files:** `src-tauri/src/commands.rs`, `src-tauri/tests/cache_hit.rs`,
`src-tauri/Cargo.toml` (registered the new `[[test]]`).

## I1 — No logger installed

Added `tauri-plugin-log = "2"` (v2.9.0 resolved) and registered it in a new
shared `app_lib::builder()` function in `lib.rs`, which both `run()` (the
real app) and the two `harness = false` integration tests now use — so the
diagnostic wiring is exercised by tests, not just declared. Default targets
(stdout + rotating log-dir file) require no extra configuration.

**Verified live** (see "Mutation checks / live verification" below): a
throwaway test forced a real permission-denied file into `analyze_folder`
and confirmed the exact `commands.rs:199` `log::warn!("cannot hash...")`
call now surfaces as `[WARN] cannot hash .../unreadable.jpg: Permission
denied (os error 13)` via the installed logger. Deleted after verification
(not part of the permanent suite — a chmod-based test is fragile as CI, and
running as root ignores permissions).

**Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/lib.rs`,
`src-tauri/tests/sidecar_worker_pool.rs` (now uses the shared `builder()`).

## I2 — Two error paths discard completed work

Both converted from `?` (abort the whole command) to log-and-continue,
matching the existing pattern at the notification-failure site:
- `create_dir_all(&thumbnail_dir)` — logs and proceeds; the sidecar's own
  `ThumbnailWriter.write` makes an independent per-photo attempt and
  degrades to a `nil` `thumbnailPath` on failure regardless.
- `db.put_features(...)` in the write-back loop — logs and proceeds; a
  failed cache write no longer discards a photo the sidecar already spent
  potentially multi-minute analysis producing.

**Files:** `src-tauri/src/commands.rs`.

## I3 — Event chapters ordered by filename, not chronology

**Fix:** `groupByEvent` in `app/types/features.ts` now sorts groups by
`eventCluster` id ascending (which `cluster::event_clusters` assigns
chronologically, with the undated bucket always getting the highest id),
instead of ordering by first appearance in the (path-sorted) input array.

**Tests:**
- Updated `"orders groups by first appearance..."` → renamed and inverted to
  assert chronological ordering by id.
- Added a new test with input constructed so path order and capture order
  disagree (`eventCluster: 5` appears first in the array, `eventCluster: 1`
  second), asserting the output is ordered `[1, 5]`.

**Files:** `app/types/features.ts`, `tests/features.test.ts`.

## I4 — Cross-language SHA-256 agreement unpinned

Pinned both `hash_file` (Rust) and `contentHash` (Swift) against the same
committed fixture (`sidecar/Fixtures/landscape.jpg`) and the same literal
hex string (`08c8f73e...20159`, computed via `shasum -a 256`):
- Rust: `commands::tests::hash_matches_the_literal_pinned_against_swifts_content_hash`.
- Swift: `analyzerHashMatchesThePinnedRustLiteral` in `AnalyzerTests.swift`.

`Analyzer.contentHash` was changed from `private` to internal visibility
(matching the existing project convention already used for `topLeft` /
`imageNormalizedTopLeft` in `VisionAnalyzer.swift`) so the Swift test can
call it directly instead of going through the full `Analyzer.analyze`
pipeline — see "an issue I introduced and fixed" below for why that matters.

**Files:** `src-tauri/src/commands.rs`,
`sidecar/Sources/PhotobookEngine/Analyzer.swift`,
`sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`.

## I5 (scoped) — Feature-record field names unpinned

Added `photoFeaturesEncodesExactlyThePinnedTopLevelKeySet` to
`AnalyzerTests.swift`: builds a `PhotoFeatures` with every optional field
populated (non-nil, so `encodeIfPresent` doesn't drop anything), encodes it,
decodes via `JSONSerialization` (not `PhotoFeatures`' own `Decodable` — a
shared rename applied to both encode and decode sides would pass unchanged
and prove nothing), and asserts the top-level key set matches a pinned
`Set<String>` exactly — failing on both missing and unexpected keys.

**Files:** `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`.

## Escalated — SmileProxy comment + diagnostic

- Reworded the `minOuterLipPoints` doc comment in `SmileProxy.swift`: no
  longer states "Vision's `outerLips` region reliably reports roughly 8-12
  points" as fact. Now explicitly says this is an unverified assumption
  (Vision only documents the *total* face-mesh count, 65/76, never a
  per-region breakdown), cross-references `docs/PROJECT-STATUS.md`'s "NOT
  verified" list, and states the consequence (silently disables smile
  detection library-wide if wrong).
- Added a one-line `FileHandle.standardError.write` warning in
  `confidence(for:)` when `outerLips` is non-nil but under
  `minOuterLipPoints`, distinguishing "too few points" from "bad pose" —
  both previously collapsed silently to the same `nil` return. **Verified
  live**: this fired during the real Swift test run (see mutation section
  below) — `PhotobookEngine: outerLips has 4 points, below
  minOuterLipPoints (6)`.

**Files:** `sidecar/Sources/PhotobookEngine/SmileProxy.swift`.

## Cheap items — all four included

- **M2:** Removed all four artifacts: `fs:default` from
  `capabilities/default.json`, `@tauri-apps/plugin-fs` from `package.json`
  (via `bun remove`), `tauri-plugin-fs` from `Cargo.toml` (via `cargo
  remove`), and its `.plugin(tauri_plugin_fs::init())` call from `lib.rs`.
  Verified zero remaining references via grep before removing.
- **M3:** Added `base-uri 'self'; form-action 'none'` to the CSP in
  `tauri.conf.json`.
- **M7:** Added `is_apple_double()` (`name.starts_with("._")`) to
  `commands.rs`, applied in the folder-scan filter chain alongside
  `supported_extension`, with two new unit tests.
- **M4:** Deleted `Sidecar::ping()` (confirmed zero callers anywhere,
  including tests — `sidecar_ping.rs` builds a `Request` with
  `RequestKind::Ping` directly and never calls this method) and
  `Db::hashes_needing_analysis` plus its two now-pointless tests (confirmed
  zero callers via grep beyond its own definition/tests).

## An issue I introduced and fixed along the way

The first version of the I4 Swift test called `Analyzer.analyze(paths:)`
(the full pipeline, invoking Vision) to check the returned hash. Running the
full Swift suite immediately after showed
`analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency` (a
pre-existing 300-photo Vision-concurrency deadlock stress test) **failed
with a genuine 120s timeout** — a real regression, not flakiness. The test
file's own extensive comments already document this exact failure mode:
swift-testing runs `@Test` functions concurrently by default, and adding an
*independent* Vision-calling `@Test` function races against the stress test
and has reproducibly starved Vision's internal
`VNControlledCapacityTasksQueue` before (this is why
`analyzerThumbnailWriting` deliberately combines three cases into one `@Test`
function instead of three). My new test was exactly that mistake. Fixed by
exposing `Analyzer.contentHash` (removing `private`) so the test can hash
the fixture directly without touching Vision or `concurrentPerform` at all —
after which a full clean `swift test` run passed all 73 tests, including the
300-photo stress test.

## Mutation checks (C1, C2, I3, I4) — required, with output

All four were verified to fail when the behavior they name is broken, then
restored and re-verified passing.

**C1** — reverted `smilePercent`'s `== null` to `=== null`:
```
✘ smilePercent > returns null when the backend omits smileFraction entirely (undefined, not null)
AssertionError: expected NaN to be null
```

**I3** — reverted `groupByEvent` to first-appearance-in-input ordering:
```
✘ orders groups chronologically by eventCluster id, not by first appearance in the input
  expected [ 5, 1 ] to deeply equal [ 1, 5 ]
✘ orders groups chronologically when path order and capture order disagree
  expected [ 5, 1 ] to deeply equal [ 1, 5 ]
```

**C2** — reverted `lookup_cache`'s hit arm to push cached features verbatim:
```
thread 'commands::tests::a_cache_hit_carries_the_current_path_not_the_stale_cached_one' panicked:
assertion `left == right` failed: a cache hit must carry the path passed in THIS call, not the stale path baked into the cached row
  left: ".../stale-path-original.jpg"
 right: ".../stale-path-duplicate.jpg"
```

**I4 (Swift side)** — changed `String(format: "%02x", ...)` to `%02X` (uppercase hex):
```
✘ Test run with 1 test in 0 suites failed after 0.001 seconds with 1 issue.
```

**I4 (Rust side)** — changed `format!("{:x}", ...)` to `{:X}`:
```
thread 'commands::tests::hash_matches_the_literal_pinned_against_swifts_content_hash' panicked:
assertion `left == right` failed: ...
  left: "08C8F73E189BA397FF2097FE192831E8F266F98538233E9B12B5790E65720159"
 right: "08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159"
```

Every mutation was restored immediately after capturing the failure, and the
full relevant suite re-run green afterward.

## Verified live (not just unit tests)

- `bun run sidecar` (release build) — clean, only the known pre-existing
  Swift `#SendableClosureCaptures` warning in `Analyzer.swift` (untouched
  code, unrelated to this wave).
- `cargo build` / `cargo clippy --all-targets` — clean apart from the one
  known pre-existing `cluster.rs` `ptr_arg` warning (I introduced and then
  fixed one *new* clippy warning of my own, `cloned_ref_to_slice_refs` in my
  new `commands.rs` test — see diff).
- Full `cargo test` (unit + both `harness=false` e2e binaries +
  `sidecar_ping`) — 75 lib tests + 1 + the two custom-output e2e binaries,
  all green, run against the freshly rebuilt sidecar binary.
- Full `swift test` — 73/73 green (was 71 per `PROJECT-STATUS.md`; +2 net:
  I4's Swift test and I5's contract test).
- Full `bun run test` (vitest) — 429/429 green (was 425; +4 net: 3
  `smilePercent` tests, net +1 `groupByEvent` test after replacing one).
- `bun run lint` (oxlint --deny-warnings) — clean. Caught and fixed one
  warning I introduced (`unicorn/no-array-sort` on `groupByEvent`'s
  `.sort()` — switched to `.toSorted()`).
- **`bun tauri build --bundles app`** — full release build, ran to
  completion, produced `PhotobookGen.app`. Confirms the M2 dependency/plugin
  removal and the I1 addition don't break bundling or config validation.
- I1's diagnostic wiring verified live with a throwaway (deleted)
  integration test: a chmod-000 file fed through the real `analyze_folder`
  path produced `[WARN] cannot hash .../unreadable.jpg: Permission denied
  (os error 13)` on stdout via the installed `tauri-plugin-log` logger.

## What was skipped and why

Nothing from the assigned scope was skipped. Explicitly out of scope per the
task brief and not touched: vacuous tests in `VisionAnalyzerTests` /
`MetricsTests` / `templates.test.ts`, thumbnail eviction, `JSONEncoder`
date-strategy test config, the untested-behaviour list in
`PROJECT-STATUS.md` items 1–7 (RAW/HEIC throughput, the real face code
path, Pixajoy page-count semantics, the trim-vs-safe-text guide question,
CSP in a packaged bundle beyond what `bun tauri build` exercises, the
notification banner actually rendering on screen).
