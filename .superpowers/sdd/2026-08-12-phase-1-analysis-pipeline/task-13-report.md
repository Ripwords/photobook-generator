# Task 13 report: within-book percentile ranking

## Implementation

`src-tauri/src/ranking.rs` (new module), registered via `pub mod ranking;` in
`src-tauri/src/lib.rs` (inserted alphabetically between `protocol` and
`sidecar`, no other lines touched).

```rust
pub fn percentiles(values: &[f64]) -> Vec<u8> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    values
        .iter()
        .map(|v| {
            let count = sorted.partition_point(|s| s <= v);
            ((count as f64 / n as f64) * 100.0).round() as u8
        })
        .collect()
}
```

Matches the brief's Step 3 exactly. `db.rs` and `cluster.rs` were not touched.

## TDD sequence (commands + actual output)

**1. Wrote the brief's test module verbatim, added `pub mod ranking;`, ran
before implementing:**

```
$ cargo test --manifest-path src-tauri/Cargo.toml ranking
error[E0425]: cannot find function `percentiles` in this scope
 --> src/ranking.rs:7:17
... (6 errors, one per test)
error: could not compile `photobook-generator` (lib test) due to 6 previous errors
```
Confirmed failing as expected (compile error, not a runtime assertion — matches
brief's expectation of "file not found for module" class of failure, module
existed but function didn't).

**2. Implemented `percentiles`, reran:**

```
$ cargo test --manifest-path src-tauri/Cargo.toml ranking
running 6 tests
test ranking::tests::empty_input_returns_empty ... ok
test ranking::tests::ties_receive_the_same_percentile ... ok
test ranking::tests::all_percentiles_are_within_bounds ... ok
test ranking::tests::preserves_input_order_not_sorted_order ... ok
test ranking::tests::ranks_ascending_values_across_the_range ... ok
test ranking::tests::single_value_is_the_top_percentile ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 22 filtered out
```

No `resource path ... doesn't exist` error occurred — `bun run sidecar` was
not needed for this task (that dependency is for the sidecar-binary
integration test, unaffected by this module).

**3. Full workspace test run after adding my own tests (below):**

```
$ cargo test --manifest-path src-tauri/Cargo.toml
... 34 passed in lib tests (12 in ranking::tests, plus db/cluster/protocol) ...
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/sidecar_ping.rs
test sidecar_binary_responds_to_ping_with_matching_id_and_version ... ok
test result: ok. 1 passed; 0 failed
   Doc-tests app_lib
test result: ok. 0 passed; 0 failed
```

**4. `cargo fmt --check`:** pre-existing drift in `db.rs`, `protocol.rs`,
`sidecar.rs` (brace-style struct-literal wrapping, unrelated to this task,
not touched). `ranking.rs` itself produced zero diff lines — confirmed by
grepping the check output for `ranking.rs` and getting nothing back.

**5. Commit:**

```
$ git add src-tauri/src/ranking.rs src-tauri/src/lib.rs
$ git commit -m "feat(ranking): add within-book percentile ranking ..."
[feat/phase-1-analysis-pipeline 882a685] feat(ranking): add within-book percentile ranking
 2 files changed, 127 insertions(+)
 create mode 100644 src-tauri/src/ranking.rs
```

Commit SHA: `882a685ee289ab766f2ae6eb3f533aba20129b75`

## Mutation-check evidence

I did not just eyeball the brief's tests — I hand-wrote three broken
implementations, swapped each into `ranking.rs` in place of the real one
(tests unchanged), ran `cargo test ranking`, confirmed the failure, then
restored the real implementation and reran to confirm green. This was done
sequentially against the *same* six brief tests, not against my added tests.

**Mutant A — order preservation.** Changed `values.iter()` to `sorted.iter()`
in the final `.map()`, so the function returns percentiles in sorted order
instead of input order.
Result: `preserves_input_order_not_sorted_order` FAILED
(`left: 33, right: 100` — expected the largest-input-first value to rank 100,
got the smallest-sorted value's rank instead). All 5 other brief tests still
passed. **Verdict: this test is real, unlike the flagged prior-task test —
its input `[9.0, 1.0, 5.0]` is deliberately unsorted, so a sorted-order bug
is caught.**

**Mutant B (first attempt, informative near-miss) — `count = partition_point(s
< v) + 1`.** I intended this to simulate "ties broken by position" but it
turned out to be mathematically identical to the correct `<=` formula for
distinct values, and for ties it still assigns the *same* (wrong) percentile
to every tied element, because strict-`<` count doesn't depend on an
element's position among duplicates. All 6 brief tests passed. This is not a
gap in the brief's tests — it's a reminder that "ties get the same value" and
"ties get the *correct* value" are different claims, and only the latter is
pinned by the `<=`-boundary tests below.

**Mutant C — ties genuinely broken by original position.** Stable-sorted
`(original_index, value)` pairs and assigned each element's rank by its own
position in that sort, so three equal values get three different ranks
based on which came first.
Result: `ties_receive_the_same_percentile` FAILED (`left: 33, right: 67`).
**Verdict: real. The brief's tie test does catch genuine positional
tie-breaking**, contrary to my prior expectation given the pattern flagged
in the task instructions.

**Mutant D — `<` instead of `<=`.**
Result: THREE tests failed — `single_value_is_the_top_percentile`
(`[0]` vs expected `[100]`), `ranks_ascending_values_across_the_range`
(`p[0]` was `0` vs expected `25`), and `preserves_input_order_not_sorted_order`
(`67` vs expected `100`). **Verdict: real, and over-determined — three
independent tests catch this, not just one.**

**Conclusion: unlike the immediately preceding task, none of the six brief
tests here are decorative.** Each of the three defect classes named in your
instructions (order preservation, ties, `<=` vs `<`) is caught by at least
one test, and two of the three are caught redundantly by multiple tests.
I did not need to silently fix anything in the brief's test file.

One soft observation, not a defect: `all_percentiles_are_within_bounds` is
genuinely weak — it only asserts `v <= 100` and would pass under almost any
implementation, correct or badly broken (e.g. an implementation returning
all zeros passes it trivially). It doesn't need fixing since the other five
tests carry the real correctness burden, but it's not pulling weight on its
own.

## My additional tests (in `src-tauri/src/ranking.rs`, appended to the same
`tests` module, all passing)

- `partial_ties_get_equal_percentile_distinct_from_non_tied_values` —
  `[1.0, 2.0, 2.0, 3.0]` → `[25, 75, 75, 100]`. The brief only tests an
  *all-identical* array for ties; a partial tie is a different shape of bug
  surface (e.g. a rank formula that's internally consistent for a uniform
  population but wrong once tied and non-tied values coexist).
- `negative_scores_rank_correctly_preserving_input_order` —
  `[1.0, -1.0, 0.0, 0.5, -0.5]` → `[100, 20, 60, 80, 40]`. Exercises the
  documented aesthetic-score range and doubles as a second, independent
  order-preservation check with exact expected values (not just relative
  comparisons).
- `single_repeated_value_throughout_all_tie_at_top` —
  `[7.0; 5]` → `[100, 100, 100, 100, 100]`. All ties must land at 100, not
  merely be mutually equal.
- `infinities_are_ranked_correctly` — `[-inf, 0.0, inf]` → `[33, 67, 100]`.
- `tied_infinities_receive_equal_top_percentile` —
  `[inf, inf, 0.0]` → `[100, 100, 33]`.
- `nan_does_not_panic_though_it_is_not_meaningfully_ranked` — pins only the
  no-panic/length-preserved guarantee, not correctness (see below).

## Edge cases the brief omits

**NaN — behaves badly, silently, and this matters for Task 15.** Empirically
verified via a temporary probe (not committed):

```
percentiles(&[1.0, NaN, 2.0])       -> [33, 0, 33]
percentiles(&[1.0, NaN, 2.0])       -> [33, 0, 33]   (repeat: deterministic)
percentiles(&[NaN, NaN, NaN])       -> [0, 0, 0]
percentiles(&[5.0, NaN, NaN, 1.0])  -> [25, 0, 0, 0]
```

Root cause: `sort_by` with `partial_cmp(...).unwrap_or(Equal)` doesn't panic
on NaN, but it also doesn't produce a total order — NaN compares "equal" to
everything for sorting purposes, so its position in `sorted` is essentially
arbitrary. `partition_point` then does a binary search assuming the array is
sorted with respect to its predicate; with a NaN in an unpredictable slot,
that assumption breaks and the binary search can return a wrong index **for
every element in the call, not just the NaN one**. This is visible above:
`5.0` is the true maximum of `[5.0, NaN, NaN, 1.0]` but was ranked 25, not
100. It's deterministic per input (no randomness — reran and got identical
output), but it is not correct and not something to rely on.

Practical implication for Task 15: **the caller must filter out NaN before
calling `percentiles`, the same way it already must filter `None`.** The
function's doc comment doesn't currently say this explicitly — worth adding
a one-line note if Task 15's caller isn't airtight about it. I did not add
NaN-filtering to `percentiles` itself since the brief scopes this function as
pure/minimal and the interface contract already delegates `None`-filtering
to the caller; NaN is the same class of problem.

**Infinities — work correctly, no special casing needed.** `f64::INFINITY`
and `f64::NEG_INFINITY` are totally ordered via `partial_cmp` against finite
numbers and each other, so they sort and rank exactly as expected, including
ties between two `+inf` values (both correctly land at 100). Locked in by
the two tests above.

**Single repeated value throughout — works correctly.** All-tied inputs
correctly land at 100 for every element, not some other value shared across
elements. Locked in by `single_repeated_value_throughout_all_tie_at_top`
(brief's `ties_receive_the_same_percentile` uses n=3; I used n=5 for a
slightly different sample size, not load-bearing on its own).

**Negative scores — work correctly.** No special-casing needed since the
`<=` comparison and sort are value-based, not sign-based. Locked in by
`negative_scores_rank_correctly_preserving_input_order` with exact expected
values across the documented [-1, 1] aesthetic-score range.

## Concerns / things worth a second look before Task 15

1. ~~NaN handling — filtering responsibility needs to be explicit and
   enforced at the Task 15 call site~~ — **resolved below**, `percentiles`
   now contains NaN itself.
2. `all_percentiles_are_within_bounds` in the brief is weak on its own (see
   mutation-check conclusion) but not harmful — left as-is per instructions
   not to silently alter the brief's tests.

## Amendment: NaN is now contained inside `percentiles`, not the caller's problem

Per coordinator direction, delegating NaN handling to callers was rejected:
the failure is non-local (a NaN in one position corrupts percentiles
computed for *other*, valid values), and that's exactly the kind of bug that
gets diagnosed three layers away from its cause. Fixed at the source.

### Root cause, precisely

`sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))` sorted the
*entire* input including NaN. `partial_cmp` returns `None` whenever either
operand is NaN, and the `Equal` fallback made NaN compare as "equal" to
everything, so its placement in the sorted array was arbitrary — breaking
the monotonicity that `partition_point`'s binary search assumes for *all*
elements, not just the NaN one.

### Fix

```rust
pub fn percentiles(values: &[f64]) -> Vec<u8> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    // Only non-NaN values participate in the sort and in the population
    // count. NaN compares unordered against everything, so including it
    // would make the sort order incoherent and corrupt `partition_point`'s
    // binary search for every other value, not just the NaN entry.
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n_valid = sorted.len();

    values
        .iter()
        .map(|v| {
            if v.is_nan() || n_valid == 0 {
                return 0;
            }
            let count = sorted.partition_point(|s| s <= v);
            ((count as f64 / n_valid as f64) * 100.0).round() as u8
        })
        .collect()
}
```

`sorted` is built only from non-NaN values, so `partial_cmp(...).unwrap()`
is safe (no NaN ever reaches it) and the binary search invariant holds. The
denominator is `n_valid` (count of non-NaN values), not the original `n` —
this is what makes "as if the NaN were not there" literally true: a valid
value's percentile is computed exactly as it would be by calling
`percentiles` on the NaN-filtered sub-array. Doc comment on the function now
states the contract explicitly.

### Tests replacing the old `nan_does_not_panic_though_it_is_not_meaningfully_ranked`

```rust
#[test]
fn nan_entries_receive_zero() {
    let p = percentiles(&[5.0, f64::NAN, 1.0]);
    assert_eq!(p[1], 0);

    let all_nan = percentiles(&[f64::NAN, f64::NAN]);
    assert_eq!(all_nan, vec![0, 0]);
}

#[test]
fn nan_does_not_perturb_the_percentiles_of_other_values() {
    let with_nan = percentiles(&[5.0, f64::NAN, 1.0, 3.0]);
    let without_nan = percentiles(&[5.0, 1.0, 3.0]);

    assert_eq!(with_nan[0], without_nan[0], "5.0 (max)");
    assert_eq!(with_nan[1], 0, "the NaN entry itself");
    assert_eq!(with_nan[2], without_nan[1], "1.0 (min)");
    assert_eq!(with_nan[3], without_nan[2], "3.0 (middle)");
}
```

The second test is the one that matters — it's the property that was
actually broken. `nan_entries_receive_zero` alone would pass under the old,
corrupting implementation (verified below), which is exactly the trap the
coordinator flagged.

### Mutation-check: reverted the sort/filter to the old behaviour, kept everything else

Swapped only the `sorted` construction back to the pre-fix version (include
NaN, `unwrap_or(Equal)` fallback), left the `v.is_nan() -> 0` branch and
`n_valid` naming in place, reran:

```
$ cargo test --manifest-path src-tauri/Cargo.toml ranking
running 13 tests
test ranking::tests::all_percentiles_are_within_bounds ... ok
test ranking::tests::negative_scores_rank_correctly_preserving_input_order ... ok
test ranking::tests::infinities_are_ranked_correctly ... ok
test ranking::tests::empty_input_returns_empty ... ok
test ranking::tests::partial_ties_get_equal_percentile_distinct_from_non_tied_values ... ok
test ranking::tests::nan_entries_receive_zero ... ok
test ranking::tests::preserves_input_order_not_sorted_order ... ok
test ranking::tests::ranks_ascending_values_across_the_range ... ok
test ranking::tests::single_repeated_value_throughout_all_tie_at_top ... ok
test ranking::tests::single_value_is_the_top_percentile ... ok
test ranking::tests::tied_infinities_receive_equal_top_percentile ... ok
test ranking::tests::ties_receive_the_same_percentile ... ok
test ranking::tests::nan_does_not_perturb_the_percentiles_of_other_values ... FAILED

---- ranking::tests::nan_does_not_perturb_the_percentiles_of_other_values stdout ----
thread '...' panicked at src/ranking.rs:146:9:
assertion `left == right` failed: 1.0 (min)
  left: 75
 right: 33

test result: FAILED. 12 passed; 1 failed; 0 ignored; 0 measured; 22 filtered out
```

Confirms two things at once: (1) the fix is load-bearing — reverting it
breaks a real test — and (2) `nan_entries_receive_zero` passed even under
the corrupting old sort, exactly as predicted: a test that only pins "NaN
gets 0" gives false confidence while the actual corruption of neighbouring
values continues undetected. Restored the fix immediately after (verified
byte-identical to the pre-mutation file via a saved copy) and reran the full
workspace suite.

### Final verification

```
$ cargo test --manifest-path src-tauri/Cargo.toml
running 35 tests
test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/sidecar_ping.rs
test result: ok. 1 passed; 0 failed
   Doc-tests app_lib
test result: ok. 0 passed; 0 failed
```

`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, grepped for
`ranking.rs`: no output — still clean.

Commit: `feat(ranking): contain NaN inside percentiles instead of the caller's contract`
(Conventional Commits, no `--no-verify`).
