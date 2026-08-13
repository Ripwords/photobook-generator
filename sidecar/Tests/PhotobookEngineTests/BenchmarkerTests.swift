import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()   // PhotobookEngineTests
        .deletingLastPathComponent()   // Tests
        .deletingLastPathComponent()   // sidecar
        .appendingPathComponent("Fixtures/\(name)").path
}

// Every call below passes `visionOverride` to stub out real Vision --
// deliberately, not for speed. `Benchmarker`'s real path
// (`VisionGate.run { VisionAnalyzer.analyze(image) }`) is the exact code
// `Analyzer.analyzeOne` already runs and is already covered by
// `AnalyzerTests`'s real-Vision tests (including the 300-photo deadlock
// regression guard). Earlier versions of this file called real Vision
// directly, and even a single `.serialized`, VisionGate-routed call here
// reproducibly starved libdispatch's shared cooperative pool the moment it
// ran alongside the rest of this test target: `AnalyzerTests`'s 300-photo
// stress test alone can occupy the *entire* pool on this machine (its
// `concurrentPerform` fans out to core count, and every worker is either
// inside VisionGate's 4-permit section or blocked waiting for one), leaving
// no thread free to even schedule a new test's Task, regardless of how
// little work that Task does once scheduled. See `Benchmarker.swift`'s
// `visionOverride` doc comment for the full account. What these tests
// actually need to verify -- stage ordering, the fallback flag, extension
// parsing, positional ordering, JSON round-tripping -- has nothing to do
// with what Vision itself returns, so stubbing it costs nothing real.
private func stubVision(_: CGImage) -> VisionResult { VisionResult() }

@Test func benchmarkerReportsExtensionDimensionsAndPositiveStageTimings() {
    let records = Benchmarker.benchmark(
        paths: [fixture("landscape.jpg")], thumbnailDir: nil, visionOverride: stubVision
    )
    guard case .ok(let result) = records[0] else {
        Issue.record("expected an ok record")
        return
    }
    #expect(result.ext == "jpg")
    #expect(result.width > 0)
    #expect(result.height > 0)
    #expect(result.timings.hashMs >= 0)
    #expect(result.timings.exifMs >= 0)
    #expect(result.timings.decodeMs >= 0)
    #expect(result.timings.visionMs >= 0)
    #expect(result.timings.metricsMs >= 0)
    #expect(result.timings.thumbnailWriteMs >= 0)
    #expect(result.timings.totalMs >= result.timings.decodeMs,
             "total must be at least the decode stage alone")
}

@Test func benchmarkerFlagsFullDecodeFallbackForAPostageStampSource() {
    // Embedded thumbnail (~160x106) is well below the default floor (1536).
    let records = Benchmarker.benchmark(
        paths: [fixture("postage-stamp-source.jpg")], thumbnailDir: nil, visionOverride: stubVision
    )
    guard case .ok(let result) = records[0] else {
        Issue.record("expected an ok record")
        return
    }
    #expect(result.usedFullDecodeFallback == true)
}

@Test func benchmarkerDoesNotFallBackForALargeSourceWithNoEmbeddedThumbnail() {
    // large-no-thumbnail.jpg (2000x1333, no embedded thumbnail) is large
    // enough that decoding it directly already meets the default
    // analysisMaxPixel floor (1536) on the first pass -- must NOT report a
    // fallback. Not `landscape.jpg`: at 1200x800 it is smaller than the
    // 1536 floor, so it always reports a "fallback" even with nothing
    // smaller to have fallen back from -- that exercises a different,
    // uninteresting edge case (a source image smaller than the analysis
    // target) rather than the one this assertion is about.
    let records = Benchmarker.benchmark(
        paths: [fixture("large-no-thumbnail.jpg")], thumbnailDir: nil, visionOverride: stubVision
    )
    guard case .ok(let result) = records[0] else {
        Issue.record("expected an ok record")
        return
    }
    #expect(result.usedFullDecodeFallback == false)
}

@Test func benchmarkerProducesOneFailedRecordPerMissingPath() {
    let records = Benchmarker.benchmark(
        paths: ["/nonexistent/nope.jpg"], thumbnailDir: nil, visionOverride: stubVision
    )
    #expect(records.count == 1)
    guard case .failed(let path, _) = records[0] else {
        Issue.record("expected a failed record")
        return
    }
    #expect(path == "/nonexistent/nope.jpg")
}

@Test func benchmarkerReturnsExactlyOneRecordPerInputPathInOrder() {
    let paths = [fixture("landscape.jpg"), "/nonexistent/nope.jpg", fixture("portrait-rot90.jpg")]
    let records = Benchmarker.benchmark(paths: paths, thumbnailDir: nil, visionOverride: stubVision)
    #expect(records.count == 3)
    guard case .ok(let r0) = records[0] else { Issue.record("expected ok at 0"); return }
    #expect(r0.path == fixture("landscape.jpg"))
    guard case .failed(let p1, _) = records[1] else { Issue.record("expected failed at 1"); return }
    #expect(p1 == "/nonexistent/nope.jpg")
    guard case .ok(let r2) = records[2] else { Issue.record("expected ok at 2"); return }
    #expect(r2.path == fixture("portrait-rot90.jpg"))
}

@Test func benchmarkFileResultRoundTripsThroughJson() throws {
    let records = Benchmarker.benchmark(
        paths: [fixture("landscape.jpg")], thumbnailDir: nil, visionOverride: stubVision
    )
    let data = try encoder.encode(records)
    let decoded = try decoder.decode([BenchmarkRecord].self, from: data)
    guard case .ok(let result) = decoded[0] else {
        Issue.record("expected an ok record after round trip")
        return
    }
    #expect(result.ext == "jpg")
    #expect(result.width > 0)
}
