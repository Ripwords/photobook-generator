# Task 12 Report: Near-duplicate and event clustering

## Addendum: coordinator review fixes (round 3)

Round 2's `tied_timestamps_do_not_split` claimed in its comment to exercise the strict `>` split condition, but its input (`Some(5), Some(5)`, gap of `0` against `gap_seconds = 86_400`) is nowhere near the threshold — both `>` and `>=` evaluate `false` there, so the test cannot distinguish the two operators. The coordinator's reviewer verified this empirically by flipping `>` to `>=` and reran the whole suite: all 12 tests (including `tied_timestamps_do_not_split`) still passed, proving the comment's claim false.

**Fix:** kept `tied_timestamps_do_not_split` (identical timestamps not splitting is still worth asserting on its own) but corrected its comment to stop claiming operator coverage, and added a new test that actually sits on the boundary:

```rust
#[test]
fn gap_exactly_at_threshold_does_not_split() {
    // gap == gap_seconds exactly: false under `>` (no split, correct),
    // true under `>=` (would incorrectly split — the regression this pins).
    let ids = event_clusters(&[Some(0), Some(86_400)], 86_400);
    assert_eq!(ids[0], ids[1]);
}
```

**Mutation-check:** flipped `time - prev > gap_seconds` to `>=` in `event_clusters` and reran:
```
$ cargo test --manifest-path src-tauri/Cargo.toml cluster
test cluster::tests::gap_exactly_at_threshold_does_not_split ... FAILED
  left: 0
 right: 1
test cluster::tests::tied_timestamps_do_not_split ... ok
test result: FAILED. 12 passed; 1 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s
```
`gap_exactly_at_threshold_does_not_split` correctly fails under `>=` (id 1 instead of expected 0 — the pair split when it shouldn't have), while `tied_timestamps_do_not_split` still passes under either operator, confirming the boundary test — not the tied-timestamps test — is the one that actually pins the strict-`>` contract. Reverted `>=` back to `>` and reconfirmed full suite green:
```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 22 tests (src/lib.rs) ... all ok, including 13 cluster::tests::*
running 1 test (tests/sidecar_ping.rs) ... ok
```

Commit: `a8e65dd` — `test(cluster): pin the strict gap-threshold boundary in event_clusters`

---

## Addendum: coordinator review fixes (round 2)

The coordinator's independent review confirmed the input-order vacuity finding from round 1 and additionally caught a real bug I missed:

**Bug: `event_clusters` broke its own dense-id property when every timestamp is `None`.** `current` starts at `0` and only advances when a gap is found between *dated* entries. With zero dated entries, `current` never moves, but the old code unconditionally set `undated = current + 1 = 1`. Every element then got id `1`, and id `0` was never assigned to anything — a phantom empty chapter for Task 15's consumers whenever a photo set has no EXIF dates at all (stripped metadata, screenshots, downloads).

**Fix applied** in `src-tauri/src/cluster.rs`'s `event_clusters`: added an `any_dated` flag, set `true` only when the dated-entries loop actually runs. `undated` is now `current + 1` when `any_dated` is true (unchanged behavior for the mixed/all-dated cases) and `0` when `any_dated` is false (fixes the all-nil case):
```rust
let undated = if any_dated { current + 1 } else { 0 };
```

**New tests added**, per the coordinator's spec:
- `all_nil_timestamps_still_get_dense_ids_starting_at_zero` — asserts all three ids are equal *and* equal to `0` (not just equal to each other, which would pass under the bug).
- `single_dated_element_yields_one_id` — `event_clusters(&[Some(1_000)], 86_400) == vec![0]`.
- `single_undated_element_yields_one_id` — `event_clusters(&[None], 86_400) == vec![0]`.
- `tied_timestamps_do_not_split` — `event_clusters(&[Some(5), Some(5)], 86_400)` keeps both in the same cluster (gap of `0` is not `> gap_seconds`).

**Mutation-check evidence (pins the bug):** reverted `event_clusters` to the pre-fix logic (`let undated = current + 1;` unconditionally, no `any_dated` guard) and reran:
```
$ cargo test --manifest-path src-tauri/Cargo.toml cluster
test cluster::tests::all_nil_timestamps_still_get_dense_ids_starting_at_zero ... FAILED
  left: 1
 right: 0
test cluster::tests::single_undated_element_yields_one_id ... FAILED
  left: [1]
 right: [0]
test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s
```
Both `all_nil_timestamps_still_get_dense_ids_starting_at_zero` and `single_undated_element_yields_one_id` correctly fail against the pre-fix implementation — confirmed they pin the bug rather than merely describing the fix. Restored the fixed implementation afterward.

**Full suite after fix + new tests:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 21 tests (src/lib.rs) ... all ok, including 12 cluster::tests::*
running 1 test (tests/sidecar_ping.rs) ... ok
running 0 tests (doc-tests) ... ok
```

**Not fixed, per coordinator instruction:** the unguarded `i64` subtraction in the gap check (`time - prev`) and the absence of union-by-rank in `near_duplicate_clusters` were flagged by the reviewer but explicitly left as-is — both are fine at this scale with EXIF-derived timestamps.

Commit: `e5a907a` — `fix(cluster): assign undated group id 0 when no photos have timestamps`

---

## What was implemented (round 1)

- Created `src-tauri/src/cluster.rs` with:
  - `pub fn hamming(a: u64, b: u64) -> u32` — popcount of XOR.
  - `pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize>` — single-link agglomerative clustering via union-find (path-compressing `find`, union by direct assignment). O(n²) pairwise comparison, acceptable at the few-hundred-element scale used by Task 15. Cluster ids are compacted to `0..k` via a `HashMap<root, label>` in first-encountered order.
  - `pub fn event_clusters(timestamps: &[Option<i64>], gap_seconds: i64) -> Vec<usize>` — sorts `(index, timestamp)` pairs internally by timestamp, walks the sorted sequence incrementing a cluster counter wherever the gap exceeds `gap_seconds`, then writes each result back to `ids[index]` (its *original* position) so the returned vector matches input order. Undated photos are collected into one trailing cluster (`current_max + 1`) after the pass.
- Added `pub mod cluster;` to `src-tauri/src/lib.rs` (alphabetically ordered before `pub mod db;`). Did not touch `db.rs` or create `ranking.rs` (Task 13's module).
- Implemented exactly the 7 tests given in the brief, verbatim, plus one additional test (see "Deviation" below).

## Commands run, in order, with actual output

**1. Confirm branch/state:**
```
$ git status --short && git branch --show-current
feat/phase-1-analysis-pipeline
```
(clean tree, correct branch, as expected before starting)

**2. Step 1 — wrote `cluster.rs` with only the `#[cfg(test)]` module (no implementation yet).**

**3. Ran tests without `pub mod cluster;` wired into `lib.rs` yet** (sanity check before Step 2's prescribed run):
```
$ cargo test --manifest-path src-tauri/Cargo.toml cluster
running 0 tests
test result: ok. 0 passed; 0 failed; ...
```
Note: with the module unregistered, cargo silently doesn't compile `cluster.rs` at all — no error, just 0 tests. This does not match the brief's predicted failure text ("file not found for module 'cluster'"); see Deviation #1 below.

**4. Added `pub mod cluster;` to `lib.rs`, reran (this is the brief's actual Step 2 failure check):**
```
$ cargo test --manifest-path src-tauri/Cargo.toml cluster
error[E0425]: cannot find function `hamming` in this scope
error[E0425]: cannot find function `near_duplicate_clusters` in this scope
error[E0425]: cannot find function `event_clusters` in this scope
... (10 errors total)
error: could not compile `photobook-generator` (lib test) due to 10 previous errors
```
Confirmed failing as expected (compile error, since the test module references undefined functions).

**5. Step 3 — implemented `hamming`, `near_duplicate_clusters`, `event_clusters` above the test module (verbatim from brief).**

**6. Step 4 — reran:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml cluster
running 7 tests
test cluster::tests::empty_input_returns_empty ... ok
test cluster::tests::hamming_counts_differing_bits ... ok
test cluster::tests::clustering_is_transitive_within_threshold ... ok
test cluster::tests::identical_hashes_land_in_one_cluster ... ok
test cluster::tests::events_split_on_a_large_time_gap ... ok
test cluster::tests::photos_without_timestamps_get_their_own_cluster ... ok
test cluster::tests::distant_hashes_land_in_separate_clusters ... ok
test result: ok. 7 passed; 0 failed; ...
```
All 7 pass. Full workspace suite also green (16 lib tests + 1 integration test at that point); no sidecar-path build error was hit.

## Mutation-check evidence

### Check A: transitivity test vs. a naive (non-union-find) implementation

First attempt at a "naive" mutation (seed-and-propagate, no `find()`/path compression) turned out to still be transitively correct for a 3-node path graph by construction, so it did *not* kill the test — a useful negative result showing that a merely different-looking algorithm isn't automatically "not union-find" in effect. I discarded that attempt and used a genuinely broken one: link each match directly to `i`'s raw index (not `i`'s resolved cluster root), so a later merge of `b`-`c` doesn't fold `c` into `a`'s group:

```rust
pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize> {
    let n = phashes.len();
    let mut ids: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(phashes[i], phashes[j]) <= max_distance {
                ids[j] = i;
            }
        }
    }
    ids
}
```

Result:
```
test cluster::tests::clustering_is_transitive_within_threshold ... FAILED
  left: 0
 right: 1
test cluster::tests::identical_hashes_land_in_one_cluster ... FAILED
test result: FAILED. 5 passed; 2 failed; ...
```
`clustering_is_transitive_within_threshold` correctly dies under this mutation. Restored the real union-find implementation and reconfirmed 7/7 pass.

### Check B: input-order test vs. a sorted-order-return mutation

Mutated `event_clusters` to build the result vector in sorted-timestamp order and append undated entries at the end, without writing back to original indices:

```rust
let mut ids = Vec::with_capacity(timestamps.len());
// ... push in sorted-time order instead of ids[index] = current ...
```

Result under this mutation:
```
test cluster::tests::event_clusters_returns_ids_in_input_order_not_sorted_order ... FAILED
  left: 0
 right: 1
test cluster::tests::photos_without_timestamps_get_their_own_cluster ... FAILED
  left: 0
 right: 1
test cluster::tests::events_split_on_a_large_time_gap ... ok   <-- still passes!
test result: FAILED. 6 passed; 2 failed; ...
```

This confirms the concern flagged in the task instructions: **the brief's own `events_split_on_a_large_time_gap` test is vacuous for the input-order property.** Its input (`[Some(0), Some(60), Some(3*day), Some(3*day+60)]`) is already in ascending timestamp order, so sorted-order output happens to equal input-order output — the test cannot distinguish a correct implementation from one that silently returns sorted order. It survives the mutation above unchanged.

`photos_without_timestamps_get_their_own_cluster` *does* happen to catch this particular mutation, but only incidentally — its `None` sits at input index 1 (the middle), so appending undated entries at the end shifts everything after it by one position, and the test's positional assertions catch the shift. It is not a reliable, general test of "returns ids in input order"; a mutation that reordered only the dated entries (no undated photos involved) would not necessarily trip it.

**Deviation from brief:** I added one test not in the brief's Step 1 code block, `event_clusters_returns_ids_in_input_order_not_sorted_order`, using genuinely out-of-order input (index 0 = latest timestamp, index 3 = second-latest) so the sorted-order mutation is caught directly and non-vacuously, per the task instructions' explicit requirement ("Constructing input that is already sorted would make this vacuous — make sure it is not"). Verified it passes against the correct implementation and fails against the mutation (shown above). Restored the correct implementation afterward and reran the full suite: 8 cluster tests pass, 17 lib tests total, 1 integration test — all green.

## Final verification

```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 17 tests (src/lib.rs)  ... all ok, including 8 cluster::tests::*
running 1 test (tests/sidecar_ping.rs) ... ok
running 0 tests (doc-tests) ... ok
```

```
$ cargo build --manifest-path src-tauri/Cargo.toml
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
```
No warnings (the earlier `unused import: super::*` warning, seen only while the test module had no implementation to reference, is gone once the functions exist and are used).

```
$ git status --short
 M src-tauri/src/lib.rs
?? src-tauri/src/cluster.rs
```
(before commit; clean after)

## Commit

```
06d8e99 feat(cluster): add near-duplicate and event clustering
 2 files changed, 154 insertions(+)
 create mode 100644 src-tauri/src/cluster.rs
```

## Deviations and why

1. **Predicted failure text didn't match reality.** The brief's Step 2 expects `FAIL — file not found for module 'cluster'`. In practice, since `cluster.rs` already existed on disk (created in Step 1) but `lib.rs` didn't yet declare `pub mod cluster;`, cargo simply doesn't compile the file — no error, `0 tests` reported, build succeeds. The actual "confirm it fails" moment only occurs once `pub mod cluster;` is added (Step 3's first half, done here ahead of the rest of Step 3 to get a real red state), at which point the failure is a set of `E0425: cannot find function` compile errors, not a "file not found" error. This is a copy-paste artifact in the brief's expected output, not a functional problem — the TDD intent (see it fail, then make it pass) was preserved.
2. **Added an 8th test** (`event_clusters_returns_ids_in_input_order_not_sorted_order`) beyond the brief's literal Step 1 code, because the brief's own test for that subtlety is vacuous (see Check B above). This was requested explicitly by the task instructions' mutation-check requirement, not something I did silently — flagged here as instructed.

No other deviations. `db.rs` and `ranking.rs` were not touched. No I/O, no dependencies beyond `std`. Did not need to run `bun run sidecar` — the sidecar binary was already present from earlier tasks and the build succeeded without it.

## Uncertain / worth a second look

- The union-find `near_duplicate_clusters` uses a *global* mutable `parent` vec captured by a nested `fn find` that takes `&mut Vec<usize>` as an explicit parameter (not a closure) — this is what the brief specified verbatim, and it works, but it's a slightly unusual style (nested fn instead of closure) worth noting only because it's easy to misread as a closure at a glance.
- `near_duplicate_clusters`'s cluster-id compaction order depends on `HashMap` iteration order of *insertion into `entry()`*, which in this code is actually deterministic (driven by the `0..n` map order, not by hash iteration) — ids are assigned in first-encountered index order, not hash-randomized order. Confirmed this by reading the code path: `labels.entry(root).or_insert(next)` is called in a deterministic `(0..n).map(...)` sequence, so this is fine, just flagging that "HashMap" in the type doesn't imply nondeterminism here.
