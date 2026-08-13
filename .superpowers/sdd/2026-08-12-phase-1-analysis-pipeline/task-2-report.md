# Task 2 Report: Swift package and NDJSON protocol loop

## Status: DONE

## What was implemented

Created a SwiftPM executable package `sidecar/` at the repo root containing:

- `sidecar/Package.swift` — swift-tools-version 6.0, `platforms: [.macOS(.v15)]`, one executable target `PhotobookEngine` and one test target `PhotobookEngineTests` depending on it.
- `sidecar/Sources/PhotobookEngine/Protocol.swift` — `RequestKind` enum (`ping`, `analyze`), `Request` struct, `PongResult`/`ErrorResult` structs, hand-rolled `Codable` `ResponseResult` enum with explicit `{"type": ..., "data": ...}` envelope, `Response` struct, `Engine.version = "0.1.0"`.
- `sidecar/Sources/PhotobookEngine/main.swift` — NDJSON read/write loop: reads lines from stdin via `readLine`, skips empty lines, decodes each as `Request`, emits `unknown`/`malformed request` error on decode failure, dispatches `ping` → `pong` response, `analyze` → stubbed `"not implemented"` error response (left as a stub deliberately — Task 9 implements real analysis). Encoder/decoder both set `dateEncodingStrategy`/`dateDecodingStrategy = .secondsSince1970` per the brief's note about avoiding the Swift-vs-Rust epoch mismatch, even though nothing encodes a `Date` yet.
- `sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift` — the three swift-testing tests given verbatim in the brief (`decodesPingRequest`, `encodesPongResponseWithMatchingId`, `encodesErrorResponse`).

All code was copied verbatim from the brief with no logic changes.

## Deviation from the brief (and why)

The brief's file list didn't mention `.gitignore`. Before `git add sidecar`, I noticed `swift build`/`swift test` produces `sidecar/.build/` (compiled objects, ~hundreds of files) and the repo's root `.gitignore` had no entry excluding it. I added one line, `sidecar/.build`, to the root `.gitignore` so build artifacts don't get committed. This is standard SwiftPM hygiene, not a change to the protocol/behavior the brief specifies. Verified with `git status --short` before staging that only the 4 intended source/test files (plus the `.gitignore` edit) were staged — no `.build` contents were added.

No other deviations. Did not touch `RequestKind.analyze` beyond the stub in the brief (per task instructions, Task 9 implements real analysis).

## Environment

- `swift --version`: `swift-driver version: 1.148.6 Apple Swift version 6.3.3 (swiftlang-6.3.3.1.3 clang-2100.1.1.101)`, target `arm64-apple-macosx28.0`.
- Confirmed branch was already `feat/phase-1-analysis-pipeline` and working tree was clean before starting.

## Commands run, in order, with actual output

### 1. Preliminary checks

```
$ git status && git branch --show-current && swift --version && ls
On branch feat/phase-1-analysis-pipeline
nothing to commit, working tree clean
feat/phase-1-analysis-pipeline
swift-driver version: 1.148.6 Apple Swift version 6.3.3 (swiftlang-6.3.3.1.3 clang-2100.1.1.101)
Target: arm64-apple-macosx28.0
app
bun.lock
docs
node_modules
nuxt.config.ts
package.json
sg.rs
src-tauri
tests
```

### 2. Step 1 — wrote the failing test

Created `sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift` with the exact contents from the brief (see file — not reproduced here since it's verbatim).

### 3. Step 2 — ran test to verify failure (before Package.swift/implementation existed)

```
$ swift test --package-path sidecar
error: Could not find Package.swift in this directory or any of its parent directories.
```

Exit code 1. This matches the brief's expected failure mode ("FAIL — no such package").

### 4. Step 3 — wrote the implementation

Created `sidecar/Package.swift`, `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/main.swift` — all verbatim from the brief.

### 5. Step 4 — ran tests to verify pass

```
$ swift test --package-path sidecar
Building for debugging...
[0/11] /Users/jiajingteoh/Documents/photobook-generator/sidecar/.build/arm64-apple-macosx/debug/PhotobookEnginePackageTests.derived/runner.swift
[1/11] Write sources
[4/11] Write PhotobookEngine-entitlement.plist
[5/11] Write swift-version--58304C5D6DBC2206.txt
[7/14] Compiling PhotobookEngine main.swift
[8/14] Compiling PhotobookEngine Protocol.swift
[9/14] Emitting module PhotobookEngine
[9/14] Write Objects.LinkFileList
[10/14] Linking PhotobookEngine
[11/14] Applying PhotobookEngine
[13/16] Emitting module PhotobookEngineTests
[14/16] Compiling PhotobookEngineTests ProtocolTests.swift
[15/18] Emitting module PhotobookEnginePackageTests
[16/18] Compiling PhotobookEnginePackageTests runner.swift
[16/18] Write Objects.LinkFileList
[17/18] Linking PhotobookEnginePackageTests
Build complete! (4.64s)
Test Suite 'All tests' started at 2026-08-12 17:07:20.400.
Test Suite 'All tests' passed at 2026-08-12 17:07:20.401.
	 Executed 0 tests, with 0 failures (0 unexpected) in 0.000 (0.001) seconds
◇ Test run started.
↳ Testing Library Version: 1902
↳ Target Platform: arm64e-apple-macos14.0
◇ Test encodesErrorResponse() started.
◇ Test decodesPingRequest() started.
◇ Test encodesPongResponseWithMatchingId() started.
✔ Test encodesErrorResponse() passed after 0.001 seconds.
✔ Test decodesPingRequest() passed after 0.001 seconds.
✔ Test encodesPongResponseWithMatchingId() passed after 0.001 seconds.
✔ Test run with 3 tests in 0 suites passed after 0.001 seconds.
```

All 3 tests pass, as expected.

### 6. Manual pipe check (brief's required verification)

```
$ echo '{"id":"a","kind":"ping"}' | swift run --package-path sidecar PhotobookEngine
[0/1] Planning build
Building for debugging...
[0/3] Write swift-version--58304C5D6DBC2206.txt
Build of product 'PhotobookEngine' complete! (0.11s)
{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}
```

Output matches the brief's expected value exactly: `{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}`.

### 7. Extra sanity checks (not required by brief, done for confidence)

Multi-line input covering `analyze` stub and a malformed line:

```
$ printf '{"id":"b","kind":"analyze","paths":["x.jpg"]}\nnot-json\n{"id":"c","kind":"ping"}\n' | swift run --package-path sidecar PhotobookEngine
Building for debugging...
[0/3] Write swift-version--58304C5D6DBC2206.txt
Build of product 'PhotobookEngine' complete! (0.10s)
{"result":{"type":"error","data":{"message":"not implemented"}},"id":"b"}
{"id":"unknown","result":{"data":{"message":"malformed request"},"type":"error"}}
{"id":"c","result":{"data":{"version":"0.1.0"},"type":"pong"}}
```

- `analyze` request correctly stubs out with `"not implemented"` — confirms the deliberate stub behavior is in place for Task 9 to fill in later.
- Malformed line (`not-json`) correctly falls back to `id: "unknown"` / `"malformed request"` and the loop continues processing subsequent lines (`c` still gets a pong). Confirms the loop doesn't crash or exit on bad input.

Architecture check:

```
$ file sidecar/.build/arm64-apple-macosx/debug/PhotobookEngine
sidecar/.build/arm64-apple-macosx/debug/PhotobookEngine: Mach-O 64-bit executable arm64

$ lipo -info sidecar/.build/arm64-apple-macosx/debug/PhotobookEngine
Non-fat file: sidecar/.build/arm64-apple-macosx/debug/PhotobookEngine is architecture: arm64
```

Confirms arm64-only build, consistent with the global constraint (no x86_64 slice, no universal binary).

### 8. Gitignore fix and staging check

```
$ cat .gitignore
node_modules
.nuxt
.output
dist
src-tauri/target
src-tauri/binaries
src-tauri/gen
.DS_Store
*.log
```
(no `.build` entry — added `sidecar/.build` line before staging)

```
$ git add sidecar .gitignore && git status --short
M  .gitignore
A  sidecar/Package.swift
A  sidecar/Sources/PhotobookEngine/Protocol.swift
A  sidecar/Sources/PhotobookEngine/main.swift
A  sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift
```

Confirms only the 4 intended files plus the gitignore edit were staged — `.build/` artifacts were correctly excluded.

### 9. Commit

```
$ git commit -m "feat(sidecar): add NDJSON protocol loop with ping"
[feat/phase-1-analysis-pipeline 3f77a28] feat(sidecar): add NDJSON protocol loop with ping
 5 files changed, 116 insertions(+)
 create mode 100644 sidecar/Package.swift
 create mode 100644 sidecar/Sources/PhotobookEngine/Protocol.swift
 create mode 100644 sidecar/Sources/PhotobookEngine/main.swift
 create mode 100644 sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift
```

Commit SHA: `3f77a28`

## Things I'm unsure about

- The brief's commit command was `git add sidecar` (no `.gitignore`). I staged both `sidecar` and the modified `.gitignore` in one commit rather than a separate `chore:` commit, on the judgment that a one-line gitignore addition supporting this same feature belongs with it rather than as a separate commit. If a strictly separate `chore: ignore sidecar build artifacts` commit is preferred, that's a two-command follow-up, but I judged bundling it as more consistent with "don't create needless churn/history noise" while a single feature commit is still in flight.
- I did not run `swift build -c release` (only debug via `swift test` / `swift run`) since the brief doesn't request a release build at this task boundary; Task 3 covers the build script / Rust spawn side where release-mode building presumably gets wired up.

---

## Fix report: review findings (id-loss on malformed requests, silent encode failures)

Code review on Task 2 came back spec/quality approved but flagged two Important findings, both gaps the original brief didn't anticipate, both landing in Task 3's path (Rust's id-based request/response correlation with timeouts). Both are now fixed.

### Finding 1 — malformed requests with a valid `id` but unrecognised `kind` lost their `id`

Original `main.swift` did `try? decoder.decode(Request.self, ...)` as an all-or-nothing decode; if `kind` was valid JSON but not a known `RequestKind` case, the whole decode failed and the error response went out with `id: "unknown"`. Task 3's Rust client correlates responses by `id`, so an `"unknown"` id is never matched and the caller burns its full timeout instead of failing fast.

**Fix:** added `struct RequestEnvelope: Decodable { let id: String }` — a lenient decode attempted only when the full `Request` decode fails, to recover just the `id` so it can be echoed back in the error response. Falls back to `"unknown"` only when even that fails (line isn't valid JSON at all, or has no `id` field).

### Finding 2 — `emit()` dropped encode failures silently

Original `emit()` was `guard let data = try? encoder.encode(response) else { return }` — on encode failure, no output at all, no diagnostic. To Rust this is indistinguishable from a hang.

**Fix:** `emit()` now uses `do/catch`. On failure it writes a diagnostic to **stderr** (never stdout — stdout is the protocol channel), then writes a minimal fallback `Response` carrying the same `id` and a generic `"internal encoding error"` message to stdout, so the caller is never left waiting. If even that fallback fails to encode (extremely unlikely, since it only contains plain strings), a last-resort hand-built JSON string with the id manually escaped is written directly.

### Required refactor: extracted `main.swift`'s logic into `Handler.swift`

The review asked for the line-handling logic to be extracted into a testable function (e.g. `handle(line:) -> Response`) if it wasn't reachable from tests otherwise. I did this, but hit an unexpected wall along the way that's worth flagging explicitly:

**First attempt** put `RequestEnvelope`, `emit(_:)`, and `handle(line:)` directly in `main.swift` alongside the top-level `while let line = readLine()` loop. Tests compiled, but every test that called `handle(line:)` **crashed the test process with signal 11 (segfault)** — reproduced twice, isolated via `swift test --filter` to confirm it was specifically the two tests invoking `handle(line:)`, not the others. Root cause: in a SwiftPM executable target, `main.swift` is compiled as a top-level-code file whose statements double as the module's synthesized entry point; functions defined alongside that top-level code are not safely callable via `@testable import` from a separate test binary — invoking them crashes the process.

**Fix:** moved `RequestEnvelope`, `encoder`, `decoder`, `emit(_:)`, and `handle(line:)` into a new file, `sidecar/Sources/PhotobookEngine/Handler.swift`, leaving `main.swift` reduced to just the three-line stdin loop. This is not on the brief's original file list, but was necessary — SwiftPM executable-target semantics require it for `handle(line:)` to be safely testable at all. `Protocol.swift` was left as pure Codable-type definitions (unchanged) to keep the file boundary clean: types in `Protocol.swift`, engine/dispatch logic in `Handler.swift`, process entry point in `main.swift`.

**Second-order issue:** moving `encoder.dateEncodingStrategy = .secondsSince1970` / `decoder.dateDecodingStrategy = .secondsSince1970` as free-standing mutation statements into `Handler.swift` failed to compile — `error: expressions are not allowed at the top level` — because only `main.swift` may contain top-level executable statements in an executable target; other files may only contain declarations. Fixed by building `encoder`/`decoder` via immediately-invoked closures (`let encoder: JSONEncoder = { let e = JSONEncoder(); e.dateEncodingStrategy = .secondsSince1970; return e }()`) instead of post-init mutation — this is a declaration (a global `let` with a closure-call initializer), not a bare statement, so it's permitted outside `main.swift`.

### Tests added

`sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift` — appended three tests:

```swift
@Test func unknownKindEchoesRequestId() throws {
    let line = #"{"id":"real-id","kind":"bogus"}"#
    let response = handle(line: line)
    #expect(response.id == "real-id")
    guard case .error(let err) = response.result else {
        Issue.record("expected an error result")
        return
    }
    #expect(err.message == "malformed request")
}

@Test func invalidJsonProducesUnknownId() throws {
    let response = handle(line: "not json at all")
    #expect(response.id == "unknown")
    guard case .error(let err) = response.result else {
        Issue.record("expected an error result")
        return
    }
    #expect(err.message == "malformed request")
}

@Test func envelopeDecodesGarbageKind() throws {
    let json = #"{"id":"e1","kind":"totally-not-a-kind"}"#
    let envelope = try JSONDecoder().decode(RequestEnvelope.self, from: Data(json.utf8))
    #expect(envelope.id == "e1")
}
```

Covering files:
- `sidecar/Sources/PhotobookEngine/Handler.swift` (new — `RequestEnvelope`, `encoder`/`decoder`, `emit(_:)`, `handle(line:)`)
- `sidecar/Sources/PhotobookEngine/main.swift` (reduced to the stdin loop only)
- `sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift` (three new tests appended)

### TDD sequence followed

1. Added the three tests above to `ProtocolTests.swift` while `handle`/`RequestEnvelope` didn't exist yet.
2. Ran `swift test --package-path sidecar` — confirmed compile failure (`cannot find 'handle' in scope`, `cannot find 'RequestEnvelope' in scope`).
3. Implemented the fix directly in `main.swift` first — tests compiled, but two of the new tests **crashed the test process** with signal 11. Diagnosed via `swift test --filter <name>` on each test individually to isolate which ones triggered it.
4. Refactored the logic into `Handler.swift`, hit the "expressions not allowed at top level" compile error on the encoder/decoder mutation statements, fixed via immediately-invoked closures.
5. Re-ran `swift test --package-path sidecar` — all 6 tests passed, no crash. Re-ran twice more to confirm stability (segfaults can be non-deterministic; this one reproduced consistently both before and after the fix, giving confidence the fix, not luck, resolved it).
6. Ran the manual pipe checks (below).

### Commands run, with actual output

**Step 2 — confirm tests fail to compile:**

```
$ swift test --package-path sidecar
...
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift:28:20: error: cannot find 'handle' in scope
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift:38:20: error: cannot find 'handle' in scope
/Users/jiajingteoh/Documents/photobook-generator/sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift:49:45: error: cannot find 'RequestEnvelope' in scope
error: fatalError
```

**First implementation attempt (logic in `main.swift`) — segfault, reproduced twice:**

```
$ swift test --package-path sidecar
...
error: Process '.../swiftpm-testing-helper ...' exited with unexpected signal code 11
...
◇ Test unknownKindEchoesRequestId() started.
◇ Test invalidJsonProducesUnknownId() started.
(no further output — process crashed)
```

Isolated with `--filter`:

```
$ swift test --package-path sidecar --filter unknownKindEchoesRequestId
... exited with unexpected signal code 11 ...
◇ Test unknownKindEchoesRequestId() started.
(crash, no pass/fail line)

$ swift test --package-path sidecar --filter invalidJsonProducesUnknownId
... exited with unexpected signal code 11 ...
◇ Test invalidJsonProducesUnknownId() started.
(crash, no pass/fail line)

$ swift test --package-path sidecar --filter envelopeDecodesGarbageKind
✔ Test envelopeDecodesGarbageKind() passed after 0.001 seconds.
(this one doesn't call handle(line:) — passed fine, confirming handle() itself was the trigger)
```

**After moving logic to `Handler.swift` and fixing the top-level-statement compile error — full pass:**

```
$ swift test --package-path sidecar
Building for debugging...
[0/7] Write sources
[1/7] Write swift-version--58304C5D6DBC2206.txt
[3/10] Compiling PhotobookEngine main.swift
[4/10] Compiling PhotobookEngine Handler.swift
[5/10] Emitting module PhotobookEngine
[5/10] Write Objects.LinkFileList
[6/10] Linking PhotobookEngine
[7/10] Applying PhotobookEngine
[9/12] Compiling PhotobookEngineTests ProtocolTests.swift
[10/12] Emitting module PhotobookEngineTests
[10/12] Write Objects.LinkFileList
[11/12] Linking PhotobookEnginePackageTests
Build complete! (0.90s)
Test Suite 'All tests' started at 2026-08-12 17:13:09.098.
Test Suite 'All tests' passed at 2026-08-12 17:13:09.099.
	 Executed 0 tests, with 0 failures (0 unexpected) in 0.000 (0.001) seconds
◇ Test run started.
↳ Testing Library Version: 1902
↳ Target Platform: arm64e-apple-macos14.0
◇ Test encodesPongResponseWithMatchingId() started.
◇ Test decodesPingRequest() started.
◇ Test encodesErrorResponse() started.
◇ Test envelopeDecodesGarbageKind() started.
◇ Test unknownKindEchoesRequestId() started.
◇ Test invalidJsonProducesUnknownId() started.
✔ Test decodesPingRequest() passed after 0.001 seconds.
✔ Test invalidJsonProducesUnknownId() passed after 0.001 seconds.
✔ Test encodesPongResponseWithMatchingId() passed after 0.001 seconds.
✔ Test encodesErrorResponse() passed after 0.001 seconds.
✔ Test unknownKindEchoesRequestId() passed after 0.001 seconds.
✔ Test envelopeDecodesGarbageKind() passed after 0.001 seconds.
✔ Test run with 6 tests in 0 suites passed after 0.001 seconds.
```

Ran twice more back-to-back — both times all 6 tests passed with no crash, confirming stability.

**Manual pipe check — baseline ping still works:**

```
$ echo '{"id":"a","kind":"ping"}' | swift run --package-path sidecar PhotobookEngine
{"id":"a","result":{"data":{"version":"0.1.0"},"type":"pong"}}
```

**Manual pipe check — Finding 1 fix (unknown kind echoes real id; invalid JSON still falls back to "unknown"; loop continues after both):**

```
$ printf '{"id":"real-id","kind":"bogus"}\nnot json at all\n{"id":"c","kind":"ping"}\n' | swift run --package-path sidecar PhotobookEngine
{"result":{"type":"error","data":{"message":"malformed request"}},"id":"real-id"}
{"result":{"type":"error","data":{"message":"malformed request"}},"id":"unknown"}
{"result":{"type":"pong","data":{"version":"0.1.0"}},"id":"c"}
```

`id: "real-id"` (from the unrecognised-kind line) is now correctly echoed instead of `"unknown"`; the genuinely-not-JSON line still correctly falls back to `"unknown"`.

**Confirmed stdout stays protocol-clean (stderr redirected away, same input):**

```
$ printf '{"id":"real-id","kind":"bogus"}\nnot json at all\n{"id":"c","kind":"ping"}\n' | swift run --package-path sidecar PhotobookEngine 2>/dev/null
{"result":{"type":"error","data":{"message":"malformed request"}},"id":"real-id"}
{"result":{"type":"error","data":{"message":"malformed request"}},"id":"unknown"}
{"result":{"type":"pong","data":{"version":"0.1.0"}},"id":"c"}
```

Identical 3 response lines to the previous check, but `swift run`'s "Building for debugging..." status lines are gone — confirming those go to stderr, not stdout, and stdout carries only protocol JSON lines.

I did not specifically force an encoder failure to exercise Finding 2's fallback path end-to-end (constructing a `Response` value that fails to encode under `JSONEncoder` is not straightforward with the current `Codable` types — none of `Response`'s fields can hold a non-encodable value), so that branch is exercised by inspection/code-review confidence rather than a runtime reproduction. This is the one thing I'm not fully certain about: the `emit()` catch block, the secondary fallback encode, and the last-resort hand-built-JSON branch are all correctness-reviewed but not test-covered or manually triggered, since swift-testing has no built-in way to inject an encoder failure without adding a seam (e.g. a protocol-wrapped encoder) that the brief and review didn't ask for.

### Commit

```
$ git add sidecar
$ git commit -m "fix(sidecar): preserve request id on decode failure and harden emit against encode errors"
```

Commit SHA: `6205658`
