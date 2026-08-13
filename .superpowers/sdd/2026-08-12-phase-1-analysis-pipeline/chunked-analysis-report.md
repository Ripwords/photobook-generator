# Chunked incremental analysis — "0 / 283 frozen for 14s" fix

Branch: `feat/phase-1-analysis-pipeline`

## The bug (as diagnosed, not re-derived)

`analyze_folder` called `lookup_cache(&db, &paths)` once, over the WHOLE
folder, before the first sidecar batch or the first UI update ever
happened. `lookup_cache` hashes every file (SHA-256 over raw bytes) to
consult the cache. On a 283-photo folder of 24MB Sony ARW files that's
~6.8GB of hashing at ~490MB/s measured throughput — ~14s minimum — during
which the `Scanned` event had already fired with the total, so the UI sat
at "0 / 283" with a determinate-looking but frozen bar. Analysis itself is
fast (89ms/photo per `scripts/benchmark.sh`); the stall is entirely the
upfront hash pass.

## The fix

Replaced the two-phase pipeline (hash everything → cache-lookup everything
→ dispatch everything to the sidecar) with one chunked pipeline that does
hash → cache-lookup → sidecar-dispatch → stream, **per chunk**, before
moving to the next chunk. Chunk sizes ramp from 4 up to
`sidecar::BATCH_SIZE` (16): `4, 8, 16, 16, 16, ...`, so the first progress
event only waits on hashing 4 files instead of the whole folder, and later
chunks amortise sidecar round-trips at the same size the sidecar already
uses internally.

### New code

- **`sidecar::ramp_chunk_sizes(total) -> Vec<usize>`** (`src-tauri/src/sidecar.rs`) —
  pure, named, tested. Produces `[4, 8, 16, 16, ..., remainder]`.
- **`sidecar::chunk_paths_ramped(paths) -> Vec<Vec<String>>`** — splits a
  path list by that ramp, distinct from the existing `chunk_paths` (which
  uniformly chunks the sidecar's OWN cache-miss batches at a fixed size).
- **`commands::gather_chunked(db, paths, analyze, on_progress) -> GatherResult`**
  (`src-tauri/src/commands.rs`) — the pure per-chunk pipeline: for each
  ramped chunk, `lookup_cache` (hash + cache lookup) → stream cache hits →
  hand misses to the pluggable `analyze` closure → stream sidecar results.
  Pluggable `analyze` (mirroring `sidecar::analyze_batches_with_progress`'s
  `call`) makes this unit-testable without a live `AppHandle`/sidecar.
  Returns `GatherResult { ok, cached, failed }` — the gathered records in
  chunk-arrival order, NOT sorted or derived.
- **`analyze_folder`** now wraps the ENTIRE gather (hashing included, not
  just the sidecar calls) in one `spawn_blocking`, calls `gather_chunked`,
  then calls `finalize_photos` exactly once, at the end, over the complete
  `ok` — unchanged from before. Percentiles/clusters are still computed
  once over the whole set; only the *gathering* is incremental. Moving the
  hashing into `spawn_blocking` too is an added correctness fix: it was
  previously blocking synchronous I/O running inline on the tokio async
  worker pool.
- **Frontend label**: `app/pages/index.vue`'s running-state copy changed
  from "`X / Y analyzed`" to "`X / Y processed`" — cache hits and hash
  failures aren't "analyzed" this run, and `processed` (already computed
  as `analysed + cached + failed`) is what the counter actually shows.

### Invariants preserved (verified, not assumed)

- `finalize_photos` (sort → derive `percentiles`/clusters) still runs
  exactly once, over the complete set, unchanged.
- Exactly one record per input path: `gather_chunked`'s
  `result.ok.len() + result.failed == input.len()` is a dedicated test.
- Cache-before-sidecar ordering within each chunk (unchanged from
  `lookup_cache`'s existing contract, just called once per chunk now).
- `AnalysisEvent`/`StreamState` streaming contract (partial records carry
  no rank; `PhotoTile`'s "unranked" state) — untouched, and visually
  confirmed live (see screenshots below).

## Tests added

`src-tauri/src/sidecar.rs`:
- `ramp_starts_at_four_doubles_to_batch_size_then_holds`
- `ramp_covers_every_path_exactly_once_in_order` (sums to total, several `n`)
- `ramp_of_zero_is_empty`
- `ramp_never_exceeds_batch_size_per_chunk`
- `ramp_final_chunk_is_the_remainder_even_if_smaller_than_the_target`
- `chunk_paths_ramped_reconstructs_the_original_paths_in_order`
- `chunk_paths_ramped_of_empty_input_is_empty`

`src-tauri/src/commands.rs`:
- `gather_chunked_reports_progress_for_a_chunk_that_is_entirely_cache_hits` —
  the brief's explicit "cache-hit-only chunk still emits progress" case.
- `gather_chunked_counts_reconcile_against_the_input` — hits+misses(ok)+
  failures reconcile against the input count, across a mixed chunk.
- `positional_correspondence_survives_chunked_gather_into_finalize_photos` —
  5 paths handed to `gather_chunked` in **descending** order (opposite of
  the ascending order `finalize_photos` sorts into), split across **two**
  ramp chunks `[4, 1]`, mixing cache hits and sidecar misses in each chunk.
  Asserts `aestheticPct`/`eventCluster` attach to the path that OWNS that
  data, not to gather/chunk position.

Full suite: `cargo test` → **100 lib tests + 4 integration binaries, all
green**. `cargo clippy --all-targets` → clean except the known
pre-existing `cluster.rs` `ptr_arg` warning (unrelated, pre-dates this
change). `bun run test` → **437 passed**. `bun run lint` → clean
(`oxlint --deny-warnings`, zero output). `bun run test:swift` → **87
passed** (untouched by this change; ran as a sanity check since the
sidecar binary was rebuilt).

## Mutation evidence for the positional-correspondence test

Per the task's instruction to mutation-check this specific test, not just
write it: temporarily replaced `finalize_photos`'s `ok.sort_by(...)` line
with a no-op comment, ran `cargo test --lib`, then reverted.

**Before revert (mutated), `cargo test --lib` output:**

```
---- commands::tests::attributes_percentile_data_to_the_correct_photo_after_sorting stdout ----
thread '...' panicked at src/commands.rs:1138:9:
assertion `left == right` failed
  left: String("/p/b.jpg")
 right: "/p/a.jpg"

---- commands::tests::attributes_event_cluster_to_the_correct_photo_after_sorting stdout ----
thread '...' panicked at src/commands.rs:1163:9:
assertion `left == right` failed
  left: String("/p/late.jpg")
 right: "/p/early.jpg"

---- commands::tests::sorts_output_by_path_regardless_of_input_order stdout ----
thread '...' panicked at src/commands.rs:1112:9:
assertion `left == right` failed
  left: String("/p/b.jpg")
 right: "/p/a.jpg"

---- commands::tests::positional_correspondence_survives_chunked_gather_into_finalize_photos stdout ----
thread '...' panicked at src/commands.rs:1037:9:
assertion `left == right` failed: finalize_photos must sort into ascending
path order regardless of gather/chunk order
  left: [".../gather-positional-e.jpg", ".../gather-positional-d.jpg",
         ".../gather-positional-c.jpg", ".../gather-positional-b.jpg",
         ".../gather-positional-a.jpg"]
 right: [".../gather-positional-a.jpg", ".../gather-positional-b.jpg",
         ".../gather-positional-c.jpg", ".../gather-positional-d.jpg",
         ".../gather-positional-e.jpg"]

test result: FAILED. 96 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out
```

All four sort/positional tests fail, including the new chunked-gather one
— confirming it is genuinely sensitive to the exact bug class named in the
brief ("a chunk's records appended out of order"), not a rubber-stamp.
After reverting the mutation, `cargo test` returned to 100/100 passing
(shown above).

## Timings, before and after, on the real folder

Real folder: `/Users/jiajingteoh/Downloads/test`, 9 real Sony ARW files
(~24MB each, ~224MB total) — the only real ARW folder available in this
environment (the user's reported 283-photo folder is not present here).
Measured with a temporary `harness = false` integration test
(`src-tauri/tests/real_folder_timing.rs`, deleted before commit — see
"Verification scaffolding" below) that drives the real `analyze_folder`
against a live `AppHandle` and the real sidecar, with a **cold cache**
(fresh throwaway `HOME` per run) both times, printing a timestamp for
every streamed event.

**Before (stashed back to the pre-fix `commands.rs`/`sidecar.rs` via `git
stash`, `bun run sidecar && cargo test --test real_folder_timing`):**

```
[  0.001s] scanned total=9
[  6.860s] batch photos=9 analysed=9 cached=0 failed=0
[  6.863s] done
SUMMARY ... batches=1 first_batch_at=6.860499375 total_elapsed=6.863s
```

One batch, covering all 9 photos at once — nothing renders until 6.86s in.

**After (this change, same command, cold cache):**

```
[  0.001s] scanned total=9
[  3.197s] batch photos=4 analysed=4 cached=0 failed=0
[  6.882s] batch photos=5 analysed=9 cached=0 failed=0
[  6.883s] done
SUMMARY ... batches=2 first_batch_at=3.1973435 total_elapsed=6.883s
```

**Time-to-first-photo: 6.86s → 3.20s (a 53% reduction)** on this 9-file
folder, with total completion time unchanged (6.863s → 6.883s, ±20ms,
noise). The fix moves work earlier; it does not add work.

This folder is far too small to reproduce the reported 14-second freeze
(9 files ≈ 216MB hashes in well under a second at 490MB/s — hashing was
never the bottleneck for 9 files). The mechanism is what matters and is
directly observable in the batch counts: **before**, exactly one batch
covering the whole folder (proving the old code gathered everything, then
streamed once); **after**, two batches following the `[4, 5]` ramp shape
exactly (4 in the first chunk, the remaining 5 — the ramp's next step
would be 8, but only 5 paths were left — in the second). For a 283-photo
folder the ramp would produce ~19 chunks (`4, 8, 16×16, 15`), so the first
tile would appear after hashing 4 files (a few hundred ms) instead of all
283 (~14s), which is the actual shape of the reported bug.

**Side finding, not in scope to fix here:** the full-pipeline per-photo
cost observed above (~760ms/photo average across both runs) is well above
the isolated `scripts/benchmark.sh` figure (89ms/photo, decode 21ms +
Vision 39ms). The gap is not explained by this change — chunking doesn't
touch per-photo analysis cost, and the "before" run (one monolithic
sidecar batch, no chunking at all) shows the same per-photo cost. Likely
candidates: sidecar cold-start/model-load overhead attributed to the
first request, and/or full RAW file I/O and thumbnail writing that
`scripts/benchmark.sh`'s isolated per-stage timing may not fully capture.
Worth measuring separately; flagged here rather than silently absorbed
into this task's numbers.

## Verified in the running app

Folder picker cannot be clicked in this sandbox (no Accessibility
permission). Used the sanctioned technique from `streaming-report.md`: a
temporary `onMounted` hook in `app/pages/index.vue` set `folder.value` to
the real folder and called the composable's own `retry()` (no new code
path), `bun run dev` against a **cold cache** (deleted
`~/Library/Application Support/com.jiajingteoh.photobook/photobook.sqlite`
first), window-ID-scoped `screencapture -l<id>` (resolved via a small
Swift `CGWindowListCopyWindowInfo` snippet, no Accessibility APIs — same
technique as `task-15-report.md`), burst-captured every 0.5s from launch.

**Mid-run** (`chunked-analysis-mid-run.png`): captured ~1–1.5s after
launch, shows **"4 / 9 processed in test"** (confirming the label fix:
says "processed", not "analyzed"), 4 real decoded tiles rendered with an
"unranked" badge (the pre-existing streaming-design invariant — partial
records carry no rank), and 5 skeleton placeholders for the rest — the
grid visibly growing before the run finishes.

**Final** (`chunked-analysis-final-results.png`): "9 scanned / 9 analyzed
/ 0 from cache / 0 failed / 8 keepers", all 9 tiles ranked with real
aesthetic/sharpness percentiles, correct hero/burst badges — `Done`
correctly merged ranks into the tiles already streamed, exactly as
before.

Both screenshots saved alongside this report (git-ignored, local only,
same as the rest of `.superpowers/sdd/`).

**Cleanup after verification:** killed the dev server and the spawned
`PhotobookGen`/`cargo`/`nuxt` processes (`pkill -f "target/debug/
PhotobookGen"`, `pkill -f "cargo  run --no-default-features"`, `pkill -f
"nuxt dev"`; confirmed via `ps aux` that nothing was left running before
finishing). No leftover `bun run dev` on port 3000.

**Verification scaffolding, all reverted before commit:**
- `onMounted` hook in `app/pages/index.vue` — removed; `git diff` shows
  only the intended label-copy change.
- `src-tauri/tests/real_folder_timing.rs` and its `[[test]]` entry in
  `src-tauri/Cargo.toml` — deleted; `git status` confirms neither is
  present.
- `git stash`/`git stash pop` used to get a clean "before" baseline for
  timing, applied only to `src-tauri/src/commands.rs` and
  `src-tauri/src/sidecar.rs`; popped immediately after the "before" run
  and confirmed restored via `git status`/re-running the full test suite.

`git status --short` at the end of this task shows exactly three modified
files: `app/pages/index.vue`, `src-tauri/src/commands.rs`,
`src-tauri/src/sidecar.rs`. Nothing else.

## Commands run (verbatim, condensed)

```
bun run sidecar
cd src-tauri && cargo build                    # clean
cargo test                                      # 100 lib + 4 integration binaries, all green
cargo clippy --all-targets                      # clean apart from known cluster.rs warning
cd .. && bun run test                           # 437 passed
bun run lint                                    # oxlint --deny-warnings, clean
bun run test:swift                              # 87 passed (sanity check, untouched code)
```
