# Streaming analysis results to the UI — implementation report

Branch: `feat/phase-1-analysis-pipeline`

## Addendum: `invalid args onEvent` regression, diagnosed and closed

The user hit `invalid args \`onEvent\` for command \`analyze_folder\`: command
analyze_folder missing required key onEvent` on the running app after the
first commit landed. This section is the diagnosis; the original report below
is otherwise unchanged.

**Ruled out, in order, with evidence:**

1. **Argument-name mapping (checked first since it's cheapest to disprove).**
   Read `tauri-macros-2.6.3`'s `wrapper.rs` directly: `WrapperAttributes`
   defaults `argument_case` to `ArgumentCase::Camel`, and `parse_arg` calls
   `key.to_lower_camel_case()` on every argument unconditionally unless
   `rename_all = "snake_case"` is set (it isn't here). So `on_event` **is**
   mapped to `onEvent` by default — confirmed from source, not assumed.
2. **Channel serialization across the version boundary.** `@tauri-apps/api`
   resolves to exactly `2.11.1` (`node_modules/@tauri-apps/api/package.json`);
   `tauri` resolves to `2.11.5` (`Cargo.lock`). Grepped both: JS's `Channel`
   serializes to `` `__CHANNEL__:${id}` `` (`core.js`); Rust's
   `IPC_PAYLOAD_PREFIX` is `"__CHANNEL__:"` (`tauri-2.11.5/src/ipc/channel.rs`).
   Identical sentinel. Not a version-skew bug.
3. **Registration path.** `src-tauri/src/lib.rs:32` —
   `tauri::generate_handler![commands::analyze_folder]` — still references the
   one function, now with the new signature. Nothing shadows it.
4. **Stale build — confirmed as the actual cause.** Multiple stray `bun run
   dev` processes had accumulated across my own earlier verification passes
   (I'd killed some but not all; found and killed 4 leftover processes at one
   point, then found the app running again unexpectedly later in the session).
   One of those stale processes is almost certainly what the user hit: its
   Rust binary had been hot-rebuilt by `tauri dev`'s file watcher to the new
   `on_event`-requiring signature mid-session, while its webview had a module
   graph from before `useAnalysis.ts` was updated to construct and send a
   `Channel` — so the frontend called `invoke("analyze_folder", { folder })`
   with no `onEvent` key at all, and the new Rust signature correctly
   rejected it. This is exactly the failure mode a stale build produces and
   exactly what the error text says (a **missing** key, not a malformed one).

**Verified through the real UI path (not just the Rust-example harness from
the original report):**

```
pkill -f "target/debug/PhotobookGen"; pkill -f "nuxt dev"
rm -rf .nuxt node_modules/.cache node_modules/.vite .output
rm -f src-tauri/target/debug/PhotobookGen
bun run sidecar
cargo build --manifest-path src-tauri/Cargo.toml
```

Then, since the folder picker still can't be clicked through in this
environment (Accessibility permission still unavailable — same wall as
before, not forced again), added a temporary `onMounted` hook to
`app/pages/index.vue` that set `folder.value` to a real 48-photo test folder
and called the existing `retry()` (no new code path, just the composable's
own entry point), ran `bun run dev` fresh, and screenshotted the actual app
window. Result: no `invalid args` error anywhere in the Rust log; the UI
reached the `results` state with `48 scanned / 48 analyzed / 0 from cache / 0
failed / 3 keepers`, real thumbnails, correct burst badges, hero badges, and
per-photo aesthetic/sharpness percentiles — i.e. `Batch` events streamed
partial tiles and the `Done` event correctly merged in ranks, all through the
real webview → IPC → Rust boundary, not a Rust-only harness. The `onMounted`
scaffolding was reverted immediately after (`git diff` confirmed `index.vue`
returned to exactly its committed state before re-running the full test
suite).

A second attempt at also catching the transient mid-run "growing grid" frame
(rather than just start/end) was inconclusive — the app window closed
mid-capture on the shared desktop before a mid-run screenshot landed, and the
run is fast enough (~1s for 48 tiny fixtures) that it's an easy miss. Not
pursued further: the first run already proves the boundary end-to-end
(no error, correct streamed+merged output), which is the property in
question; the visual mid-run frame is corroborating, not load-bearing.

**No code fix was needed for the bug itself** — the command signature, the
macro's argument mapping, and the Channel wire format were all already
correct. The fix is procedural: don't leave stale `bun run dev` processes
running across a `src-tauri/` signature change; kill and restart clean. This
is worth calling out explicitly rather than editing something that was never
wrong.

**Also fixed while here:** `AnalysisEvent::Batch`'s `analysed`/`cached`/
`failed` counts were documented as cumulative but didn't say what breaks if a
consumer sums them across events. Both `commands.rs` (Rust) and
`app/types/features.ts` (TS) now say explicitly: read the latest event's
value directly, never sum across events, because summing double-counts every
photo already reflected in an earlier `Batch`. Doc-only change, committed as
`a940832`.

---

## The problem

`analyze_folder` processed a whole folder and returned one `AnalysisSummary` at
the end. On a folder of hundreds/thousands of RAW files the UI showed an
indeterminate progress bar and nothing else for the entire run. The design
called for streaming records to the UI as batches complete, using
`tauri::ipc::Channel<T>` (not Tauri's event system, which its own docs
describe as JSON-string-only and unsuited to high-throughput/low-latency
streaming).

## The hard part, and how it was resolved

Several derived values (`percentiles`, `nearDupCluster`/`eventCluster`) are
computed across the **whole** set and are meaningless — and would visibly
change — if assigned from a partial population. `finalize_photos` also
depends on sorting happening *before* those arrays are computed and read back
by index; this is a documented trap in the project (`event_clusters` and
`percentiles` return values in input order, zipped positionally).

Resolution, exactly along the lines the brief suggested:

- **Stream partial records.** A new pure function, `partial_photo` (in
  `src-tauri/src/commands.rs`), projects a full features record down to
  everything intrinsic to ONE photo — `path`, `hash`, `width`, `height`,
  `isUtility`, `faceCount` (derived from the `faces` array), `sceneTags`,
  `smileFraction`, `thumbnailPath`. It never touches `nearDupCluster`,
  `eventCluster`, `aestheticPct`, or `sharpnessPct`. These are streamed in
  `AnalysisEvent::Batch` events, one per resolved batch (the immediate
  cache-hit batch, then one per `sidecar::BATCH_SIZE`-sized sidecar batch).
- **`finalize_photos` is unchanged.** It's still the one and only place
  percentiles/cluster ids get computed, still sorts before deriving, still
  zips positionally. It now feeds a single `AnalysisEvent::Done { summary }`
  event fired once at the very end, instead of being the function's whole
  return value in isolation.
- **The frontend never shows a rank that will change.** `PartialAnalyzedPhoto`
  (new type in `app/types/features.ts`) structurally *lacks*
  `aestheticPct`/`sharpnessPct`/`nearDupCluster`/`eventCluster` — a tile
  rendered from a `Batch` event cannot access fields that don't exist on its
  type. `AnalyzedPhoto extends PartialAnalyzedPhoto` adds them, and only
  photos from the final `Done` summary are ever typed that way.
  `PhotoTile.vue` uses a type guard (`isRanked`) to decide whether to render
  the aesthetic/sharpness badges or a plain "unranked" indicator — no
  placeholder number that later jumps.

### Why this preserves positional correspondence

The risk named in the brief is silently attributing one photo's data to
another via a positional zip. Two places could have broken this and didn't:

1. **`finalize_photos` itself is untouched** — same sort-then-derive-then-zip
   code as before, same existing tests
   (`attributes_percentile_data_to_the_correct_photo_after_sorting`,
   `attributes_event_cluster_to_the_correct_photo_after_sorting`).
   Mutation-checked below.
2. **The new streaming path never zips by index at all.** `partial_photo`
   operates on one record at a time (no array, no index). `batch_progress`
   splits one already-resolved batch into `(ok photos, failed count)` by
   iterating and branching on `status`, not by position. The frontend's
   `applyAnalysisEvent` reducer *appends* `event.photos` to an array — it
   never reads anything back out by a computed index either. There is no
   positional zip anywhere in the new code for this risk to hide in.

## What was built

### Rust (`src-tauri/src/`)

- **`sidecar.rs`**: `analyze_batches` (retry/backfill core) is now a thin
  wrapper around `analyze_batches_with_progress`, which adds an `on_batch`
  callback fired once per batch with that batch's already-resolved records
  (before they're appended to the running output) — so a caller can stream
  without duplicating the retry logic. `SidecarPool::analyze_all` gained an
  `on_batch` parameter it threads straight through.
- **`commands.rs`**:
  - `AnalysisEvent` enum (`Scanned { total }`, `Batch { photos, analysed,
    cached, failed }`, `Done { summary }`), `#[serde(rename_all =
    "camelCase", tag = "kind")]`, matching the brief's suggested shape.
  - `partial_photo(features) -> Value` — the intrinsic-fields projection
    described above. Preserves Swift's `encodeIfPresent` omission semantics:
    `smileFraction`/`thumbnailPath` are copied only when present in the
    source, never manufactured as an explicit `null`.
  - `batch_progress(records) -> (Vec<Value>, usize)` — pure ok/failed split
    of one resolved batch into streamable partials + a failure count.
  - `analyze_folder` gained an `on_event: Channel<AnalysisEvent>` parameter.
    Emits `Scanned` right after the folder scan, one `Batch` for cache hits
    (if any) before the sidecar work starts, one `Batch` per sidecar batch
    (via the `on_batch` callback, cumulative running counts), and one `Done`
    at the very end. All `.send()` failures are logged and swallowed —
    non-fatal, same pattern as the existing completion-notification path.
  - `AnalysisSummary` gained `Clone` (needed to send it in `Done` and still
    return it as the command's own resolved value).

### TypeScript (`app/`)

- **`app/types/features.ts`**: split `AnalyzedPhoto` into
  `PartialAnalyzedPhoto` (intrinsic fields) and `AnalyzedPhoto extends
  PartialAnalyzedPhoto` (adds the four whole-set fields). Added `isRanked`
  type guard, `ScannedEvent`/`BatchEvent`/`DoneEvent`/`AnalysisEvent` types
  mirroring Rust's wire shape field-for-field, `StreamState`, and the pure
  reducer `applyAnalysisEvent(state, event) -> state` — the entire client-side
  accumulation logic, extracted so it's unit-testable without a live
  `Channel`/`invoke`.
- **`app/composables/useAnalysis.ts`**: `analyze()` now constructs a `new
  Channel<AnalysisEvent>()`, wires `onmessage` to `applyAnalysisEvent`, and
  passes it as `onEvent` to `invoke("analyze_folder", ...)`. Exposes
  `scannedTotal`, `processed` (cumulative analysed+cached+failed), and
  `partialPhotos` (arrival-order array) alongside the existing `summary`.
- **`app/components/PhotoTile.vue`**: `photo` prop widened to
  `AnalyzedPhoto | PartialAnalyzedPhoto`. Aesthetic/sharpness badges only
  render when `isRanked(photo)`; otherwise a small "unranked" indicator with
  a clock icon, at full `text-white` for contrast parity with the rest of the
  caption (no opacity reduction, to preserve the AA contrast the design
  review already validated).
- **`app/pages/index.vue`**: `running` state now shows a determinate
  `UProgress` (`model-value=processed`, `max=scannedTotal`; falls back to
  indeterminate — `null` model value — for the brief window before `Scanned`
  arrives), "`processed` / `scannedTotal` analyzed" text in the same
  monospace-tabular-nums style as the results header, and a growing grid of
  real `PhotoTile`s built from `partialPhotos`, padded with a capped number
  (≤48) of `USkeleton` placeholders for "more coming" without rendering
  thousands of empty nodes on huge folders. State machine (`entry` / `running`
  / `error` / `no-images` / `no-analyzed` / `results`) is unchanged.

## Tests written

**Rust** (`cargo test --manifest-path src-tauri/Cargo.toml`): 90 unit tests
(up from 74), 4 integration tests (up from 2, all green):

- `sidecar.rs`: 3 new tests on `analyze_batches_with_progress` —
  `on_batch` fires once per batch with that batch's own records;
  concatenating everything `on_batch` receives equals the function's return
  value exactly; a failed batch does not shift a later batch's streamed
  paths.
- `commands.rs`: `partial_photo` (5 tests: intrinsic fields carried,
  `faceCount` derived, `phash` stripped, never carries a whole-set
  derivation even if the source somehow had one, absent `smileFraction`
  stays absent rather than becoming `null`); `batch_progress` (3 tests: ok/
  failed split, empty batch, all-failed batch); two accumulation tests
  wiring `sidecar::analyze_batches_with_progress` + `batch_progress` together
  the same way `analyze_folder` does — partial photos accumulate in input
  order across batches, and a failed batch does not shift or corrupt later
  streamed photos (the brief's two required properties, both explicit
  tests).
- New integration test `src-tauri/tests/streaming_order.rs` (`harness =
  false`, real `AppHandle`, real sidecar binary, real `sidecar/Fixtures/`):
  asserts the wire-level invariant the whole feature is for — at least one
  real `batch` event (carrying real partial photos) arrives strictly before
  the single `done` event, and every photo in the final summary was already
  streamed via a `batch` event first.
- `cache_hit.rs` / `sidecar_worker_pool.rs` updated for the new `on_event`
  parameter (no-op `Channel::new(|_| Ok(()))` — they test other things) and
  still pass.

**TypeScript** (`bun run test`): 437 tests (up from 425). New:
`applyAnalysisEvent` (7 tests) covering scanned/batch/done handling,
accumulation across batches in arrival order, a zero-photo failed batch not
shifting a later batch's photos, and non-mutation of the input state;
`isRanked` (2 tests).

## Mutation evidence

**Streaming accumulation (Rust)** — mutated `analyze_batches_with_progress`
in `sidecar.rs` so `on_batch` was called with the cumulative `out` so far
instead of just the current batch's resolved records (`on_batch(&out)`
instead of `on_batch(&resolved)`):

```
test commands::tests::a_failed_batch_does_not_shift_or_corrupt_later_streamed_photos ... FAILED
test commands::tests::streamed_partial_photos_accumulate_in_input_order_across_batches ... FAILED
test sidecar::tests::a_failed_batch_does_not_shift_a_later_batchs_streamed_paths ... FAILED
test sidecar::tests::on_batch_fires_once_per_batch_with_that_batchs_own_records ... FAILED
test sidecar::tests::on_batch_records_concatenate_to_exactly_the_return_value ... FAILED
test result: FAILED. 85 passed; 5 failed
```

All 5 new streaming tests caught it. Reverted; back to 90/90 passing.

**Streaming accumulation (TypeScript)** — mutated `applyAnalysisEvent`'s
`batch` case to replace instead of append (`partialPhotos: [...event.photos]`
instead of `[...state.partialPhotos, ...event.photos]`):

```
× accumulates partial photos across batches in arrival order
× a failed (zero-photo) batch does not shift a later batch's photos
Tests  2 failed | 435 passed (437)
```

Both new tests caught it. Reverted; back to 437/437 passing.

**Positional correspondence (`finalize_photos`, pre-existing, per the brief's
explicit ask)** — mutated `finalize_photos` to compute the derived arrays
(`dup_ids`, `event_ids`, `aesthetic`, `sharpness`) *before* sorting `ok` by
path, then read them back post-sort by index (the exact historical bug class
these tests exist for):

```
test commands::tests::attributes_percentile_data_to_the_correct_photo_after_sorting ... FAILED
test commands::tests::attributes_event_cluster_to_the_correct_photo_after_sorting ... FAILED
test result: FAILED. 88 passed; 2 failed
```

Both pre-existing tests caught it, confirming they still guard the invariant
this feature depends on. Reverted; back to 90/90 passing.

## Verified live

No project-specific "run" skill exists for this app. GUI automation
(`osascript`/`System Events`, then `cliclick`) was attempted to drive the
native folder picker but blocked on missing Accessibility permission — the
same wall hit and correctly not forced through during the earlier
notification-banner work (see `PROJECT-STATUS.md`, "Verified vs
unverified" #7). `cliclick` was installed, tried once, produced no visible
effect, and was uninstalled again rather than escalating further against a
live, shared desktop.

Instead, `analyze_folder` was exercised directly and completely for real —
same command, same `Channel` mechanism, same real sidecar binary, real
photos — via a throwaway `cargo run --example` harness (built, run, and
deleted; not part of the diff) that logged every event's kind and elapsed
time. Against a 48-file folder (`sidecar/Fixtures/` copied 8x, all cache
misses, 3 sidecar batches of 16):

```
starting analyze_folder over .../streaming-test-photos
[692.000µs] scanned: total=48
[691.037ms] batch: +16 photos this batch, running analysed=16 cached=0 failed=0
[852.601ms] batch: +16 photos this batch, running analysed=32 cached=0 failed=0
[  1.032s] batch: +16 photos this batch, running analysed=48 cached=0 failed=0
[  1.040s] done: 48 final photos in summary
[  1.042s] analyze_folder returned: total=48 cached=0 failed=0 photos=48
```

Three distinct `batch` events arrive ~160-180ms apart over just over a
second of real wall-clock time, each carrying 16 real partial photos, with
`done` arriving last — direct, real-timing evidence that the UI would render
tiles progressively rather than waiting for one blocking result. This is the
same property `streaming_order.rs` now checks permanently in CI (against the
smaller, fast, always-available `sidecar/Fixtures/` set, so it isn't a slow
test).

Separately confirmed the built UI renders correctly under the new
composable/types with no runtime error: launched `bun run dev`, screenshotted
the real app window (entry state, "Choose a photo folder to begin"),
confirmed no crash and correct Tailwind/NuxtUI rendering. Did not capture the
mid-run/growing-grid screenshot specifically, since reaching it requires the
folder picker interaction that Accessibility permission blocks.

## Constraints checked

- macOS/arm64, `bun run sidecar` run before every `cargo` command.
- Conventional Commits used for the commit.
- No `--no-verify`.
- No `any` introduced in TypeScript.
- `bun run lint` (oxlint --deny-warnings) clean. One legitimate false-positive
  suppressed with a justified inline comment: `unicorn/prefer-add-event-listener`
  fires on `onEvent.onmessage = ...` in `useAnalysis.ts`, but Tauri's
  `Channel` is not a DOM `EventTarget` — it has no `addEventListener`, only
  the settable `onmessage` field the rule is warning against.
- `cargo clippy --all-targets` clean apart from the pre-existing `cluster.rs`
  `ptr_arg` warning (untouched by this change).
- No em dashes introduced (checked with `grep` across every changed file).
- `smileFraction` absent-vs-null handling preserved end-to-end, including in
  the new streaming path (`partial_photo` copies it only when the key is
  present in the source, tested explicitly).

## Uncertain / left as-is

- The `Batch` event's (`analysed`, `cached`, `failed`) counts are cumulative
  running totals as of that event, not per-batch deltas — chosen so the
  frontend never needs to sum anything to render "X / Y analyzed". Not
  explicitly specified either way in the brief; documented in both the Rust
  doc comment and the TS type.
- Cache hits stream as a single `Batch` event immediately after `lookup_cache`
  returns (before any sidecar work), rather than being chunked further. For a
  fully-cached re-run of a huge folder this means one large `Batch` payload
  instead of several smaller ones — still much better than the previous
  fully-blocking behaviour, and cache lookups are cheap (no decode), but a
  folder of many thousands of cache hits hasn't been measured for the size of
  that single IPC message.
- The mid-run/growing-grid state was not captured in a screenshot (see
  "Verified live" above) — only the entry-state screenshot and the timed
  event log substitute for it. If GUI verification is wanted, it needs either
  Accessibility permission granted to the terminal/agent, or a dev-only
  "auto-analyze this folder on launch" hook added and removed for the
  purpose (not added here, since it isn't part of the feature).
