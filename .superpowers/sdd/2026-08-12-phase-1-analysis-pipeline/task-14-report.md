# Task 14 report: Sidecar resilience — batching, timeout, respawn

## Implementation

Files modified:
- `src-tauri/src/protocol.rs` — added `ResponseResult::Analyzed(Vec<serde_json::Value>)` as a tuple variant (matches Swift's `{"type":"analyzed","data":[...]}` under `#[serde(tag = "type", content = "data")]`). Adding the variant made `Sidecar::ping`'s existing 2-arm match non-exhaustive, so it now has a third arm mapping `Analyzed(_)` to a `Malformed` error.
- `src-tauri/src/sidecar.rs` — added `BATCH_SIZE`, `chunk_paths`, `timeout_for`, `failure_record`, `Sidecar::analyze`, `SidecarPool` (`new`, `ensure`, `analyze_all`, `Default`), and the test module, all as specified in the brief verbatim, plus one deviation (see below) and extra tests (see "Mutation evidence").
- `src-tauri/tests/sidecar_ping.rs` — drains the child's stderr on a background thread (see "Stderr pipe" below). Not in the brief's file list; done because the brief's context section asked me to assess and fix if cheap.

### Deviation from the brief: `Sidecar` field became `Option<CommandChild>`, plus a `Drop` impl

The brief's context section flagged that `Sidecar` has no `Drop` and asked me to add one if straightforward. `CommandChild::kill(self)` consumes `self` by value, but `Drop::drop` only ever gets `&mut self`, so the field had to become `child: Option<CommandChild>` to let `Drop::drop` `.take()` it out and call `.kill()`. This touched two other spots: `spawn()` now wraps the child in `Some(..)`, and `request()` unwraps it via `.as_mut().expect("Sidecar's child is only taken by Drop")` — safe in practice since nothing but `Drop` ever empties the option, so `request()` can never observe `None` on a live `Sidecar`. This was straightforward (no unsafe code, ~10 lines), so I did it rather than reporting it as blocked.

```rust
impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
    }
}
```

Killing the child also resolves the "leaks the drain task" half of the same review note: killing the process ends its stdout, the shell plugin's channel closes, `rx.recv().await` in the drain task returns `None`, and the task exits.

### Stderr pipe in `sidecar_ping.rs`

The brief's context flagged that this integration test pipes the child's stderr (`Stdio::piped()`) but never reads it, which could fill the OS pipe buffer and block the child on write if it gets chatty. I checked whether `analyze` makes this reachable: it doesn't directly — this test only ever sends a `Ping` request, and `analyze` isn't exercised by any Rust-level integration test (see "Coverage gaps" below), so this task doesn't newly reach that code path. I fixed it anyway since it was cheap and is exactly the kind of latent hazard that would bite silently the day someone adds an `analyze` integration test or the Swift side starts logging to stderr:

```rust
let stderr = child.stderr.take().expect("child stderr was piped");
std::thread::spawn(move || {
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        line.clear();
    }
});
```

Note this is only a partial mitigation elsewhere: `Sidecar` (the tauri_plugin_shell-based struct in `sidecar.rs`, used by the real app) does not have this problem — its drain task's `while let Some(event) = rx.recv().await` already consumes every `CommandEvent` variant including `Stderr`, even though only `Stdout` is acted on, so the plugin-managed stderr is never allowed to back up.

## Commands run

```
$ cargo test --manifest-path src-tauri/Cargo.toml   # step 2, before implementation
error[E0425]: cannot find function `chunk_paths` in this scope
error[E0425]: cannot find function `chunk_paths` in this scope
error[E0425]: cannot find function `timeout_for` in this scope
error[E0425]: cannot find function `timeout_for` in this scope
error[E0425]: cannot find function `timeout_for` in this scope
error[E0425]: cannot find function `failure_record` in this scope
error: could not compile `photobook-generator` (lib test) due to 7 previous errors
```

```
$ cargo test --manifest-path src-tauri/Cargo.toml   # step 4, after implementation
running 43 tests
... (all cluster/ranking/protocol/db/sidecar tests) ...
test sidecar::tests::chunks_paths_into_batches ... ok
test sidecar::tests::empty_input_produces_no_batches ... ok
test sidecar::tests::chunking_preserves_every_path_in_order ... ok
test sidecar::tests::synthetic_failure_record_names_the_path ... ok
test sidecar::tests::timeout_has_a_floor_for_a_single_photo ... ok
test sidecar::tests::timeout_matches_exact_formula ... ok
test sidecar::tests::timeout_scales_with_batch_size ... ok
test sidecar::tests::zero_batch_size_does_not_panic ... ok
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/sidecar_ping.rs
running 1 test
test sidecar_binary_responds_to_ping_with_matching_id_and_version ... ok
test result: ok. 1 passed; 0 failed

   Doc-tests app_lib
test result: ok. 0 passed
```

```
$ cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
warning: writing `&mut Vec` instead of `&mut [_]` involves a new object where a slice will do
  --> src/cluster.rs:12:21   (pre-existing, unrelated to this task)
warning: `photobook-generator` (lib) generated 1 warning
```

No `bun run sidecar` rebuild was needed — `src-tauri/binaries/photobook-engine-aarch64-apple-darwin` already existed from an earlier task.

## Mutation evidence

I mutated the implementation three ways, ran the test suite, recorded results, then reverted (verified with `diff` against a pre-mutation backup — clean).

**1. `chunk_paths` reverses the chunk list** (`v.reverse()` after collecting):
```
test sidecar::tests::chunks_paths_into_batches ... FAILED
  left: 5, right: 10   (batches[0].len() was the remainder chunk)
test sidecar::tests::chunking_preserves_every_path_in_order ... FAILED
  left: ["/photos/6.jpg", "/photos/3.jpg", ...], right: ["/photos/0.jpg", ...]
```
Both of the brief's own tests catch this — not vacuous.

**2. `chunk_paths` silently drops the last path** (`&paths[..paths.len()-1]` before chunking):
```
test sidecar::tests::chunks_paths_into_batches ... FAILED
test sidecar::tests::chunking_preserves_every_path_in_order ... FAILED
test sidecar::tests::zero_batch_size_does_not_panic ... FAILED   (my added test)
```
Caught, not vacuous.

**3. `timeout_for` ignores its argument** (`Duration::from_secs(10)` regardless of `batch_len`):
```
test sidecar::tests::timeout_scales_with_batch_size ... FAILED
  assertion failed: timeout_for(20) > timeout_for(1)
test sidecar::tests::timeout_matches_exact_formula ... FAILED   (my added test)
  left: 10s, right: 13s
test sidecar::tests::timeout_has_a_floor_for_a_single_photo ... ok   <-- passes vacuously in isolation
```

Answering the brief's specific questions:
- **Would `chunking_preserves_every_path_in_order` fail if chunks were emitted in reverse order, or if a path were dropped?** Yes to both, confirmed above by mutation.
- **Would the timeout tests fail if `timeout_for` ignored its argument entirely?** `timeout_scales_with_batch_size` does fail (confirmed above), so the *suite* catches it. But `timeout_has_a_floor_for_a_single_photo` **passes on its own** under that exact mutant, because `Duration::from_secs(10) >= Duration::from_secs(10)` is true — it only pins the floor, not that the value actually depends on `batch_len`. It's the pairing with `timeout_scales_with_batch_size` that saves it; in isolation it's vacuous for this mutant. I did not modify this test (per instructions), but flagging it as required.

I did not find the given `chunks_paths_into_batches` / `chunking_preserves_every_path_in_order` tests to be vacuous — they're solid. The one real gap in the brief's own suite is the timeout floor test's weakness in isolation, and the fact that no given test pins the exact "3s per photo" multiplier (a mutant using e.g. 1ms/photo instead of 3s/photo would pass `timeout_scales_with_batch_size` and the floor test, since both only check inequalities, not the literal value from the spec).

### Tests I added to close these gaps

- `timeout_matches_exact_formula` — pins `timeout_for(0) == 10s`, `timeout_for(1) == 13s`, `timeout_for(16) == 10+3*16` seconds, i.e. the literal "10s + 3s per photo" from the brief, not just an inequality.
- `zero_batch_size_does_not_panic` — the reference implementation's `.max(1)` guard against `[T]::chunks(0)` panicking is untouched by every test in the brief (none call `chunk_paths` with `batch_size: 0`), so a mutant deleting `.max(1)` would pass the whole given suite. Confirmed by mutation #2 above (which also exercises this path) and added a direct test.

## Coverage gaps (untested, and why)

`SidecarPool::analyze_all`'s two most valuable behaviors — respawn-on-crash and the one-record-per-input-path guarantee under failure — are **not covered by any test I wrote**, because both `ensure()` and `Sidecar::spawn()` require a real `AppHandle`, which can't be constructed in a `#[cfg(test)]` unit test without standing up a Tauri app (no mock/trait exists for `Sidecar` — `analyze_all` calls the concrete type directly). Specifically untested:
- A batch that times out or the sidecar crashes on attempt 1, succeeds on attempt 2 (the respawn path).
- A batch that fails twice in a row → synthetic `failed` records, one per path, with `self.inner` left `None` so the *next* `analyze_all` call respawns fresh.
- A sidecar returning a short/reordered array (the "record count mismatch" branch) is also unexercised beyond code reading.
- The new `Drop` impl killing the child on a real spawned process (only exercised implicitly by `sidecar_ping.rs`'s `child.kill()`/`wait()` at the end of that test, which is the raw `std::process::Command` path, not `Sidecar`'s `tauri_plugin_shell` path).

**Could an integration test reach any of this, the way `sidecar_ping.rs` does for ping?** Partially, and only the parts that don't need `SidecarPool`/`AppHandle`:
- `Sidecar::analyze` end-to-end against the real binary (raw `Command`, following `sidecar_ping.rs`'s pattern) is reachable and would validate the `ResponseResult::Analyzed` wire format actually round-trips from Swift — that's a real gap since nothing currently sends a real `Analyze` request to the built binary. I did not add this because it's out of the brief's file list (`Test: inline #[cfg(test)] module in sidecar.rs` only) and because task 15 will need real fixture photos to make it meaningful (the hostile-input fixture corpus from task 10 could supply these).
- `SidecarPool::analyze_all`'s respawn/retry/failure-record logic specifically is **not** reachable this way, because that logic lives above `Sidecar` and is coupled to `AppHandle` via `ensure()`. Reaching it would need either: (a) a `tauri::test::mock_app()`-based test that spawns a sidecar and kills it mid-batch to force the crash path, or (b) refactoring `SidecarPool` to take a trait object instead of concrete `Sidecar`/`AppHandle` so a fake can simulate crash-then-succeed. Neither was in scope here; flagging for task 15 or a follow-up if this logic's correctness needs direct verification rather than code review.

## Commit

```
git add src-tauri
git commit -m "feat(sidecar): add batching, scaled timeouts, and crash respawn"
```

## Follow-up: extracting `analyze_batches` to close the central coverage gap

Per coordinator feedback, extracted the retry-and-backfill logic out of `SidecarPool::analyze_all` into a pure, crate-private function:

```rust
pub(crate) fn analyze_batches<F>(paths: &[String], batch_size: usize, mut call: F) -> Vec<serde_json::Value>
where F: FnMut(&[String]) -> Result<Vec<serde_json::Value>, SidecarError>
```

It owns chunking, up to two `call` invocations per batch, the record-count check, and synthesizing `failure_record`s. `SidecarPool::analyze_all` is now a thin wrapper:

```rust
pub fn analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value> {
    analyze_batches(paths, BATCH_SIZE, |batch| {
        let result = self.ensure(app).and_then(|sidecar| sidecar.analyze(batch.to_vec()));
        if result.is_err() {
            // Drop the child so the next ensure() respawns it.
            self.inner = None;
        }
        result
    })
}
```

The respawn side effect (`self.inner = None` on error, `self.ensure(app)` to spawn) stays in the wrapper's closure; `analyze_batches` itself touches no `AppHandle`/`Sidecar` state.

### New tests (all in `sidecar.rs`'s existing `#[cfg(test)] mod tests`)

- `analyze_batches_passes_through_records_unchanged_on_success` — always-succeeding fake, checks count and that each output record sits at the same offset as its input path.
- `analyze_batches_retries_once_then_succeeds` — fails attempt 1, succeeds attempt 2; asserts `invocations == 2` (the retry actually happened, not just that the end state looks right) and correct records.
- `analyze_batches_synthesizes_one_failure_record_per_path_when_call_always_errors` — always-failing fake; asserts exactly 2 invocations (no runaway retry beyond the spec'd two attempts) and one `failed` record per path, each naming its own path.
- `analyze_batches_backfills_when_call_returns_too_few_records` — the subtle case the coordinator flagged: fake returns 2 records for a 3-path batch (via `Ok` — this is not an `Err`, so it must be caught by the length check, not the retry loop). Asserts the output is still 3 records, all `failed`, one per path — the short array is never extended verbatim.
- `analyze_batches_keeps_successful_batches_at_correct_offsets_around_a_failed_one` — 7 paths, batch size 3 → `[0,1,2] [3,4,5] [6]`; the middle batch always errors, the other two always succeed. Asserts total count equals total input (7) and, per offset, that the record at each index both names its own input path and has the expected status (`ok` for offsets 0-2 and 6, `failed` for offsets 3-5) — this is the offset check, not just a count, so a batch-reordering bug would be caught.

### Mutation evidence for the wrong-count case

Mutated `analyze_batches`' backfill logic to drop the length check (`Some(r) if r.len() == batch.len() => ...` / mismatch arm collapsed into `Some(r) => out.extend(r)`, i.e. the short array is extended verbatim instead of being replaced with failure records):

```
test sidecar::tests::analyze_batches_backfills_when_call_returns_too_few_records ... FAILED
  assertion `left == right` failed: must still be one record per input path
  left: 2
 right: 3
```

Caught immediately — the other 12 sidecar tests still passed under this mutant, confirming this specific test (not incidental coverage from another test) is what guards the count-mismatch path. Reverted and confirmed a clean `diff` against the pre-mutation file, then reran the full suite (48 lib tests + 1 integration test, all passing).

### What remains uncovered, by design

`SidecarPool::analyze_all`'s respawn side effect (`self.ensure(app)` spawning a real `Sidecar`, `self.inner = None` forcing a respawn on the next call) is still untested and, per the coordinator's instruction, intentionally so — it needs a real `AppHandle`, and introducing a trait abstraction purely to fake that one call was judged not worth it. What *is* now covered is everything `analyze_all` delegates to `analyze_batches`: retry count, failure-record synthesis, and — the guarantee this task exists for — one record per input path at the correct offset, including when the sidecar returns a short array. The only thing outside `analyze_batches`'s reach is "does calling `Sidecar::spawn` again actually produce a working child," which is exercised indirectly by `sidecar_ping.rs`'s real-binary ping test, just not through `SidecarPool`.

## Commit (follow-up)

```
git add src-tauri
git commit -m "refactor(sidecar): extract analyze_batches for direct unit testing"
```

## Follow-up 2: pin the `analyzed` wire format in `protocol.rs`

Coordinator review noted `protocol.rs` already had `deserializes_pong_response` and `deserializes_error_response` establishing the "decode a literal Swift wire-format line" pattern, but nothing did the same for `analyzed` — the case every photo actually goes through. `Analyzed(Vec<serde_json::Value>)` decoding `{"type":"analyzed","data":[...]}` was correct by `#[serde(tag = "type", content = "data")]` semantics for a single-field tuple variant, but nothing pinned it: turning it into a struct variant, or changing the `content` key, would still compile and only fail at runtime as a `Malformed` error from the real sidecar — which `analyze_batches` then silently degrades into failure records for the whole batch.

Added `deserializes_analyzed_response` to `src-tauri/src/protocol.rs`, decoding a literal wire-format line with two records shaped like real ones (`{"status":"ok","features":{...}}` and `{"status":"failed","path":"...","message":"..."}`), asserting the variant matches and both the count and field contents of the decoded array are correct.

```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib protocol::
running 4 tests
test protocol::tests::deserializes_error_response ... ok
test protocol::tests::serializes_ping_request_as_single_line ... ok
test protocol::tests::deserializes_pong_response ... ok
test protocol::tests::deserializes_analyzed_response ... ok
test result: ok. 4 passed; 0 failed
```

Full suite reran clean after this change (49 lib tests + 1 integration test).

## Commit (follow-up 2)

```
git add src-tauri
git commit -m "test(protocol): pin the analyzed response wire format"
```
