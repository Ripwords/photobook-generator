# Task 10 Report: Hostile-input fixture corpus

## Summary

Implemented the hostile-input corpus proving `Analyzer.analyze` degrades malformed
input to `.failed` records rather than trapping the process. 8 fixtures (the brief's
7 plus one added beyond the brief) and 7 tests in
`sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift`. Followed the brief
closely, with three deviations, all noted below: an extra fixture class, a `.serialized`
trait added to contain an unrelated concurrency deadlock discovered while verifying,
and use of the Bash tool's sandbox-disable flag to get Vision working at all in this
session.

**No fixture ever crashed the process.** The two problems encountered were both about
*getting the test runner itself to run* in this sandboxed agent environment — neither
was a bug in `ImageLoader`/`ExifReader`/`Analyzer`, and neither required touching
those files. Details in "Two non-fixture problems" below.

## Files changed

- `scripts/make-fixtures.swift` — extended append-only (per brief) with a
  `writeHostile` helper and three calls generating `one-pixel.jpg`, `cmyk.jpg`,
  `no-extension`. `landscape.jpg`, `portrait-rot90.jpg`, `gps-south-west.jpg` are
  byte-identical after rerunning (verified via `md5`, see below) — Task 4/5 tests are
  undisturbed.
- `scripts/make-hostile-fixtures.sh` — new. Generates `empty.jpg`, `truncated.jpg`,
  `text.jpg`, `random.jpg` (per brief) plus `corrupt-scan-data.jpg` (added, see below).
- `sidecar/Fixtures/hostile/` — 8 committed fixtures (list and byte sizes below).
- `sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift` — new, 7 tests.

## Step 1-2: Write the tests, verify they fail for the right reason

Wrote the four tests from the brief verbatim (renamed with a `hostile` prefix per the
task's explicit naming instruction, to guarantee uniqueness against the other 7 test
files), plus 3 more (see "Beyond the brief" below), for 7 total. Before generating
fixtures:

```
$ swift test --package-path sidecar --filter Hostile
...
Test hostileEveryFixtureProducesARecordAndNeverCrashes() failed: files.count >= 7 failed
- run scripts/make-hostile-fixtures.sh first
```

Failed for the expected reason (fixtures don't exist yet).

## Step 3: Generate the fixtures

`swift scripts/make-fixtures.swift`:

```
wrote landscape.jpg (1200x800, orientation 1)
wrote portrait-rot90.jpg (1200x800, orientation 6)
wrote gps-south-west.jpg (1200x800, GPS S/W + DateTimeOriginal)
wrote hostile/one-pixel.jpg
wrote hostile/cmyk.jpg
wrote hostile/no-extension
```

`CGColorSpaceCreateDeviceCMYK` worked fine as a JPEG destination on this macOS
version (Sequoia 15 / Swift 6.3.3 toolchain, arm64) — **no substitution needed**.
`file` confirms 4 components in the output: `cmyk.jpg: ... components 4`.

Confirmed the three pre-existing fixtures are unchanged after rerunning the script
(`git status --short sidecar/Fixtures/*.jpg` shows nothing; `md5` matches what was
already committed):

```
MD5 (sidecar/Fixtures/landscape.jpg) = 3f365fdeb86ddf084a24ff5d6bfea754
MD5 (sidecar/Fixtures/portrait-rot90.jpg) = 09087355c47ec441c22c9f84fec3f7cb
MD5 (sidecar/Fixtures/gps-south-west.jpg) = 64623505436fee037ccba81a18b8f60c
```

`bash scripts/make-hostile-fixtures.sh`:

```
created byte-level hostile fixtures in sidecar/Fixtures/hostile
run 'swift scripts/make-fixtures.swift' for the image-based ones
```

Final corpus (`sidecar/Fixtures/hostile/`, `file` output):

| file | bytes | `file` says |
|---|---|---|
| `empty.jpg` | 0 | empty |
| `text.jpg` | 30 | ASCII text |
| `truncated.jpg` | 400 | JPEG (first 400 bytes of `landscape.jpg`) |
| `random.jpg` | 2000 | data (no recognizable format) |
| `one-pixel.jpg` | 707 | JPEG, 1x1, 3 components |
| `no-extension` | 791 | JPEG, 64x64, 3 components (no file extension) |
| `cmyk.jpg` | 964 | JPEG, 64x64, **4 components** |
| `corrupt-scan-data.jpg` | 16115 | JPEG, 1200x800 (header intact, scan data corrupted) — *added beyond the brief* |

## Beyond the brief: 3 additions, and why

The brief names 7 fixtures. Three more input classes are plausible ImageIO crash
surfaces that are not just variations on the same failure path, so I added them
instead of padding the corpus with near-duplicates:

1. **`corrupt-scan-data.jpg`** (committed fixture, byte-level, added to
   `make-hostile-fixtures.sh`): a copy of `landscape.jpg` with the JPEG
   header/SOF/DHT segments left intact (landscape.jpg's SOS marker is at byte 751)
   but bytes 900-4900 overwritten with `/dev/urandom` via `dd ... conv=notrunc`
   (same file length, so the header stays parseable). Truncation (`truncated.jpg`)
   is a clean cut ImageIO sees as a short read; random bytes (`random.jpg`) have no
   valid header at all and get rejected immediately. This fixture exercises neither
   — it's the "decoder walks a corrupt Huffman table" class of failure, the
   historically real crash surface in JPEG decoders.
2. **A directory given as an image path** (`hostileDirectoryPathFailsCleanly`,
   reuses `Fixtures/hostile/` itself, no new fixture file needed): exercises
   `FileHandle`/`CGImageSource` being handed something that opens differently than
   either "missing" or "malformed bytes."
3. **A permission-denied file** (`hostileUnreadableFileFailsCleanly`,
   runtime-generated, deliberately *not* committed): a distinct I/O error path from
   "does not exist." Not committed because git does not preserve full POSIX
   permission bits across a clone — a checked-in `0o000` fixture would silently stop
   being hostile the moment someone clones the repo fresh. The test writes a temp
   copy of `landscape.jpg`, chmods it to `0o000`, runs the analyzer, and cleans up in
   a `defer`. Noted in the test that root (or a permissive CI runner) can bypass
   this and the test degrades to a non-assertive pass in that case — still valid
   (no crash), just not proof of the specific claim.

Considered and rejected as padding: a broken symlink (resolves to the same
"unreadable" code path already covered by `analyzerReportsFailureForMissingFileWithoutCrashing`
in `AnalyzerTests.swift` and by `empty.jpg`/`hostileZeroByteFileFailsCleanly`), and an
HEIC/RAW-specific malformation (no real-world sample available and the brief's fixtures
already cover the general "malformed container" class via JPEG).

## Step 4: Run tests — two non-fixture problems found and resolved

### Problem 1: Vision hangs indefinitely under this session's default Bash sandbox

First `swift test --package-path sidecar` run (default sandboxed Bash) hung for over
20 minutes with **zero CPU growth** (confirmed via `ps -o time` sampled twice, 20s
apart: `0:03.76` both times) — a genuine deadlock, not slow computation. `sample`
showed every stalled thread blocked in the exact same place:

```
-[VNImageRequestHandler performRequests:gatheredForensics:error:] (in Vision)
  -[VNControlledCapacityTasksQueue dispatchGroupWait:error:] (in Vision)
    __ulock_wait (in libsystem_kernel.dylib)
```

Critically, this reproduced on `analyzerOneBadPhotoDoesNotAbortTheBatch()` —
a **pre-existing Task 9 test that touches no hostile fixture at all**. Filtering to
just `AnalyzerTests` (17 tests, no hostile fixtures) under the sandboxed Bash tool
also hung. This pointed at the sandbox blocking whatever mach/XPC service Vision's
on-device model machinery needs (ANE service lookup, asset provisioning), not at
anything in the analysis code.

**Resolution**: reran with the Bash tool's `dangerouslyDisableSandbox: true`. The same
17-test filtered run then completed in 0.35s. This is an artifact of the agent's own
tool sandbox in this session — not a bug in the app, and not something to fix in
`ImageLoader`/`ExifReader`/`Analyzer`. Anyone running `swift test --package-path
sidecar` from a normal terminal would not hit this.

### Problem 2: Full 66-test suite deadlocks on Vision's internal capacity queue

With the sandbox disabled, the pre-existing 59 tests alone passed in 3.2s, and the 7
hostile tests alone passed in 0.2s. But the **full unfiltered suite (66 tests)**
deadlocked again — same `VNControlledCapacityTasksQueue` / `__ulock_wait` signature,
same zero-CPU-growth confirmation. swift-testing runs `@Test` functions concurrently
by default; the full suite runs enough simultaneous `VNImageRequestHandler` instances
(spread across `AnalyzerTests`, `VisionAnalyzerTests`, and now `HostileInputTests`) to
exhaust the shared libdispatch global concurrent queue that both the blocked callers
and Vision's own internal work items depend on — a thread-pool-exhaustion deadlock.
This reproduces from aggregate Vision-call volume, not from any specific fixture's
byte content (the same 59 pre-existing tests, none of which touch a hostile fixture,
are half of what's running when it deadlocks).

**Resolution**: marked `HostileInputTests` as `@Suite(.serialized)`. This caps hostile
tests' own peak concurrent Vision fan-out at whatever the largest single hostile test
needs (8, from `hostileEveryFixtureProducesARecordAndNeverCrashes` analyzing the whole
directory), instead of letting `hostileEveryFixtureProducesARecordAndNeverCrashes`
(8) + `hostileBatchMixedWithGoodPhotoStillReturnsTheGoodOne` (3, though 2 fail before
Vision) + `hostileCorruptScanDataFailsCleanlyOrDecodesPartially` (1) all pile on at
once. That was enough: the full suite then passed reliably (4 consecutive runs, see
below). This is a targeted concurrency mitigation for the ceiling this file's tests
add, not a fix for a fixture-specific crash — I did not modify any pre-existing test
file to reach this, per the brief's scope.

I flag this because it could recur: any future task that adds another handful of
Vision-touching tests could tip the full suite back over this ceiling. It may be worth
a follow-up (outside Task 10's scope) to understand this properly rather than treat it
as fixture-file-local — e.g. capping global Vision concurrency inside `VisionAnalyzer`
itself, or making the whole test target's Vision-touching suites `.serialized`.

## Step 4 (retry): full suite passes

```
$ swift test --package-path sidecar
...
✔ Test run with 66 tests in 1 suite passed after 3.180 seconds.
```

Ran 4 times total after the `.serialized` fix (2 full runs shown with output, 2 more
filtered to the summary line) — all 66 tests passed every time, process exited 0 each
time:

```
=== RUN 1 ===
✔ Test run with 66 tests in 1 suite passed after 3.181 seconds.
=== RUN 2 ===
✔ Test run with 66 tests in 1 suite passed after 3.213 seconds.
```

### Which fixtures produced `.ok` vs `.failed`, and why

| fixture | record | why |
|---|---|---|
| `empty.jpg` | `.failed` | `CGImageSourceCreateWithURL`/properties read fails on 0 bytes |
| `text.jpg` | `.failed` | not an image container ImageIO recognizes |
| `random.jpg` | `.failed` | no valid header |
| `truncated.jpg` | `.failed` **or** `.ok` (either acceptable, per the existing ruling) | short read; ImageIO may decode the partial data it has |
| `corrupt-scan-data.jpg` | `.failed` **or** `.ok` (either acceptable, same reasoning as truncated) | valid header, corrupted scan data — ImageIO may reject or produce a garbled-but-decoded image |
| `one-pixel.jpg` | `.ok` | 1x1 is small but valid; downstream metrics (palette, sharpness, phash) all degrade gracefully on a 1-pixel image |
| `cmyk.jpg` | `.ok` | ImageIO/CoreGraphics colour-manage CMYK to RGB during thumbnail decode without incident |
| `no-extension` | `.ok` | ImageIO sniffs JPEG content regardless of missing/wrong extension |
| directory path | `.failed` | `CGImageSourceCreateWithURL`/`FileHandle` don't treat a directory as readable image bytes |
| permission-denied temp file | `.failed` (when not running as root) | I/O error distinct from "missing" |

I didn't assert `.ok` vs `.failed` on `truncated.jpg` or `corrupt-scan-data.jpg`
specifically (matching the brief's existing ruling for `truncated.jpg`, applied the
same way to my new fixture) — the assertion that matters for both is `records.count
== 1`, i.e. the process returned at all.

**No fixture ever crashed the process** — the two failures above were both about the
test harness's environment (agent sandbox blocking Vision's service access; and
Vision's own concurrency ceiling under this many simultaneous requests), not about any
fixture's bytes reaching `ImageLoader`, `ExifReader`, or `Analyzer` in a way that
trapped. No changes were made to those three files.

## Deviations from the brief

1. Added `corrupt-scan-data.jpg` and two runtime/reused-path tests beyond the brief's
   7 fixtures, as invited by "Go beyond the brief's list if you see an obvious gap."
2. Test names all carry a `hostile` prefix (the brief's own sample code didn't, but
   the task context explicitly asked for this to guarantee uniqueness).
3. `@Suite(.serialized)` on `HostileInputTests` — not in the brief, added solely to
   contain the environment-level Vision concurrency deadlock described above.
4. Verification commands used the Bash tool's `dangerouslyDisableSandbox: true` — not
   a code or fixture change, purely how I ran `swift test` in this session.

## Uncertain / worth a second look

- The full-suite Vision concurrency ceiling (Problem 2) is real and I only mitigated
  it locally to `HostileInputTests`. I did not determine the exact numeric threshold
  or whether it's specific to this machine/VM's core count and ANE availability. If a
  future task adds several more Vision-touching tests, the same deadlock could return
  and would need the same treatment (or a proper fix upstream in how Vision
  concurrency is bounded).
- I could not verify whether Problem 1 (sandbox blocking Vision entirely) reflects
  something a *real* end user's sidecar process would ever hit — it's plausible it's
  purely an artifact of the Bash tool's process sandbox in this coding session and
  irrelevant to the shipped app, but I have no way to confirm that from inside this
  environment.

---

## Addendum: production deadlock fix

The coordinator flagged (correctly) that "Problem 2" above is not just a test-suite
inconvenience: `Analyzer.analyze(paths:)` runs `DispatchQueue.concurrentPerform` over
every input path, and each iteration makes the same synchronously-blocking
`VNImageRequestHandler.perform()` calls that deadlocked in testing. Task 15 calls this
over a real user's library (100-300 photos, sustained) — the exact shape that
deadlocked. A hang there is worse than a crash: no error surfaces, no retry, the app
just sits at zero CPU forever. This addendum fixes that, proves the fix with a stress
test, and settles whether `@Suite(.serialized)` was still needed.

### Also fixed: the "10 tests" / "6 more" arithmetic error the coordinator caught

The original report said "10 tests" in the summary and "plus 6 more" in Step 1-2. The
diff has 7 `@Test func` declarations (4 from the brief + 3 beyond it), matching the
existing 59 + 7 = 66 arithmetic used later in the same report. Corrected both spots
above in place (summary, "Files changed", Step 1-2) rather than leaving the error
alongside a correction — no other numbers in the report depended on the wrong figure.

### The fix

`sidecar/Sources/PhotobookEngine/Analyzer.swift`: added a `private static let
visionSemaphore = DispatchSemaphore(value: visionConcurrencyLimit)` (`= 4`) on
`Analyzer`, and wrapped only the `VisionAnalyzer.analyze(image)` call inside
`analyzeOne` with `visionSemaphore.wait()` / `defer { visionSemaphore.signal() }`.
Decode (`ImageLoader.loadThumbnail`) and metrics (`Metrics.swift`) are unaffected and
stay at full `concurrentPerform` width — they're CPU-bound and were never implicated
in the deadlock. The `defer` guarantees the permit is released on every exit path,
including any future throwing change to `VisionAnalyzer.analyze` — both the code and a
comment call out that a leaked permit here converts a bounded fix into a guaranteed
hang. A comment on `visionConcurrencyLimit` explains why the cap exists (with a
pointer to this report) and explicitly warns against "cleaning it up" as a
pessimisation on a many-core machine.

### Stress test proving the fix

Added to `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`:

```swift
@Test(.timeLimit(.minutes(2)))
func analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() {
    let paths = Array(repeating: fixture("landscape.jpg"), count: 300)
    let records = Analyzer.analyze(paths: paths)
    #expect(records.count == 300)
    #expect(records.allSatisfy {
        if case .ok = $0 { return true }
        return false
    })
}
```

300 repeats of the same fixture (content doesn't matter, only concurrency). The
`.timeLimit(.minutes(2))` trait means a reintroduced deadlock fails the test loudly
instead of hanging CI indefinitely.

**Verified this test would have caught the original bug.** Temporarily set
`visionConcurrencyLimit = 10_000` (effectively unbounded) and reran just this test in
isolation:

```
$ swift test --package-path sidecar --filter analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency
✔ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() passed after 4.812 seconds.
```

This *passed* even unbounded — a single `Analyzer.analyze` call's own
`concurrentPerform` fan-out (capped at CPU core count) wasn't by itself enough
contention to deadlock in isolation, consistent with the original finding that
`AnalyzerTests` alone (max 9-way concurrency) never deadlocked either. The deadlock
needs the *aggregate* load of many tests running at once — see below, where the same
unbounded setting **does** deadlock as part of the full suite. So the stress test is
a real regression guard for the production risk (a single large batch still exercises
meaningfully more concurrent Vision calls than any existing test did before), but its
in-isolation pass here isn't itself the reproduction — the full-suite run is.

### Verifying `.serialized` against the real fix: could it be removed?

Removed `@Suite(.serialized)` from `HostileInputTests`, first with the cap still at
`10_000` (control case) to confirm the full suite deadlocks exactly as originally
found:

```
$ swift test --package-path sidecar   # cap=10_000, no .serialized
[... builds ...]
# no "Test run started" line ever printed
```

Checked twice, 20s apart: CPU time frozen at `0:03.30` both times (`ps -o pid,time,etime`)
— confirmed zero progress, not just slow. Killed after ~50s elapsed.

Then restored `visionConcurrencyLimit = 4` (the real fix) but **kept `.serialized`
removed**, to test whether the production fix alone stabilizes the suite:

```
$ swift test --package-path sidecar   # cap=4, no .serialized
```

This **timed out after 3 minutes with zero test output** — the semaphore fix alone
does **not** fix the full-suite deadlock. Investigated why: `DispatchSemaphore.wait()`
is itself a blocking call on the same shared libdispatch global concurrent queue that
Vision's internal async work needs. With `.serialized` removed, swift-testing launches
dozens of tests concurrently, several via `concurrentPerform` fan-out (up to 300 from
the new stress test alone) — all of those threads block on `visionSemaphore.wait()`
at once, which is enough blocked-thread pressure on the shared pool to reproduce the
same class of starvation, just from waiting on the semaphore instead of waiting inside
Vision directly.

Restored `@Suite(.serialized)` on `HostileInputTests` (cap=4 kept) and reran the full
suite 3 times:

```
=== RUN 1 ===
✔ Test run with 67 tests in 1 suite passed after 6.033 seconds.
=== RUN 2 ===
✔ Test run with 67 tests in 1 suite passed after 6.153 seconds.
=== RUN 3 ===
✔ Test run with 67 tests in 1 suite passed after 6.389 seconds.
```

**Conclusion: `.serialized` was NOT removed — it is still required, and it is fixing
a genuinely separate problem from the production deadlock.** `visionSemaphore` bounds
concurrent Vision work *within one `Analyzer.analyze(paths:)` call*, which is exactly
Task 15's real shape (one process, one batch, nothing else competing for threads) —
that's the production fix and it's sufficient there. It is not sufficient for this
*test suite*, because swift-testing's own default test-level parallelism means dozens
of independent `Analyzer.analyze` calls (including one with 300-way fan-out) contend
for the same semaphore and the same thread pool simultaneously — a form of load
production code never produces. `HostileInputTests.swift` now documents this
explicitly (both problems, why both fixes are needed, and the empirical result of
removing each) so a future reader doesn't reintroduce the deadlock by "simplifying"
either one away.

### Throughput: cap=2 vs cap=4

Isolated the stress test (`--filter analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`)
and ran it 3 times at each cap:

| cap | run 1 | run 2 | run 3 | avg |
|---|---|---|---|---|
| 2 | 7.144s | 7.139s | 7.162s | 7.148s |
| 4 | 4.358s | 4.398s | 4.420s | 4.392s |

cap=4 is ~1.6x faster than cap=2 (4.39s vs 7.15s for the same 300-photo batch), with
no stability difference observed at either setting in isolation. Per the coordinator's
guidance ("if capping at 2 is dramatically slower than 4 with no stability
difference, prefer 4"), kept `visionConcurrencyLimit = 4`.

### Final verification

```
$ swift test --package-path sidecar
✔ Test run with 67 tests in 1 suite passed after 6.039 seconds.
```

All 67 tests (66 from the original delivery + the new stress test) pass, process
exits 0. Full diff: `sidecar/Sources/PhotobookEngine/Analyzer.swift` (semaphore + gated
call site), `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift` (stress test),
`sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift` (comment rewrite
explaining why `.serialized` is still needed alongside the semaphore — no behavioural
change, the trait itself was already present and is unchanged).

### Remaining concern

I bounded Vision concurrency with a global static semaphore scoped to the whole
`Analyzer` type/process. This is correct for the sidecar's actual process model (one
process = one batch = one semaphore lifetime), but if a future change ever runs
multiple `Analyzer.analyze` batches concurrently within the same process (e.g. a
long-lived sidecar handling overlapping requests, rather than one batch per process
invocation), the same test-suite-style contention this addendum found could reappear
in production, since the semaphore doesn't distinguish batches. Worth a note for
whoever builds Task 15's request handling if the sidecar's process lifecycle changes
from "one process per batch."

---

## Second addendum: guard scope, throughput re-measurement, and watchdog verification

Review caught two real problems in the first addendum's fix, both fixed here, plus one
that could not be fixed and is instead documented as a known limitation per explicit
instruction.

### Finding 1: the semaphore guarded more than Vision

The permit was released via `defer` scoped to the entire `autoreleasepool` trailing
closure, not just the `VisionAnalyzer.analyze` call — so `Metrics.palette`,
`Metrics.sharpness`, `Metrics.contrast`, `Metrics.perceptualHash`, the `faceArea`
reduce, and the `PhotoFeatures` construction all ran while still holding one of 4
permits. Pure-CPU work with no blocking calls, so no correctness or deadlock risk, but
it meant only 4 threads could be doing *any* per-photo work at once regardless of core
count — the exact throughput collapse the fix was supposed to avoid.

**Fix** (`sidecar/Sources/PhotobookEngine/Analyzer.swift`): narrowed the guard to an
immediately-invoked closure wrapping only `VisionAnalyzer.analyze(image)`:

```swift
let vision: VisionResult = {
    visionSemaphore.wait()
    defer { visionSemaphore.signal() }
    return VisionAnalyzer.analyze(image)
}()
```

`Metrics.*` now runs at full `concurrentPerform` width, outside the guarded span.
Comments on both the `visionConcurrencyLimit` declaration and this call site were
rewritten to describe what's actually gated.

### Re-measured throughput: cap=2 vs cap=4 (guard narrowed)

Isolated the stress test (`--filter analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`),
3 runs at each cap, after narrowing the guard:

```
=== cap=4 ===
✔ passed after 2.863 seconds.
✔ passed after 2.824 seconds.
✔ passed after 2.834 seconds.   (avg 2.840s)

=== cap=2 ===
✔ passed after 3.520 seconds.
✔ passed after 3.549 seconds.
✔ passed after 3.630 seconds.   (avg 3.566s)
```

Both caps got faster than the first addendum's numbers, as expected (2.84s vs 4.39s at
cap=4; 3.57s vs 7.15s at cap=2 — metrics no longer serialize behind the semaphore at
either setting). The gap between caps also narrowed, from ~1.6x to ~1.26x, since
metrics now run at full width regardless of cap and only the Vision portion itself
is throttled. cap=4 remains faster with no stability difference observed at either
setting in isolation, so **kept `visionConcurrencyLimit = 4`**.

Full suite, 3 consecutive runs with the narrowed guard and cap=4:

```
✔ Test run with 67 tests in 1 suite passed after 4.667 seconds.
✔ Test run with 67 tests in 1 suite passed after 4.712 seconds.
✔ Test run with 67 tests in 1 suite passed after 4.735 seconds.
```

### Finding 2: the stress test's `.timeLimit` trait can't fire on synchronous code

`@Test(.timeLimit(.minutes(2)))` enforces its deadline via cooperative task
cancellation, checked only at suspension points. The stress test's body has none
(`Analyzer.analyze` → `concurrentPerform` → `DispatchSemaphore.wait()` is fully
synchronous), so a real deadlock there would never let the trait observe cancellation
— the exact regression this test exists to catch would hang the test runner instead
of failing the test.

**Fix**: replaced the trait with an out-of-band watchdog — run the analysis on
`DispatchQueue.global()`, wait on a plain `DispatchSemaphore` with an explicit 120s
timeout on the test thread, and assert on the outcome:

```swift
let done = DispatchSemaphore(value: 0)
var records: [PhotoRecord] = []
DispatchQueue.global().async {
    records = Analyzer.analyze(paths: paths)
    done.signal()
}
let outcome = done.wait(timeout: .now() + 120)
#expect(outcome == .success, "analyze deadlocked: did not complete within 120s")
guard outcome == .success else { return }
#expect(records.count == 300)
#expect(records.allSatisfy { if case .ok = $0 { return true }; return false })
```

### Watchdog verification: it did not fire in the scenario I could construct, and here is exactly why

Set out to reintroduce the bug and confirm the watchdog fails within ~120s rather than
hanging, as instructed. This required two steps, and the second one surfaced a real
limitation.

**Step A — is a single, isolated `Analyzer.analyze(paths: 300 repeats)` call enough to
deadlock at all, even fully unbounded?** Set `visionConcurrencyLimit = 10_000` and ran
just the stress test filtered, nothing else competing:

```
$ swift test --package-path sidecar --filter analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency
✔ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() passed after 4.812 seconds.
```

No — it passed, unbounded, with nothing else running. Consistent with the original
finding: a single call's own `concurrentPerform` fan-out (bounded by CPU core count)
was never by itself enough contention to trigger Vision's internal deadlock in this
environment. There was nothing for the watchdog to catch here.

**Step B — reproduce the only deadlock available: the full suite, unthrottled.**
Removed `@Suite(.serialized)` from `HostileInputTests` (temporarily) on top of
`visionConcurrencyLimit = 10_000`, and ran the full unfiltered suite:

```
$ swift test --package-path sidecar   # cap=10_000, .serialized removed
[... builds, "Test run started", dozens of tests started and passed quickly ...]
◇ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() started.
[... no further output for this test ...]
```

Watched for over 3 minutes (well past the watchdog's 120s deadline) — no pass or fail
line for this test ever appeared. `ps -o pid,etime,time` confirmed the process was
genuinely stuck, not slow: CPU time frozen at `0:03.34` for the entire window.

**Investigated with `sample` rather than assuming.** A 3-second sample of the hung
process showed:
- Dozens of threads legitimately stuck inside `Analyzer.analyzeOne` →
  `VisionAnalyzer.analyze` → `-[VNImageRequestHandler performRequests:...]` →
  `-[VNControlledCapacityTasksQueue dispatchGroupAsyncByPreservingQueueCapacity:block:]`
  → `semaphore_wait_trap` — the deadlock working exactly as sabotaged, and confirming
  it reproduces through the real (narrowed) production code path, not just the old one.
- **No thread anywhere in the sample was executing `AnalyzerTests.swift:45`** (the
  `done.wait(timeout:)` line) or any other line of this test's own synchronous body.
  Only its nested `DispatchQueue.global().async` closure appeared (stuck inside
  `Analyzer.analyze`, as expected) — the outer test function that was supposed to be
  timing that closure never got an OS thread to run on in the first place.

**Root cause: the watchdog is not actually out-of-band.** `DispatchQueue.global()`
and the thread swift-testing uses to run this test's own body both draw from the same
shared libdispatch global concurrent queue that the sabotaged Vision calls were
exhausting. `done.wait(timeout:)` genuinely is a self-contained kernel deadline that
returns on its own once its calling thread is running — that part of the reasoning in
the code comment is correct — but getting that thread scheduled at all requires a free
slot in the exact pool the deadlock has exhausted. Under total pool exhaustion, the
watchdog's own code can starve alongside the work it exists to bound, rather than
observing it from outside.

**I did not paper over this.** Killed the hung process
(confirmed via `ps` that nothing was left running except an unrelated `tail`), restored
`visionConcurrencyLimit = 4` and `@Suite(.serialized)` to their shipped values, and
added a "KNOWN LIMITATION" comment block directly above the test in
`AnalyzerTests.swift` recording this finding in full, rather than leaving the
watchdog's reliability assumed. Per the explicit instruction, I did not attempt to
redesign the watchdog into a fully independent (e.g. dedicated non-pool `Thread`)
mechanism myself — that's a judgment call left for the coordinator.

**What this does and doesn't mean for production.** The scenario that defeated the
watchdog required BOTH the production cap disabled AND `.serialized` removed —
`.serialized` is kept specifically because it prevents the local suite from ever
reaching that compound state. The realistic production shape (Task 15: one process,
one `Analyzer.analyze` call, nothing else competing for the shared pool) is exactly
Step A above, which did not deadlock even fully unbounded — so the watchdog was never
observed failing against the shape it actually needs to guard in production. It was
only observed failing against a synthetic double-fault (disabled cap *and* disabled
test isolation) that the shipped configuration cannot reach. I'm reporting this
distinction, not using it to wave the finding away — the coordinator should decide
whether the watchdog needs a genuinely independent thread regardless.

### Final state (before this round)

- `visionConcurrencyLimit = 4`, guard narrowed to just the Vision call.
- `HostileInputTests` keeps `@Suite(.serialized)`, byte-identical to its previous
  commit (temporarily removed and restored during the reproduction above; `git diff`
  confirms no net change).
- Stress test keeps the `DispatchSemaphore`-based watchdog (not `.timeLimit`), now
  with the known-limitation comment.
- Full suite, 3 consecutive runs, shipped configuration: all green, ~4.7s each.

---

## Third addendum: `Thread`-based watchdog, verified working

The `DispatchQueue.global()`-based watchdog above shares libdispatch's cooperative
worker pool with the exact work it's meant to bound, so it can starve alongside a real
deadlock instead of observing it from outside — that's what the second addendum found
and documented as a known limitation. Fixed here with a raw `Thread`, which the kernel
schedules directly rather than through libdispatch's pool, and re-verified against the
same reintroduced-deadlock scenario that defeated the previous version.

### The fix

`sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`:

```swift
let done = DispatchSemaphore(value: 0)
var records: [PhotoRecord] = []
let worker = Thread {
    records = Analyzer.analyze(paths: paths)
    done.signal()
}
worker.stackSize = 1 << 20
worker.start()
let outcome = done.wait(timeout: .now() + 120)
#expect(outcome == .success, "analyze deadlocked: did not complete within 120s")
guard outcome == .success else { return }
#expect(records.count == 300)
#expect(records.allSatisfy { if case .ok = $0 { return true }; return false })
```

`done.wait(timeout:)` on the test thread was already a kernel-level wait (that part
was correct before); the fix is putting the worker on a `Thread` too, so nothing in
the watchdog path depends on a free slot in the pool the deadlock is exhausting.
Replaced the "KNOWN LIMITATION" comment with one explaining why
`DispatchQueue.global()` was tried and rejected, so the reasoning survives and nobody
"simplifies" this back to a global-queue dispatch later.

### Normal-path sanity check

```
$ swift test --package-path sidecar --filter analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency
✔ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() passed after 2.784 seconds.
```

Matches the second addendum's post-narrowing numbers (~2.8s) — the `Thread` change
doesn't affect the happy path.

### Re-verification against the reintroduced deadlock

Repeated exactly the scenario that defeated the `DispatchQueue.global()` watchdog:
`visionConcurrencyLimit = 10_000` and `@Suite(.serialized)` removed from
`HostileInputTests`, full unfiltered suite:

```
$ swift test --package-path sidecar   # cap=10_000, .serialized removed, Thread-based watchdog
...
✘ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() recorded an issue at AnalyzerTests.swift:73:5: Expectation failed: (outcome → .timedOut) == .success
↳ analyze deadlocked: did not complete within 120s
✔ Test loadsLandscapeAtRequestedSize() passed after 120.023 seconds.
✔ Test paletteReturnsRequestedCountAndWeightsSumToOne() passed after 120.022 seconds.
✔ Test metricsPaletteIsStableAcrossRepeatedCalls() passed after 120.023 seconds.
✔ Test differentImagesHaveDifferentHash() passed after 120.022 seconds.
✔ Test exifReadsSouthernLatitudeAsNegative() passed after 120.022 seconds.
✔ Test visionTopLeftFlipsBottomLeftOriginToTopLeft() passed after 120.022 seconds.
✔ Test encodesErrorResponse() passed after 120.022 seconds.
✔ Test throwsOnMissingFile() passed after 120.022 seconds.
✔ Test metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() passed after 120.023 seconds.
✔ Test hostileZeroByteFileFailsCleanly() passed after 120.013 seconds.
✔ Test sharpEdgesScoreHigherThanFlatField() passed after 120.023 seconds.
✔ Test hostileDirectoryPathFailsCleanly() passed after 120.013 seconds.
✔ Test hostileTruncatedJpegFailsCleanlyOrDecodesPartially() passed after 120.013 seconds.
✔ Test metricsHashHammingDistanceModelsRealisticBurstVariation() passed after 120.023 seconds.
✘ Test analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() failed after 120.023 seconds with 1 issue.
```

**This is the confirmation the coordinator asked for.** The watchdog reported
`outcome → .timedOut`, failed the test at `120.023` seconds (essentially exactly the
configured deadline, not early and not indefinitely late), and — critically — **the
run continued**: 14 other tests that had also started before the sabotage (none of
them touching Vision — `loadsLandscapeAtRequestedSize`, palette/metrics/exif tests,
`hostile{ZeroByteFile,DirectoryPath,TruncatedJpeg}...`) went from queued to passed
within the same window, which only makes sense if my test's watchdog `Thread`
returning control freed a real scheduler resource the rest of the suite had been
waiting on. I did not adjust the timeout or the scenario to manufacture this result —
it's the same double-sabotage (cap disabled, `.serialized` removed) that produced a
genuine, unrecovered 3+ minute hang with the `DispatchQueue.global()` version.

I stopped monitoring and killed the process once this was confirmed, rather than
waiting for the *entire* suite to finish — the remaining Vision-touching tests
(`VisionAnalyzerTests`, the rest of `AnalyzerTests`, `HostileInputTests`) have no
watchdog of their own and were still genuinely deadlocked in Vision's internal queue
in this synthetic scenario; that's expected and outside what this specific test's
watchdog is responsible for.

### Restored and reconfirmed

Restored `visionConcurrencyLimit = 4` and `@Suite(.serialized)` on `HostileInputTests`
(`git diff` on both files after restoring is empty — no net change from the last
commit). Full suite, 3 consecutive runs at the shipped configuration:

```
✔ Test run with 67 tests in 1 suite passed after 4.568 seconds.
✔ Test run with 67 tests in 1 suite passed after 4.585 seconds.
✔ Test run with 67 tests in 1 suite passed after 4.604 seconds.
```

### Final state

- `visionConcurrencyLimit = 4`, guard narrowed to just the Vision call (unchanged from
  the second addendum).
- `HostileInputTests` keeps `@Suite(.serialized)`, unchanged.
- Stress test's watchdog is now `Thread`-based, verified to fire correctly (not just
  assumed) against the exact scenario that defeated the previous version.
- Only `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift` changed in this round.
