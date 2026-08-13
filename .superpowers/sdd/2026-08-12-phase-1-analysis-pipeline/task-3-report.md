# Task 3 Report: Sidecar build script and Rust spawn

Status: DONE
Commit: ba4865f — "feat(sidecar): wire Swift sidecar into Tauri with NDJSON transport"

## What was implemented

Followed the brief's steps exactly, with one necessary reordering (see Deviations).

1. **`src-tauri/src/protocol.rs`** (new) — `RequestKind`, `Request`, `ResponseResult`
   (`#[serde(tag = "type", content = "data", rename_all = "lowercase")]`), `Response`,
   plus the three inline `#[cfg(test)]` tests from the brief, verbatim.
2. **`src-tauri/src/sidecar.rs`** (new) — `SidecarError`, `Sidecar { child, lines, counter }`
   with `spawn`, `next_id`, `request`, `ping`, verbatim from the brief. The drain-thread
   pattern and the "ignore stale response ids" loop in `request` were kept exactly as
   instructed (context notes explained why).
3. **`scripts/build-sidecar.sh`** (new, `chmod +x`) — builds the Swift package in release
   mode targeting `arm64-apple-macos15.0`, copies `PhotobookEngine` to
   `src-tauri/binaries/photobook-engine-<host-tuple>`.
4. **`src-tauri/src/lib.rs`** — added `pub mod protocol;` and `pub mod sidecar;` above `run()`.
5. **`src-tauri/tauri.conf.json`** — added `"externalBin": ["binaries/photobook-engine"]`
   under `bundle` (not `resources` — externalBin binaries get codesigned, resources don't).
6. **`src-tauri/capabilities/default.json`** — replaced permissions array with
   `core:default`, `dialog:default`, `fs:default`, and an explicit
   `shell:allow-execute` entry scoped to `{ "name": "binaries/photobook-engine", "sidecar": true }`.
7. **`.gitignore`** — already contained `src-tauri/binaries` and `sidecar/.build` from
   the Task 1/2 commit; verified, no change needed.
8. **`package.json`** — already had a `"sidecar": "bash scripts/build-sidecar.sh"` script
   from earlier scaffolding; no change needed, `bun run sidecar` worked as-is.

## TDD sequence and commands run

### Step 1/2: write failing test, confirm it fails

Wrote `src-tauri/src/protocol.rs` containing only the `#[cfg(test)] mod tests` block from
the brief (types not yet defined).

First attempt at "confirm failing":

```
$ cargo test --manifest-path src-tauri/Cargo.toml
   Compiling photobook-generator v0.1.0 ...
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.59s
     Running unittests src/lib.rs ...
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

This was not a real signal — `protocol.rs` wasn't wired into `lib.rs` yet, so the compiler
never saw the test module at all (0 tests ran instead of failing). Since Step 3 requires
adding `pub mod protocol;` to `lib.rs` regardless, I added that line first (deviation,
see below), then re-ran:

```
$ cargo test --manifest-path src-tauri/Cargo.toml
error[E0422]: cannot find struct, variant or union type `Request` in this scope
 --> src/protocol.rs:7:19
error[E0412]: cannot find type `Response` in this scope
  --> src/protocol.rs:16:18
error[E0412]: cannot find type `Response` in this scope
  --> src/protocol.rs:27:18
error[E0433]: failed to resolve: use of undeclared type `RequestKind`
 --> src/protocol.rs:7:51
error[E0433]: failed to resolve: use of undeclared type `ResponseResult`
  --> src/protocol.rs:29:13
error[E0433]: failed to resolve: use of undeclared type `ResponseResult`
  --> src/protocol.rs:19:13
error: could not compile `photobook-generator` (lib test) due to 6 previous errors
```

This is a genuine failure (missing types), functionally equivalent to the brief's expected
"file not found for module" — the brief's stated expected error text didn't literally
occur, but the intent (test fails to compile/run before implementation exists) is verified.

### Step 3: implement

Added the type definitions to `protocol.rs`, wrote `sidecar.rs`, wired both modules into
`lib.rs`, added `externalBin` to `tauri.conf.json`, and the `shell:allow-execute` capability
entry — all exactly as specified in the brief.

Attempting `cargo test` at this point failed for an unrelated reason — `tauri-build`'s
build script validates that `externalBin` resource paths exist on disk *at compile time*,
and the sidecar binary hadn't been built yet:

```
$ cargo test --manifest-path src-tauri/Cargo.toml
error: failed to run custom build command for `photobook-generator v0.1.0 ...`
Caused by:
  ...
  resource path `binaries/photobook-engine-aarch64-apple-darwin` doesn't exist
```

This confirms `bun run sidecar` must run before any `cargo build`/`cargo test` once
`externalBin` is configured — worth knowing for CI ordering, not a bug.

### Step 4: build sidecar, then run tests — verify pass

```
$ bun run sidecar
$ bash scripts/build-sidecar.sh
Building for production...
[0/4] Write sources
[1/4] Write swift-version--58304C5D6DBC2206.txt
[3/5] Compiling PhotobookEngine Handler.swift
[3/5] Write Objects.LinkFileList
[4/5] Linking PhotobookEngine
Build complete! (1.67s)
built src-tauri/binaries/photobook-engine-aarch64-apple-darwin
```

`rustc --print host-tuple` → `aarch64-apple-darwin`, which matches the copied binary's
suffix (`photobook-engine-aarch64-apple-darwin`) exactly.

```
$ cargo test --manifest-path src-tauri/Cargo.toml
   Compiling photobook-generator v0.1.0 ...
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.00s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/app_lib-2993071c651071ef)

running 3 tests
test protocol::tests::serializes_ping_request_as_single_line ... ok
test protocol::tests::deserializes_error_response ... ok
test protocol::tests::deserializes_pong_response ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs ...
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

   Doc-tests app_lib
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

3/3 passing, matches brief's expectation exactly.

Also confirmed a full `cargo build` compiles clean with no warnings:

```
$ cargo build --manifest-path src-tauri/Cargo.toml
   Compiling photobook-generator v0.1.0 ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
```

### Step 5: commit

Verified the built binary and Swift `.build` artifacts are properly gitignored before
staging:

```
$ git check-ignore -v src-tauri/binaries/photobook-engine-aarch64-apple-darwin
.gitignore:6:src-tauri/binaries	src-tauri/binaries/photobook-engine-aarch64-apple-darwin
$ git check-ignore -v sidecar/.build
.gitignore:10:sidecar/.build	sidecar/.build
```

```
$ git add -A && git status
Changes to be committed:
	new file:   scripts/build-sidecar.sh
	modified:   src-tauri/capabilities/default.json
	modified:   src-tauri/src/lib.rs
	new file:   src-tauri/src/protocol.rs
	new file:   src-tauri/src/sidecar.rs
	modified:   src-tauri/tauri.conf.json
```

```
$ git commit -m "feat(sidecar): wire Swift sidecar into Tauri with NDJSON transport"
[feat/phase-1-analysis-pipeline ba4865f] feat(sidecar): wire Swift sidecar into Tauri with NDJSON transport
 6 files changed, 174 insertions(+), 2 deletions(-)
 create mode 100755 scripts/build-sidecar.sh
 create mode 100644 src-tauri/src/protocol.rs
 create mode 100644 src-tauri/src/sidecar.rs
```

No `--no-verify` used, hooks (if any) ran normally.

## Deviations from the brief and why

1. **Wired `pub mod protocol;` into `lib.rs` before writing the implementation**, ahead
   of where Step 3 places it. The brief's Step 2 expects the bare test file (no types) to
   fail with "file not found for module 'protocol'" — but that message only occurs if
   `lib.rs` already references the module while the file itself is absent. Since I created
   `protocol.rs` first per Step 1 (as instructed) and only the module wiring was missing,
   the honest way to get a real failing-test signal was to add the `mod` line early, which
   produced compile errors for the missing types instead. This is functionally the same
   TDD gate (test can't pass because implementation doesn't exist yet) with a different
   concrete error message. No `pub mod sidecar;` was added until sidecar.rs existed and
   compiled, in the same spirit.
2. **Ran `bun run sidecar` before the final `cargo test`**, not simultaneously as
   `bun run sidecar && cargo test ...` in one line as the brief's Step 4 literally shows —
   functionally identical, just run as two separate commands so I could inspect each
   output distinctly for this report.

No other deviations. Did not touch `main.swift` or `Handler.swift` (not required, and the
brief explicitly said not to). Did not add anything from Task 14 (batching/timeout/respawn,
`Analyzed` variant) — `ping`/`pong` round trip is the full deliverable per the brief.

## Uncertainties / things worth flagging upstream

- I did not write an actual integration test that spawns the sidecar via a live
  `AppHandle` and calls `Sidecar::ping()` end-to-end (e.g. through `tauri dev` or an
  `#[tauri::command]`) — the brief's Step 4 only asks for the unit tests plus the build
  script's path output, and a full `AppHandle`-driven integration test wasn't specified
  or straightforward to construct headlessly. If a later task (or reviewer) wants proof
  of an actual live ping/pong through the shell plugin, that's still open — the plumbing
  (spawn/request/ping) is unit-tested for wire format only, not exercised against the
  real spawned process in this task.
- `tauri-build`'s compile-time validation of `externalBin` resource paths means any fresh
  checkout / CI run must execute `bun run sidecar` before the first `cargo build` or
  `cargo test` in `src-tauri`. This isn't documented anywhere yet (e.g. a root `bun run
  build` or CI script) — flagging in case Task 15 or a later CI task needs to sequence
  this correctly.

---

## Fix round (post-review)

Review came back spec-compliant with two Important findings. Both were fixed in this
task per the coordinator's direction (the `pub mod protocol;` deviation above was
explicitly approved, no change).

### Finding 1: build-ordering trap disclosed but not fixed

The problem: `externalBin` in `tauri.conf.json` activated `tauri-build`'s compile-time
resource-path check, but neither `beforeDevCommand` nor `beforeBuildCommand` built the
sidecar, so a fresh checkout's first `bun run dev`/`bun run build` would fail on a
confusing "resource path doesn't exist" error. The only record of the actual fix was
in this report file, which nothing downstream reads.

**Fix applied:**

1. `src-tauri/tauri.conf.json` — chained the sidecar build into both hooks:
   ```
   "beforeDevCommand": "bun run sidecar && bun run ui:dev",
   "beforeBuildCommand": "bun run sidecar && bun run generate",
   ```
2. Created `README.md` (none existed before) at the repo root, documenting:
   - `bun run dev` / `bun run build` build the sidecar automatically via the hooks above.
   - Direct `cargo build`/`cargo test` invocations inside `src-tauri/` bypass those hooks
     and must be preceded by `bun run sidecar`, with the exact error text to expect if
     skipped, so it's greppable.
   - A `Scripts` section listing all `bun run` entry points.

Verified `swift build` is a cheap no-op on an unchanged tree — confirmed while running
`bun run sidecar` a second time:

```
$ bun run sidecar
$ bash scripts/build-sidecar.sh
[0/1] Planning build
Building for production...
[0/2] Write swift-version--58304C5D6DBC2206.txt
Build complete! (0.15s)
built src-tauri/binaries/photobook-engine-aarch64-apple-darwin
```

0.15s — negligible on every `bun run dev`/`bun run build` invocation.

### Finding 2: no automatic verification of the cross-language round trip

The problem: all three original tests were Rust-side unit tests of Rust's own
serialization. Nothing exercised Swift's hand-rolled `Codable` implementation against
Rust's `#[serde(tag = "type", content = "data")]` decoding — a regression on either side
would only be caught by manually re-reading both files.

**Fix applied:** added `src-tauri/tests/sidecar_ping.rs`, a standard Cargo integration
test (separate test binary, not `#[cfg(test)]` inside the lib) that:

- Resolves the sidecar binary path via `env!("CARGO_MANIFEST_DIR")` +
  `env!("TAURI_ENV_TARGET_TRIPLE")` rather than a relative path or a hardcoded triple.
  `TAURI_ENV_TARGET_TRIPLE` is the same env var `tauri-build`'s build script sets via
  `cargo:rustc-env` (confirmed by reading
  `tauri-build-2.6.3/src/lib.rs:479-534` in the local cargo registry checkout) — it's the
  same value already used to validate the `externalBin` resource path exists, so the test
  can't drift from what the build actually validated.
- Spawns the real built binary directly with `std::process::Command` (piped
  stdin/stdout/stderr) — no `AppHandle`/`tauri_plugin_shell` involved, per the reviewer's
  guidance that the binary is just a subprocess.
- Writes one `Request { id: "integration-1", kind: Ping, paths: None }` line (via
  `app_lib::protocol` — the actual production types, not a hand-written JSON string).
- Reads the response line on a background thread and joins via
  `mpsc::Receiver::recv_timeout(Duration::from_secs(5))`, killing the child and panicking
  with a clear message on timeout, so a hung/crashed sidecar fails in ~5s instead of
  hanging CI.
- Asserts the decoded `Response.id` matches and `ResponseResult::Pong { version }` has a
  non-empty version.

Did not add `Drop` cleanup for `Sidecar`, did not stop hardcoding the triple in
`scripts/build-sidecar.sh`, and did not touch the `shell:allow-execute` scoping — all
explicitly deferred by the coordinator to later tasks.

### Verification commands and output

Rebuilt the sidecar (confirms `bun run sidecar` still produces the binary the new test
depends on) then ran the full Rust test suite:

```
$ bun run sidecar
$ bash scripts/build-sidecar.sh
[0/1] Planning build
Building for production...
[0/2] Write swift-version--58304C5D6DBC2206.txt
Build complete! (0.15s)
built src-tauri/binaries/photobook-engine-aarch64-apple-darwin

$ cargo test --manifest-path src-tauri/Cargo.toml
   Compiling photobook-generator v0.1.0 ...
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.52s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/app_lib-2993071c651071ef)

running 3 tests
test protocol::tests::serializes_ping_request_as_single_line ... ok
test protocol::tests::deserializes_error_response ... ok
test protocol::tests::deserializes_pong_response ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs (src-tauri/target/debug/deps/PhotobookGenerator-e32b27b9e37ce0e4)
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/sidecar_ping.rs (src-tauri/target/debug/deps/sidecar_ping-22e1d11cdfafb90e)

running 1 test
test sidecar_binary_responds_to_ping_with_matching_id_and_version ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s

   Doc-tests app_lib
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

4/4 tests passing (3 unit + 1 new integration test).

Confirmed `bun run dev` still starts end-to-end with the new chained hook — ran it in the
background, captured output, then killed the process:

```
$ bun run dev
$ tauri dev
     Running BeforeDevCommand (`bun run sidecar && bun run ui:dev`)
$ bash scripts/build-sidecar.sh
[0/1] Planning build
Building for production...
[0/2] Write swift-version--58304C5D6DBC2206.txt
Build complete! (0.14s)
built src-tauri/binaries/photobook-engine-aarch64-apple-darwin
$ nuxt dev
●  Nuxt 4.5.2 (with Nitro 2.13.4, Vite 8.2.1 and Vue 3.5.41)
  ➜ Local:    http://localhost:3000/
     Running DevCommand (`cargo  run --no-default-features --color always --`)
        Info Watching /Users/jiajingteoh/Documents/photobook-generator/src-tauri for changes...
   Compiling photobook-generator v0.1.0 ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.82s
     Running `target/debug/PhotobookGenerator`
```

Both the sidecar build and Nuxt dev server ran via the chained `beforeDevCommand`, Rust
compiled, and the Tauri binary launched successfully. Process was killed afterward
(`pkill -f PhotobookGenerator` / `nuxt dev` / `tauri dev` / `cargo  run`) — no residual
processes left running.

### Files touched in this round

- Modified: `src-tauri/tauri.conf.json` (chained `beforeDevCommand`/`beforeBuildCommand`)
- New: `README.md` (build-ordering note + scripts reference)
- New: `src-tauri/tests/sidecar_ping.rs` (cross-language integration test)

### Commit

```
$ git add README.md src-tauri/tauri.conf.json src-tauri/tests
$ git commit -m "fix(sidecar): auto-build sidecar on dev/build, add cross-language ping integration test"
```

(SHA recorded in the summary returned to the coordinator.)
