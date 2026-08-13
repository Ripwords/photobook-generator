# Task 11 Report: SQLite feature cache

## What was implemented

- `src-tauri/src/db.rs`: `Db` struct wrapping a `rusqlite::Connection`, with:
  - `ANALYZER_VERSION: u32 = 1` module constant.
  - `Db::open(path: &Path)` / `Db::open_in_memory()` — both run `migrate()`, which creates the
    `features` table (`hash` PK, `path`, `json`, `analyzer_version`, `created_at`) and an index
    on `analyzer_version`, and sets `PRAGMA journal_mode = WAL`.
  - `Db::analyzer_version() -> u32` associated fn returning `ANALYZER_VERSION`.
  - `Db::get_features(&self, hash: &str) -> rusqlite::Result<Option<String>>` — `SELECT json ...
    WHERE hash = ?1 AND analyzer_version = ?2`, `.optional()`.
  - `Db::put_features(&self, hash, path, json) -> rusqlite::Result<()>` — `INSERT ... ON
    CONFLICT(hash) DO UPDATE SET path, json, analyzer_version = excluded.*`.
  - `Db::hashes_needing_analysis(&self, hashes: &[String]) -> rusqlite::Result<Vec<String>>` —
    filters the input to hashes where `get_features` returns `None`.
  - `conn` field is `pub(crate)` so the test module (in the same file) can issue a raw INSERT
    for the version-skew test.
- `src-tauri/src/lib.rs`: added `pub mod db;` (placed alphabetically before `protocol`/`sidecar`).
- Implementation code matches the brief's Step 3 verbatim, except for one addition (see
  Deviations).

## Commands and output

### Step 1/2 — write test module, confirm failure

Wrote `src-tauri/src/db.rs` containing only the `#[cfg(test)] mod tests { ... }` block from the
brief (verbatim), and added `pub mod db;` to `lib.rs` so the module would actually be compiled
(without that line the failure is "file not found for module" as the brief predicts; with it,
since the file exists but has no `Db` type yet, the failure is a compile error referencing the
missing symbols — same root cause, confirmed-failing either way).

Ran `bun run sidecar` first (per the brief's context note) — completed successfully, produced
`src-tauri/binaries/photobook-engine-aarch64-apple-darwin`.

```
$ cargo test --manifest-path src-tauri/Cargo.toml db::
error[E0425]: cannot find value `ANALYZER_VERSION` in this scope
  --> src/db.rs:32:62
error[E0433]: failed to resolve: use of undeclared type `Db`
  --> src/db.rs:7:18
  (+ 4 more identical E0433 `Db` errors, one per test)
error: could not compile `photobook-generator` (lib test) due to 6 previous errors; 1 warning emitted
```
Confirmed: fails before implementation exists.

### Step 3/4 — implement, confirm pass

Prepended the `Db` implementation above the test module (verbatim from the brief, plus the
`analyzer_version()` fn — see Deviations), then:

```
$ cargo test --manifest-path src-tauri/Cargo.toml db::
running 5 tests
test db::tests::reports_only_uncached_hashes_as_needing_analysis ... ok
test db::tests::returns_none_for_unknown_hash ... ok
test db::tests::overwrites_on_reanalysis_of_same_hash ... ok
test db::tests::round_trips_features ... ok
test db::tests::ignores_rows_from_an_older_analyzer_version ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s
```

Full suite (no regressions, sidecar test still passes):

```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 8 tests
test protocol::tests::deserializes_error_response ... ok
test protocol::tests::serializes_ping_request_as_single_line ... ok
test protocol::tests::deserializes_pong_response ... ok
test db::tests::returns_none_for_unknown_hash ... ok
test db::tests::ignores_rows_from_an_older_analyzer_version ... ok
test db::tests::overwrites_on_reanalysis_of_same_hash ... ok
test db::tests::round_trips_features ... ok
test db::tests::reports_only_uncached_hashes_as_needing_analysis ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 1 test (tests/sidecar_ping.rs)
test sidecar_binary_responds_to_ping_with_matching_id_and_version ... ok
```

`cargo build --manifest-path src-tauri/Cargo.toml --lib` — clean, no warnings (public API items,
including the unused-in-tests `analyzer_version()` fn, are not flagged as dead code since the
type is `pub`).

## Mutation-check evidence

Per the brief's explicit ask, verified `ignores_rows_from_an_older_analyzer_version` genuinely
depends on the `analyzer_version` filter in the read query, not just on the write path.

Backed up `db.rs` to the scratchpad, then removed the version predicate from `get_features`:

```rust
// mutated:
.query_row(
    "SELECT json FROM features WHERE hash = ?1",
    rusqlite::params![hash],
    |row| row.get(0),
)
```

```
$ cargo test --manifest-path src-tauri/Cargo.toml db::tests::ignores_rows_from_an_older_analyzer_version
test db::tests::ignores_rows_from_an_older_analyzer_version ... FAILED
thread '...' panicked at src/db.rs:112:9:
assertion failed: db.get_features("old").unwrap().is_none()
test result: FAILED. 0 passed; 1 failed
```

Confirmed the test fails without the filter — it is not vacuous. Restored the correct
implementation from the scratchpad backup and re-ran the full suite to confirm all 5 `db::`
tests (and the other 4 pre-existing tests) pass again — output shown above under Step 3/4.

I also reasoned through (without needing a live mutation, since the logic is structural) the
other tests:

- `overwrites_on_reanalysis_of_same_hash`: if the `ON CONFLICT(hash) DO UPDATE` clause were
  dropped (plain `INSERT`), the second `put_features("abc", ...)` call would hit the `hash`
  `PRIMARY KEY` constraint and return `Err`, so `.unwrap()` would panic — the test fails loudly
  under that mutation, not silently.
- `reports_only_uncached_hashes_as_needing_analysis`: if the implementation returned all input
  hashes unconditionally, `"cached"` would incorrectly appear in `need`, failing the
  `assert_eq!`. If it returned none, `"fresh"` would be missing. Either mutation is caught.
- `returns_none_for_unknown_hash` / `round_trips_features`: straightforward round-trip tests
  with no filter logic to game.

## Addendum: coordinator follow-up (post-review)

The coordinator reviewed the two concerns raised above and asked for both to be closed before
sending for review.

### Concern 1 closed — added version-skew coverage for `hashes_needing_analysis`

Added a new test, `reports_hash_cached_at_older_analyzer_version_as_needing_analysis`, to the
`tests` module in `src-tauri/src/db.rs`:

```rust
#[test]
fn reports_hash_cached_at_older_analyzer_version_as_needing_analysis() {
    let db = Db::open_in_memory().unwrap();
    db.conn
        .execute(
            "INSERT INTO features (hash, path, json, analyzer_version) VALUES (?1,?2,?3,?4)",
            rusqlite::params!["old", "/tmp/o.jpg", "{}", ANALYZER_VERSION - 1],
        )
        .unwrap();
    let need = db.hashes_needing_analysis(&["old".to_string()]).unwrap();
    assert_eq!(need, vec!["old".to_string()]);
}
```

Ran the full `db::` suite — all 6 pass:

```
$ cargo test --manifest-path src-tauri/Cargo.toml db::
running 6 tests
test db::tests::reports_hash_cached_at_older_analyzer_version_as_needing_analysis ... ok
test db::tests::ignores_rows_from_an_older_analyzer_version ... ok
test db::tests::overwrites_on_reanalysis_of_same_hash ... ok
test db::tests::round_trips_features ... ok
test db::tests::returns_none_for_unknown_hash ... ok
test db::tests::reports_only_uncached_hashes_as_needing_analysis ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s
```

**Mutation check** — replaced `hashes_needing_analysis`'s body with an alternate implementation
that queries the `features` table for hash presence directly, without the `analyzer_version`
predicate (the exact failure mode the coordinator described):

```rust
pub fn hashes_needing_analysis(&self, hashes: &[String]) -> rusqlite::Result<Vec<String>> {
    let mut needed = Vec::new();
    for hash in hashes {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM features WHERE hash = ?1",
                rusqlite::params![hash],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if !exists {
            needed.push(hash.clone());
        }
    }
    Ok(needed)
}
```

```
$ cargo test --manifest-path src-tauri/Cargo.toml db::tests::reports_hash_cached_at_older_analyzer_version_as_needing_analysis
test db::tests::reports_hash_cached_at_older_analyzer_version_as_needing_analysis ... FAILED
thread '...' panicked at src/db.rs:140:9:
assertion `left == right` failed
  left: []
 right: ["old"]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.00s
```

Confirmed the new test genuinely catches the version-blind mutation. Restored the correct
(delegating-to-`get_features`) implementation from a scratchpad backup and re-ran the full suite:

```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 9 tests
test protocol::tests::deserializes_pong_response ... ok
test protocol::tests::deserializes_error_response ... ok
test protocol::tests::serializes_ping_request_as_single_line ... ok
test db::tests::returns_none_for_unknown_hash ... ok
test db::tests::reports_hash_cached_at_older_analyzer_version_as_needing_analysis ... ok
test db::tests::reports_only_uncached_hashes_as_needing_analysis ... ok
test db::tests::round_trips_features ... ok
test db::tests::ignores_rows_from_an_older_analyzer_version ... ok
test db::tests::overwrites_on_reanalysis_of_same_hash ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
(plus `sidecar_ping` 1/1 and doc-tests 0/0, unchanged.)

`cargo build --manifest-path src-tauri/Cargo.toml --lib` — clean, no warnings.

### Concern 2 closed — removed `Db::analyzer_version()`

Deleted the unused associated function; only the module-level `pub const ANALYZER_VERSION: u32`
remains as the single source of truth, as Task 15 was always going to reference it directly.

### Commit

`git add src-tauri/src/db.rs && git commit -m "test(db): cover version-skew in hashes_needing_analysis; drop unused accessor"`

## Deviations from the brief

1. ~~Added `pub fn analyzer_version() -> u32`~~ — added, then removed per coordinator follow-up
   (see Addendum). Final state has no such method; `ANALYZER_VERSION` is the sole accessor, as
   the coordinator confirmed the Interfaces-section listing was their own error.
2. Everything else (schema, queries, upsert, function signatures used by Task 15) matches the
   brief's Step 3 code verbatim. `hashes_needing_analysis` and `put_features` signatures are
   unchanged from the brief, as instructed. The `tests` module gained one test beyond the brief's
   verbatim list, added at the coordinator's explicit request post-review.

## Uncertain / worth a second look

- The scratchpad mutation-test workflow copied `db.rs` to `/tmp` briefly before moving the
  backup to the correct scratchpad directory (per the harness's scratchpad convention) — no
  temp file was left in `/tmp` afterward, but noting it since the environment instructions ask
  for scratchpad-only temp files.
- No filesystem-backed (`Db::open(path)`) test exists — only `open_in_memory()` is exercised, as
  specified by the brief. `open()` shares the same `migrate()` path so this is low risk, but it
  is technically untested against a real file.
