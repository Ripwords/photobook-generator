import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
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
