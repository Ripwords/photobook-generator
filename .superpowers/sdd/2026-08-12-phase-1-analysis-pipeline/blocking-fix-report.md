# Sidecar worker-pool starvation fix + review findings — report

Branch: `feat/phase-1-analysis-pipeline`. Scope: `src-tauri/` only; `app/` was
not touched.

## Finding 1 (Critical) — `Sidecar::request` starves its own drain task

### Root cause (confirmed, not re-derived)

Traced and documented by another agent in `thumbnails-report.md`: `Sidecar::spawn`
spawns the stdout-drain loop as a tokio task via `tauri::async_runtime::spawn`.
`Sidecar::request` then does a *blocking* `std::sync::mpsc::Receiver::recv_timeout`
on the calling thread. Since `analyze_folder` is an async `#[tauri::command]`,
`request` runs as a task on the same tauri/tokio worker pool the drain task
needs. On a machine with few worker threads, the blocking wait occupies the
only thread the drain task could run on, so the request resolves only once
its *own* timeout elapses — `analyze_folder` times out even though the
sidecar responded successfully.

### Fix chosen: `spawn_blocking` at the `analyze_folder` call site

In `src-tauri/src/commands.rs`, the `pool.analyze_all(...)` call (which is
where the blocking `Sidecar::request` chain lives) is now wrapped in
`tauri::async_runtime::spawn_blocking`, fetching `AppState` via
`app.state::<AppState>()` inside the closure (since the closure must be
`'static`, it can't borrow the command's `State<'_, AppState>` parameter —
so that parameter was dropped from `analyze_folder`'s signature entirely):

```rust
let records = {
    let app_for_pool = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<serde_json::Value>, String> {
        let state = app_for_pool.state::<AppState>();
        let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
        Ok(pool.analyze_all(&app_for_pool, &misses, &thumbnail_dir))
    })
    .await
    .map_err(|e| e.to_string())??
};
```

`spawn_blocking` moves the blocking wait onto tokio's separate blocking-thread
pool (default up to 512 threads, distinct from the async worker pool), so the
worker pool stays free for the drain task regardless of how many workers
exist.

### Why this over the alternatives

- **Whole path non-async**: would still need the drain task to run
  somewhere; doesn't remove the underlying dependency, just moves where the
  deadlock could resurface (e.g. from a background thread pool of our own
  design). More surface area for no real benefit over `spawn_blocking`,
  which tokio already provides for exactly this.
- **Synchronous drain on a dedicated `std::thread`** (rather than a tokio
  task): this is the more "root-cause" fix — it would remove the systemic
  risk that *any* future blocking call from an async command could re-starve
  the drain task, not just this one call site. I did not take this path
  because `Sidecar::spawn`'s drain loop reads from
  `tauri_plugin_shell::process::Receiver` (a `tokio::sync::mpsc::Receiver`);
  moving it to a raw thread means calling `.blocking_recv()` off the tokio
  runtime, which is a larger, less-tested change to a piece of code (the
  drain task) that both the ping path and this fix's regression test depend
  on being correct. `spawn_blocking` is a one-call, localized change at the
  exact site the bug lives in, with an established idiom (this is literally
  what tauri's own docs recommend for blocking work inside async commands),
  and it's what the task brief called "the straightforward approach." Given
  the fix needed to ship with strong test coverage rather than a broader
  refactor, I took the smaller, well-contained diff.

If a *second* blocking call is ever added to an async command in this
codebase, it will reproduce this same class of bug — worth a lint/review
note for the future, but out of scope here.

### Test: `src-tauri/tests/sidecar_worker_pool.rs`

Calls the real, unmodified `analyze_folder` command function (not a
reimplementation) against the real built sidecar binary, spawned through
`tauri_plugin_shell`/`AppHandle` exactly as production does, on a
deliberately single-worker-thread tokio runtime (`worker_threads(1)`,
reproducing a low-core machine).

**Why this file has no `#[test]` / uses `harness = false`:** `analyze_folder`
and everything it calls (`Sidecar::spawn`, `ShellExt::shell`) are typed
against the concrete default `AppHandle` (`= AppHandle<tauri::Wry>`), not
generic over `tauri::Runtime`. `tauri::test::mock_builder()`'s
`MockRuntime`-backed `AppHandle` does not type-check as an argument to the
real `analyze_folder` — confirmed by trying it first (compile error
`expected AppHandle<Wry>, found AppHandle<MockRuntime>`). Building a real
`Wry` app requires calling into AppKit/tao, which macOS requires happen on
the process's actual main thread; the default `#[test]` harness runs each
test on a worker thread, not the real main thread, so that would panic.
`harness = false` (added as a `[[test]]` entry in `Cargo.toml`, scoped to
just this one test binary) makes the whole file a plain `fn main()`, run
directly by `cargo test` on the real main thread. The window list is cleared
from the loaded config (`context.config_mut().app.windows.clear()`) before
`.build()` so this doesn't flash a window on screen.

**Watchdog:** a raw `std::thread::spawn` plus
`std::sync::mpsc::Receiver::recv_timeout(Duration::from_secs(60))`,
independent of the constrained runtime under test — the reproduction itself
runs via `tauri::async_runtime::spawn` (putting it truly on the
single-worker-thread pool, the same way Tauri's own async command dispatch
would) and is awaited via `tauri::async_runtime::block_on` on the watchdog
thread, which does not consume one of the runtime's worker threads. A
previous task in this project learned that a watchdog scheduled onto the
same starved pool cannot fire; this watchdog is a genuinely separate OS
thread that would fire and fail the test even under a true, permanent hang.

**HOME redirection:** the real generated `tauri.conf.json` context is used
(needed for `identifier`/`bundle.externalBin` so `app.shell().sidecar(...)`
resolves the real binary), so `app_data_dir()` would otherwise point at the
real user's `~/Library/Application Support/com.jiajingteoh.photobook`. `HOME`
is redirected to a throwaway temp dir for the duration of the test (single
`unsafe { std::env::set_var(...) }` call, first thing in `main`, before any
other thread exists) so the test doesn't write into — or lock-contend
against — the real app's sqlite cache/thumbnails.

### Commands run, actual output

```
$ cargo build --lib
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.44s

$ cargo test
running 65 tests ... test result: ok. 65 passed; 0 failed
Running tests/sidecar_ping.rs ... test result: ok. 1 passed; 0 failed
Running tests/sidecar_worker_pool.rs
ok: analyze_folder completed in 152.8355ms on a single-worker-thread runtime

$ cargo clippy --all-targets --all-features
warning: writing `&mut Vec` instead of `&mut [_]` ... src/cluster.rs:12  (pre-existing, unrelated to this change)
   (no other warnings)

$ cargo build --release
    Finished `release` profile [optimized] target(s) in 32.14s
```

### Mutation evidence (Finding 1)

Temporarily reverted the fix in `commands.rs` to the original inline call:

```rust
let records = {
    let state = app.state::<AppState>();
    let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
    pool.analyze_all(&app, &misses, &thumbnail_dir)
};
```

Ran `cargo test --test sidecar_worker_pool -- --nocapture`:

```
thread 'main' (1292501) panicked at tests/sidecar_worker_pool.rs:117:13:
assertion `left == right` failed: no photo in sidecar/Fixtures should fail to analyze
  left: 3
 right: 0
error: test failed, to rerun pass `--test sidecar_worker_pool`
```

All 3 fixture photos came back as `failed` — the sidecar batch call starved
the drain task, timed out, retried once (per `analyze_batches`), timed out
again, and was replaced with synthetic failure records. This is the exact
symptom `thumbnails-report.md` describes ("every real invocation failed with
`sidecar timed out`... even though the Swift sidecar was demonstrably
completing successfully"). Reverted; re-ran `cargo test --test
sidecar_worker_pool` and confirmed a clean pass again (161ms).

## Finding 2 (Important) — cache-before-sidecar now unit tested

Extracted the hash → cache-lookup → miss-list → count block from
`analyze_folder` into `commands::lookup_cache(db: &Db, paths: &[String]) ->
Result<CacheLookup, String>`, returning `{ hits, misses, hash_failures }`.
`analyze_folder` now calls it and derives `cached = hits.len()`, `failed =
hash_failures` (+ later sidecar failures), same behavior as before.

Four new tests in `commands.rs`, using `Db::open_in_memory()`:
- `a_cached_hash_is_a_hit_not_a_miss`
- `an_uncached_hash_is_a_miss`
- `a_hash_failure_is_counted_and_does_not_silently_vanish`
- `every_input_path_is_accounted_for_exactly_once` (asserts `hits.len() +
  misses.len() + hash_failures == input.len()`)

### Mutation evidence (Finding 2)

Temporarily replaced the cache read with a constant `None`:

```rust
Ok(hash) => match (None::<String>, hash) {
    (Some(json), _) => match serde_json::from_str(&json) { ... },
    (None, _) => misses.push(path.clone()),
},
```

`cargo test --lib commands::tests`:

```
test commands::tests::a_cached_hash_is_a_hit_not_a_miss ... FAILED
  left: 0
 right: 1
test commands::tests::every_input_path_is_accounted_for_exactly_once ... FAILED
test result: FAILED. 12 passed; 2 failed
```

Reverted; re-ran, 14/14 passed. This reproduces the exact prior regression
class ("a design that wrote to the cache and never read it").

## Finding 3 (Important) — untyped payload boundary

Added a comment to `AnalysisSummary` in `src-tauri/src/commands.rs` naming
`app/types/features.ts`'s `AnalyzedPhoto`/`AnalysisSummary` as its
counterpart and stating both sides must be changed together.

**`app/types/features.ts` was intentionally left untouched** — it's under
`app/`, which is out of my ownership boundary for this task (another agent
is redesigning the UI there concurrently). The mirroring comment that
should go on `AnalyzedPhoto` in that file, for whoever next touches it:

```ts
/**
 * Wire-compatible counterpart of `AnalysisSummary`/`photos: Vec<serde_json::Value>`
 * in src-tauri/src/commands.rs. There is no compile-time link between the two
 * sides (the Rust side is untyped JSON on purpose — Phase 2 adds fields here).
 * If you rename, add, or remove a field here, update the Rust side in the same
 * change, and vice versa.
 */
```

**Is a generated-types approach worth it for Phase 2?** Lean yes, but not a
full OpenAPI/Eden-style pipeline (this isn't a Nest.js/Elysia monorepo, and
Tauri commands aren't backed by an API schema generator the way the user's
CLAUDE.md conventions assume for that stack). The lower-effort, high-value
version: derive TypeScript types from the Rust structs directly with
`ts-rs` or `specta` (tauri-native, `specta` has first-class
`tauri-specta` integration that also generates the `invoke` wrappers) once
`AnalyzedPhoto` stops being `serde_json::Value` and gets real fields on the
Rust side in Phase 2. Until then, `photos` is deliberately untyped `Value`
specifically *because* its shape is still moving — generating types off an
`Value`-typed field would just generate `unknown`, so this is worth doing
exactly when Phase 2 gives `photos` a real Rust type, not before.

## Finding 4 (Minor) — silent drops now logged

`std::fs::read_dir(...).filter_map(Result::ok)` in `analyze_folder` replaced
with a `filter_map` that logs `log::warn!("skipping unreadable directory
entry in {folder}: {err}")` on `Err` before dropping the entry, matching the
existing `hash_file` failure logging style.

## Full test run (after all four findings applied)

```
$ cargo test
running 65 tests (lib) ... ok. 65 passed; 0 failed
Running tests/sidecar_ping.rs ... ok. 1 passed; 0 failed
Running tests/sidecar_worker_pool.rs
ok: analyze_folder completed in 153.009208ms on a single-worker-thread runtime
Doc-tests app_lib ... ok. 0 passed; 0 failed

$ cargo clippy --all-targets --all-features
warning: src/cluster.rs:12 (pre-existing, unrelated)
(no warnings from any code touched in this change)

$ cargo build --release
    Finished `release` profile [optimized] target(s) in 32.14s
```

`sidecar/Fixtures` (the fixture dir the new test points at) has 3 top-level
image files (`gps-south-west.jpg`, `landscape.jpg`, `portrait-rot90.jpg`);
`hostile/` and `PhotobookEngineTests/` are subdirectories and are excluded
by `analyze_folder`'s existing `.filter(|p| p.is_file())`, so the
adversarial fixtures under `hostile/` are not exercised by this test — it's
about the worker pool, not decode robustness.

## Does a real `analyze_folder` run now complete?

Yes, in-process, through the real command function, the real sidecar
binary, and real `tauri_plugin_shell` spawning, on a genuinely
single-worker-thread tokio runtime: completes in ~150-600ms (variance
across runs, all well under the 13s floor timeout). Before the fix, the
identical harness reproducibly failed with all photos marked `failed` after
the sidecar's retry-then-give-up path. I did not additionally verify through
the live GUI app (`bun run dev`) — the in-process test exercises the same
code path with less flakiness and was the explicit ask; re-verifying through
the full GUI was not repeated since the previous agent already established
that harness surfaces this exact bug reliably.

## Uncertain / worth a second look

- `spawn_blocking`'s default blocking-pool cap (512 threads in tokio) is
  more than enough for this app's single sidecar pool, but if `AppState`
  ever grows multiple concurrent `analyze_folder` calls in flight, each
  would occupy its own blocking-pool thread for the duration of a sidecar
  batch — not a correctness issue, just worth knowing the resource shape
  changed slightly (async worker thread → blocking-pool thread) if this is
  ever profiled.
- The `state: State<'_, AppState>` parameter was dropped from
  `analyze_folder`'s public signature (no longer needed — `AppState` is
  fetched via `app.state::<AppState>()` inside the `spawn_blocking`
  closure instead, since the closure must be `'static` and can't borrow the
  command's `State<'_, _>`). This is a public API change to a `pub async
  fn`, but `#[tauri::command]` parameters aren't part of any external
  contract (the frontend calls `invoke('analyze_folder', { folder })`,
  which was never passing `state` — it's tauri-injected), so this should be
  fully backward compatible; flagging in case anything else in `src-tauri/`
  called `commands::analyze_folder(...)` directly with a `State` argument
  (grepped for this — nothing else does).
- I did not attempt the "drain task on a dedicated `std::thread`" fix as a
  belt-and-suspenders second layer on top of `spawn_blocking`. If a future
  change reintroduces an inline blocking call somewhere else in an async
  command, it will hit the same class of bug again; `spawn_blocking` fixes
  this one call site correctly but isn't a structural guarantee against the
  next one.
