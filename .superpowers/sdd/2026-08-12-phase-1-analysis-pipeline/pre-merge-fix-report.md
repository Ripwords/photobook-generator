# Pre-merge fix report

**Branch:** `feat/phase-1-analysis-pipeline`
**Status:** All 4 fixes applied, tested, and committed. Tree is clean.

## Commits

1. `7fb5394` fix(db): bump ANALYZER_VERSION for embedded-preview decode change
2. `d89b3a0` fix(analysis): use invoke's return value as the authoritative summary
   (also fixes the vacuous `does not mutate the state object` test + freezes
   `initialStreamState`)
3. `106fd34` fix(scripts): make benchmark.sh work on stock macOS bash
4. `226a9d0` docs(status): record ImageLoader double-decode and dead benchmark path

## Fix 1 — `ANALYZER_VERSION`

Bumped `src-tauri/src/db.rs:6` from `1` to `2`, with a comment recording *why*
(embedded-preview decode path change from `a9121da`) as a worked example for
future bumps. Confirmed no test hardcodes `ANALYZER_VERSION`'s literal value —
`ignores_rows_from_an_older_analyzer_version` uses `ANALYZER_VERSION - 1`, and
`cache_hit.rs` re-analyses within the same process so both runs use the same
(now v2) constant. Cache-hit assertions still pass.

## Fix 2 — `Done` delivery / vacuous test

`useAnalysis.ts` now applies `{ kind: "done", summary }` from `invoke`'s
return value (authoritative), in addition to whatever the `Done` channel
event delivers (now purely an optimisation). `applyAnalysisEvent`'s `done`
case only assigns `state.summary`, so applying it twice was already
idempotent — added a regression test (`applying done twice is idempotent`)
pinning that.

Fixed the vacuous `does not mutate the state object passed in` test
(`structuredClone` before the call instead of comparing `initialStreamState`
to itself) and froze `initialStreamState` with `Object.freeze` in
`features.ts`. **Mutation-verified**: temporarily made the `scanned` reducer
branch mutate `state` in place (with freeze removed to isolate the check) —
the fixed test failed with the expected diff (`scannedTotal: 280` vs `5`,
leaked from an earlier test sharing the same frozen object); reverted
cleanly via a scratchpad backup, confirmed identical to original.

## Fix 3 — `benchmark.sh`

Replaced `mapfile -t` with a `while IFS= read -r` loop, verified with
`bash -n` under the actual stock `/bin/bash` (3.2.57 on this machine). Made
it version-independent rather than adding a README caveat, per the brief's
"better" option.

## Not-now items recorded

Added both deferred findings to `docs/PROJECT-STATUS.md`'s "Known smaller
debts": `ImageLoader`'s double-decode for sub-floor sources (with the
suggested guard), and `Sidecar::benchmark`/`ResponseResult::Benchmarked`
being dead code since `scripts/benchmark.sh` bypasses Rust entirely. No code
changes made for either, as instructed.

## Test summary

- **Rust** (`bun run sidecar` run first): 100 unit tests + 1 (`sidecar_ping`)
  + 3 standalone integration binaries (`cache_hit`, `sidecar_worker_pool`,
  `streaming_order`) — all green. `cargo clippy --all-targets` clean except
  the known pre-existing `cluster.rs` `ptr_arg` warning.
- **TypeScript**: `bun run test` — 438 passed (3 files). `bun run lint`
  (oxlint --deny-warnings) clean (one `no-shadow` warning surfaced and was
  fixed by renaming a local variable).
- **Swift**: `bun run test:swift` — 87 tests, 1 suite, all passed.

## Concerns

- None blocking. One pre-existing, unrelated flake was observed during the
  Swift run: `hostileBatchMixedWithGoodPhotoStillReturnsTheGoodOne` logged a
  thumbnail-write `NSCocoaErrorDomain Code=516` (file already exists) from a
  shared temp filename in that test fixture, but the test still passed. Not
  touched — out of scope for this pass, but worth a look if it ever flips to
  a real failure.
- A stray `bun run dev` / `tauri dev` pair was observed running briefly
  during this session; it was not started by me and had already exited on
  its own before any action was needed — confirmed no process on port 3000
  and no dev processes remain.
