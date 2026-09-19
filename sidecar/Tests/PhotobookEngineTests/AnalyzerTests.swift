import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

// Regression test for a real deadlock found while adding Task 10's hostile
// fixtures: VisionAnalyzer.analyze makes synchronously-blocking
// VNImageRequestHandler.perform() calls, and DispatchQueue.concurrentPerform's
// full core-count fan-out of those piling onto Vision's internal capacity
// queue at once starved the queue Vision needed to service them --
// confirmed via `sample` as a genuine zero-CPU-progress deadlock, not slow
// computation (see task-10-report.md). Analyzer's private visionSemaphore
// bounds that fan-out; this is the regression guard for it. 300 repeats of
// the same fixture is enough to exercise the concurrency -- the content
// doesn't matter, only the count in flight at once.
//
// NOT using `@Test(.timeLimit(...))` here: swift-testing enforces that trait
// through cooperative task cancellation, which only takes effect at a
// suspension point. This test body is fully synchronous all the way down
// (Analyzer.analyze -> concurrentPerform -> DispatchSemaphore.wait()) with
// no suspension points, and a thread genuinely stuck in wait() never
// returns control to let the task group observe cancellation -- so the
// exact regression this test exists to catch would hang instead of timing
// out, silently defeating the point of having a watchdog at all (verified
// empirically, see task-10-report.md).
//
// NOT using `DispatchQueue.global().async` either, even though it looks
// like the obvious "run it in the background" answer -- that was tried
// first and *failed verification*. DispatchQueue.global() schedules onto
// libdispatch's shared cooperative worker pool, which is the exact same
// pool concurrentPerform and VNImageRequestHandler's internal queue draw
// from. Under a real deadlock that pool can be fully exhausted, and a
// newly-submitted `.async` block then never gets a thread to run on at
// all -- confirmed via `sample` on a deliberately-induced hang: dozens of
// threads were legitimately stuck inside Vision's
// VNControlledCapacityTasksQueue, but this test's watchdog closure never
// appeared running anywhere, because it was queued behind the same
// exhausted pool it was supposed to be watching from outside. A watchdog
// built on DispatchQueue.global() is not out-of-band; it's in the same
// band as the thing it watches, and can starve right alongside it.
//
// Using a raw `Thread` instead: Thread is scheduled directly by the
// kernel, not through libdispatch's cooperative pool, so it stays
// runnable no matter how thoroughly that pool is starved. The test thread
// blocks on `done.wait(timeout:)`, which is also a kernel-level wait, not
// a pool-scheduled one -- so both the worker and the observer are
// independent of whatever `Analyzer.analyze` does to the shared pool
// internally (concurrentPerform legitimately using that pool is fine and
// expected; only the watchdog's ability to observe and report must not
// depend on it). Verified this against the same reintroduced-deadlock
// scenario that defeated the DispatchQueue.global() version (cap disabled,
// HostileInputTests' @Suite(.serialized) removed, full suite run): this
// version reported the expected failure in ~120s and the run continued,
// instead of hanging (see task-10-report.md for the transcript). If it
// ever deadlocks for real, the worker Thread leaks for the rest of the
// process's life, but the test reports a failure rather than hanging CI.
@Test func analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency() {
    let paths = Array(repeating: fixture("landscape.jpg"), count: 300)
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
    #expect(records.allSatisfy {
        if case .ok = $0 { return true }
        return false
    })
}

@Test func analyzerAnalysesAGoodPhoto() {
    let records = Analyzer.analyze(paths: [fixture("landscape.jpg")])
    #expect(records.count == 1)
    guard case .ok(let f) = records[0] else {
        Issue.record("expected ok record"); return
    }
    #expect(f.width == 1200)
    #expect(f.height == 800)
    // SHA-256 hex digest: exactly 64 lowercase hex characters, over raw file
    // bytes (not decoded pixels) — Rust computes the same hash independently
    // to check its cache before calling the sidecar, so the two must agree
    // on what's hashed.
    #expect(f.hash.count == 64)
    #expect(f.hash.allSatisfy { $0.isHexDigit && !$0.isUppercase })
    // The fixture is a flat colour field, so field *ranges* are asserted
    // rather than exact photographic content. Note: a flat field legitimately
    // hashes to 0 (every 8x8 cell equals the mean, so no bit is set) — that
    // is correct behaviour for this input, not a bug, so phash is not
    // asserted non-zero here.
    #expect(f.sharpness >= 0)
    #expect((0...1).contains(f.warmth))
    #expect((0...1).contains(f.contrast))
    #expect((0...1).contains(f.clippedLow))
    #expect((0...1).contains(f.clippedHigh))
    #expect(f.featurePrint.flatMap(FeaturePrint.decode)?.count == 768)
    #expect(!f.palette.isEmpty)
    #expect(f.faces.isEmpty)
    #expect(f.faceAreaFraction == 0)
    #expect(f.smileFraction == nil)
}

// MARK: - Cross-language hash agreement (I4)
//
// Rust's `hash_file` (src-tauri/src/commands.rs) is the cache lookup key;
// Swift's `contentHash` here is the write-back key AND the thumbnail
// filename. A divergence in hex case, chunk size, or algorithm makes every
// run a 100% cache miss and orphans every thumbnail written previously --
// silently, with every existing test in both languages green, because
// nothing anywhere compared the two literal outputs against each other.
// Pinned against the same fixture and the same literal string as
// commands.rs's `hash_matches_the_literal_pinned_against_swifts_content_hash`
// -- the two tests can only both pass if the implementations genuinely
// agree.
//
// Calls `Analyzer.contentHash` directly rather than going through
// `Analyzer.analyze` -- that would invoke Vision for no reason this test
// needs, and would add another independent Vision-calling `@Test` function
// racing concurrently against `analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`
// below (swift-testing schedules `@Test` functions concurrently by
// default). See that test's doc comment and `analyzerThumbnailWriting`'s for
// the same reasoning already applied elsewhere in this file.
@Test func analyzerHashMatchesThePinnedRustLiteral() throws {
    let hash = try Analyzer.contentHash(path: fixture("landscape.jpg"))
    #expect(hash == "08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159")
}

@Test func analyzerReportsFailureForMissingFileWithoutCrashing() {
    let records = Analyzer.analyze(paths: ["/nonexistent/nope.jpg"])
    #expect(records.count == 1)
    guard case .failed(let path, _) = records[0] else {
        Issue.record("expected failed record"); return
    }
    #expect(path == "/nonexistent/nope.jpg")
}

@Test func analyzerOneBadPhotoDoesNotAbortTheBatch() {
    let records = Analyzer.analyze(paths: [
        "/nonexistent/nope.jpg",
        fixture("landscape.jpg"),
    ])
    #expect(records.count == 2)
    // Order must be preserved so callers can correlate with their input.
    guard case .failed = records[0] else { Issue.record("expected failure first"); return }
    guard case .ok = records[1] else { Issue.record("expected success second"); return }
}

@Test func analyzerPreservesInputOrderUnderConcurrency() {
    // The 8 real photos each pay for a full Vision pass (aesthetics, face
    // detection, saliency, horizon, classification, text) while the missing
    // file at the end fails immediately on file open. If results were
    // appended in completion order rather than written by index, the fast
    // failure would very likely land near the front of the array, not at
    // index 8 — that's what makes this test an effective mutation check
    // rather than a coincidence of identical fixtures.
    let paths = (0..<8).map { _ in fixture("landscape.jpg") } + ["/nonexistent/a.jpg"]
    let records = Analyzer.analyze(paths: paths)
    #expect(records.count == 9)
    guard case .failed = records[8] else { Issue.record("last must be the failure"); return }
    for i in 0..<8 {
        guard case .ok = records[i] else { Issue.record("expected ok at index \(i)"); return }
    }
}

// MARK: - Thumbnail writing
//
// Analyzer.analyzeOne reuses the CGImage it already decoded for analysis to
// write a small contact-sheet JPEG. Writing must never fail the photo record
// itself -- a missing thumbnail is a degraded grid tile, not a lost photo.

private func thumbnailScratchDirectory() -> String {
    NSTemporaryDirectory() + "analyzer-thumbnail-tests-\(UUID().uuidString)"
}

// The three cases below are deliberately combined into ONE @Test function
// with three sequential Analyzer.analyze calls, rather than three separate
// @Test functions. Every case here drives a real, un-mocked Vision pass
// (Analyzer.analyzeOne always calls VisionAnalyzer.analyze regardless of
// thumbnailDir), and swift-testing schedules @Test functions concurrently
// by default -- three separate functions would add three more independent
// Vision-calling tasks racing against everything else in this module
// (including the 300-photo stress test above, which is already using its
// full slice of Analyzer's visionSemaphore, and VisionAnalyzerTests.swift's
// direct, un-gated calls to VisionAnalyzer.analyze which bypass that
// semaphore entirely). That combination reproducibly starved Vision's
// internal VNControlledCapacityTasksQueue during development of this file
// (confirmed via `sample`, same failure mode documented on the 300-photo
// test above) even though each individual call here is semaphore-gated.
// Sequential calls within one task avoid adding concurrent pressure.
@Test func analyzerThumbnailWriting() throws {
    // Case 1: thumbnailDir provided -> thumbnailPath is written and keyed by
    // content hash, not source path.
    let dir = thumbnailScratchDirectory()
    let written = Analyzer.analyze(paths: [fixture("landscape.jpg")], thumbnailDir: dir)
    guard case .ok(let f1) = written[0] else {
        Issue.record("expected ok record"); return
    }
    let path = f1.thumbnailPath
    #expect(path != nil)
    if let path {
        #expect(FileManager.default.fileExists(atPath: path))
        #expect(path == dir + "/" + f1.hash + ".jpg")
    }

    // Case 2: no thumbnailDir -> thumbnailPath stays nil. Regression guard
    // against a mutant that writes a thumbnail unconditionally regardless of
    // whether a directory was requested.
    let withoutDir = Analyzer.analyze(paths: [fixture("landscape.jpg")])
    guard case .ok(let f2) = withoutDir[0] else {
        Issue.record("expected ok record"); return
    }
    #expect(f2.thumbnailPath == nil)

    // Case 3: thumbnail write fails -> record stays .ok with thumbnailPath
    // nil, not .failed. A regular file sitting where the thumbnail directory
    // needs to be created forces ThumbnailWriter.write to throw -- a
    // portable way to exercise the failure path without relying on real
    // filesystem permissions.
    let blockerPath = thumbnailScratchDirectory()
    try Data().write(to: URL(fileURLWithPath: blockerPath))
    defer { try? FileManager.default.removeItem(atPath: blockerPath) }

    let degraded = Analyzer.analyze(paths: [fixture("landscape.jpg")], thumbnailDir: blockerPath)
    guard case .ok(let f3) = degraded[0] else {
        Issue.record("a thumbnail write failure must not turn the record into .failed"); return
    }
    #expect(f3.thumbnailPath == nil)
    // The rest of the record must be intact -- a thumbnail failure is
    // isolated, not contagious to the rest of the analysis.
    #expect(f3.width == 1200)
    #expect(f3.height == 800)
}

// MARK: - JSON contract with Rust
//
// Rust reads `record["status"]` and `record["features"]` (or `["path"]` /
// `["message"]` for failures) positionally off this exact envelope shape,
// and `ResponseResult` uses `#[serde(tag = "type", content = "data")]` on
// the Rust side. Nothing above exercises PhotoRecord's hand-rolled Codable
// or ResponseResult.analyzed directly — only a manual piped invocation did
// — so a silent typo in CodingKeys or init(from:) would break the pipeline
// at the language boundary without any test catching it. These tests close
// that gap.

private func sampleFeatures() -> PhotoFeatures {
    PhotoFeatures(
        path: "/photos/sample.jpg",
        hash: "deadbeef",
        width: 10,
        height: 20,
        exif: ExifData(
            captureDate: nil, latitude: nil, longitude: nil,
            pixelWidth: 10, pixelHeight: 20,
            make: nil, model: nil, flashFired: nil
        ),
        isUtility: false,
        aestheticScore: 0.5,
        sharpness: 1.5,
        faces: [],
        faceAreaFraction: 0,
        smileFraction: nil,
        saliencyBox: nil,
        horizonTiltDeg: nil,
        sceneTags: ["outdoor"],
        hasText: false,
        palette: [],
        warmth: 0.5,
        contrast: 0.5,
        clippedLow: 0,
        clippedHigh: 0,
        phash: 42,
        featurePrint: nil,
        thumbnailPath: nil
    )
}

private func decodedJSONObject(_ data: Data) throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
}

// MARK: - I5: complete top-level key set of PhotoFeatures
//
// `photoRecordEncodesOkEnvelope` below pins only three fields (`path`,
// `width`, `phash`). Rust reads `aestheticScore`, `sharpness`,
// `exif.captureDate`, `faces`, and `isUtility` off this exact JSON; the
// webview reads `hash`, `height`, `sceneTags`, `smileFraction`, and
// `thumbnailPath`. None of those are pinned anywhere -- rename
// `aestheticScore` on the Swift side and every existing test (including
// `photoRecordRoundTripsOkThroughJSON`, which decodes through PhotoFeatures'
// OWN `Codable`) still passes, while Rust's `unwrap_or(0.0)` in
// `finalize_photos` silently gives every photo the same aesthetic
// percentile. This test asserts the complete top-level key set via
// `JSONSerialization`, deliberately NOT via PhotoFeatures' own
// `Decodable` -- a shared rename applied identically to both the encode and
// decode side of the same struct would pass unchanged and prove nothing.
@Test func photoFeaturesEncodesExactlyThePinnedTopLevelKeySet() throws {
    // Every optional field is populated (non-nil) here, specifically so
    // `encodeIfPresent` does not drop it from the output -- this test's
    // whole point is pinning the COMPLETE key set including the optional
    // fields, so the fixture must not accidentally exercise a shape with
    // some of them silently absent (that is `sampleFeatures()`'s job,
    // reused by the envelope tests below).
    let full = PhotoFeatures(
        path: "/photos/sample.jpg",
        hash: "deadbeef",
        width: 10,
        height: 20,
        exif: ExifData(
            captureDate: Date(timeIntervalSince1970: 0),
            latitude: 1.0,
            longitude: 2.0,
            pixelWidth: 10,
            pixelHeight: 20,
            make: "Apple",
            model: "iPhone",
            flashFired: false
        ),
        isUtility: false,
        aestheticScore: 0.5,
        sharpness: 1.5,
        faces: [
            FaceObservation(
                box: [0, 0, 1, 1], yaw: 0, pitch: 0, roll: 0,
                captureQuality: 0.5, outerLips: [[0, 0]]
            ),
        ],
        faceAreaFraction: 0.1,
        smileFraction: 0.5,
        saliencyBox: [0, 0, 1, 1],
        horizonTiltDeg: 1.0,
        sceneTags: ["outdoor"],
        hasText: false,
        palette: [],
        warmth: 0.5,
        contrast: 0.5,
        clippedLow: 0.1,
        clippedHigh: 0.2,
        phash: 42,
        featurePrint: "AACAPwAAIMA=",
        thumbnailPath: "/tmp/abc.jpg"
    )

    let data = try JSONEncoder().encode(full)
    let obj = try decodedJSONObject(data)
    let keys = Set(obj.keys)

    let pinned: Set<String> = [
        "path", "hash", "width", "height", "exif", "isUtility",
        "aestheticScore", "sharpness", "faces", "faceAreaFraction",
        "smileFraction", "saliencyBox", "horizonTiltDeg", "sceneTags",
        "hasText", "palette", "warmth", "contrast", "clippedLow", "clippedHigh",
        "phash", "featurePrint", "thumbnailPath",
    ]

    let missing = pinned.subtracting(keys)
    let unexpected = keys.subtracting(pinned)
    #expect(missing.isEmpty, "PhotoFeatures is missing pinned keys: \(missing)")
    #expect(
        unexpected.isEmpty,
        "PhotoFeatures encoded unpinned keys -- update this test's `pinned` set deliberately: \(unexpected)"
    )
}

@Test func photoRecordEncodesOkEnvelope() throws {
    let data = try JSONEncoder().encode(PhotoRecord.ok(sampleFeatures()))
    let obj = try decodedJSONObject(data)
    #expect(obj["status"] as? String == "ok")
    let features = try #require(obj["features"] as? [String: Any])
    #expect(features["path"] as? String == "/photos/sample.jpg")
    #expect(features["width"] as? Int == 10)
    #expect(features["phash"] as? Int == 42)
    // A .ok record must not also carry the failure-only keys.
    #expect(obj["path"] == nil)
    #expect(obj["message"] == nil)
}

@Test func photoRecordEncodesFailedEnvelope() throws {
    let data = try JSONEncoder().encode(PhotoRecord.failed(path: "/missing.jpg", message: "boom"))
    let obj = try decodedJSONObject(data)
    #expect(obj["status"] as? String == "failed")
    #expect(obj["path"] as? String == "/missing.jpg")
    #expect(obj["message"] as? String == "boom")
    // A .failed record must not also carry the ok-only key.
    #expect(obj["features"] == nil)
}

@Test func photoRecordRoundTripsOkThroughJSON() throws {
    let original = PhotoRecord.ok(sampleFeatures())
    let data = try JSONEncoder().encode(original)
    let decoded = try JSONDecoder().decode(PhotoRecord.self, from: data)
    guard case .ok(let f) = decoded else {
        Issue.record("expected ok record after round-trip"); return
    }
    #expect(f.path == "/photos/sample.jpg")
    #expect(f.hash == "deadbeef")
    #expect(f.width == 10)
    #expect(f.height == 20)
    #expect(f.phash == 42)
}

@Test func photoRecordRoundTripsFailedThroughJSON() throws {
    let original = PhotoRecord.failed(path: "/missing.jpg", message: "boom")
    let data = try JSONEncoder().encode(original)
    let decoded = try JSONDecoder().decode(PhotoRecord.self, from: data)
    guard case .failed(let path, let message) = decoded else {
        Issue.record("expected failed record after round-trip"); return
    }
    #expect(path == "/missing.jpg")
    #expect(message == "boom")
}

@Test func responseResultAnalyzedEncodesTypeAndDataEnvelope() throws {
    let response = Response(
        id: "r1",
        result: .analyzed([.ok(sampleFeatures()), .failed(path: "/x.jpg", message: "boom")])
    )
    let data = try JSONEncoder().encode(response)
    let obj = try decodedJSONObject(data)
    #expect(obj["id"] as? String == "r1")
    let result = try #require(obj["result"] as? [String: Any])
    // Rust's #[serde(tag = "type", content = "data")] depends on exactly
    // these two keys, at the top level of `result`.
    #expect(result["type"] as? String == "analyzed")
    let entries = try #require(result["data"] as? [[String: Any]])
    #expect(entries.count == 2)
    #expect(entries[0]["status"] as? String == "ok")
    #expect(entries[1]["status"] as? String == "failed")
}

@Test func responseResultAnalyzedRoundTripsThroughJSON() throws {
    let response = Response(id: "r2", result: .analyzed([.ok(sampleFeatures())]))
    let data = try JSONEncoder().encode(response)
    let decoded = try JSONDecoder().decode(Response.self, from: data)
    #expect(decoded.id == "r2")
    guard case .analyzed(let records) = decoded.result else {
        Issue.record("expected analyzed result after round-trip"); return
    }
    #expect(records.count == 1)
    guard case .ok(let f) = records[0] else {
        Issue.record("expected ok record after round-trip"); return
    }
    #expect(f.path == "/photos/sample.jpg")
}
