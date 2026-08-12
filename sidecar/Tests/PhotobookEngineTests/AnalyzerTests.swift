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
// doesn't matter, only the count in flight at once. The time limit turns a
// reintroduced deadlock into a fast, clear test failure instead of a hung
// CI job.
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
    #expect(!f.palette.isEmpty)
    #expect(f.faces.isEmpty)
    #expect(f.faceAreaFraction == 0)
    #expect(f.smileFraction == nil)
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
        phash: 42
    )
}

private func decodedJSONObject(_ data: Data) throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
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
