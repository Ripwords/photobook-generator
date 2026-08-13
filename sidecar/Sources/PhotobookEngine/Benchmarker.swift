import Foundation
import CoreGraphics

/// Per-stage wall-clock timings for one file, in milliseconds. This is the
/// permanent diagnostic behind the `benchmark` request: it exists so a
/// future change to decode, Vision, or metrics can be measured per-format
/// (RAW/HEIC/JPEG have very different cost profiles) instead of guessing.
struct StageTimingsMs: Codable {
    let hashMs: Double
    let exifMs: Double
    let decodeMs: Double
    let visionMs: Double
    let metricsMs: Double
    let thumbnailWriteMs: Double
    let totalMs: Double
}

struct BenchmarkFileResult: Codable {
    let path: String
    /// Lowercased extension without the leading dot (e.g. "arw", "heic",
    /// "jpg") -- the grouping key `scripts/benchmark.sh` uses to separate
    /// RAW from HEIC from JPEG in its report.
    let ext: String
    let width: Int
    let height: Int
    /// Whether `ImageLoader`'s size floor rejected the embedded preview and
    /// forced a full decode. Aggregated per-extension by the benchmark
    /// script to answer "does ARW ever fall back?" (it shouldn't) and
    /// "does JPEG always fall back?" (it should, and that's correct).
    let usedFullDecodeFallback: Bool
    let timings: StageTimingsMs
}

enum BenchmarkRecord: Codable {
    case ok(BenchmarkFileResult)
    case failed(path: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, result, path, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let r):
            try c.encode("ok", forKey: .status)
            try c.encode(r, forKey: .result)
        case .failed(let path, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(path, forKey: .path)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(try c.decode(BenchmarkFileResult.self, forKey: .result))
        } else {
            self = .failed(path: try c.decode(String.self, forKey: .path),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

/// Runs the exact per-photo pipeline `Analyzer.analyzeOne` runs (hash, EXIF,
/// decode, Vision, classical metrics, optional thumbnail write), but timed
/// stage-by-stage instead of end-to-end. Deliberately sequential, not
/// `concurrentPerform`-parallel like `Analyzer.analyze`: concurrent runs
/// contend for CPU and the Vision semaphore, which is representative of real
/// throughput but makes per-stage numbers noisy and hard to compare across
/// formats. A benchmark run's wall time is therefore not directly comparable
/// to a real `analyze` batch's wall time -- see the aggregate ratios and
/// per-format comparisons instead, which are what this is for.
enum Benchmarker {
    private static func elapsedMs(since start: DispatchTime) -> Double {
        Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000
    }

    /// Test-only seam. Production (`Handler.swift`) never passes this, so it
    /// always defaults to nil and every real request runs the true
    /// `VisionGate.run { VisionAnalyzer.analyze(image) }` path -- identical
    /// to `Analyzer.analyzeOne`'s. It exists because `BenchmarkerTests`
    /// calling real Vision, even a single gated call from a single
    /// `.serialized` test, reproducibly starved libdispatch's shared
    /// cooperative pool when run alongside the rest of this test target's
    /// existing Vision-heavy tests (`AnalyzerTests`'s 300-photo stress test
    /// alone can occupy the entire pool: its `concurrentPerform` fans out to
    /// core count, and each worker is either inside `VisionGate`'s 4-permit
    /// section or blocked waiting for one -- there is no pool slack left for
    /// a new caller's test-function Task to even start, regardless of how
    /// little work it does once scheduled). Stubbing Vision here removes
    /// `BenchmarkerTests` from that contention entirely; the real
    /// `VisionGate.run { VisionAnalyzer.analyze }` call is still exercised,
    /// byte-for-byte, by `Analyzer`'s own tests.
    static func benchmarkOne(
        path: String,
        thumbnailDir: String?,
        visionOverride: ((CGImage) -> VisionResult)? = nil
    ) -> BenchmarkRecord {
        let overallStart = DispatchTime.now()
        do {
            let hashStart = DispatchTime.now()
            let hash = try Analyzer.contentHash(path: path)
            let hashMs = elapsedMs(since: hashStart)

            let exifStart = DispatchTime.now()
            _ = try ExifReader.read(path: path)
            let exifMs = elapsedMs(since: exifStart)

            var decodeMs = 0.0, visionMs = 0.0, metricsMs = 0.0, thumbnailWriteMs = 0.0
            var usedFallback = false
            var width = 0, height = 0

            try autoreleasepool {
                let decodeStart = DispatchTime.now()
                let loaded = try ImageLoader.loadThumbnailDetailed(path: path, maxPixel: Analyzer.analysisMaxPixel)
                decodeMs = elapsedMs(since: decodeStart)
                usedFallback = loaded.usedFullDecodeFallback
                let image = loaded.image
                width = image.width
                height = image.height

                // Must go through VisionGate, not call VisionAnalyzer.analyze
                // directly -- see VisionGate's doc comment. An earlier
                // version of this function called Vision unguarded and
                // reproduced the documented VNControlledCapacityTasksQueue
                // deadlock (confirmed via `sample`: every thread parked in
                // `dispatchGroupWait`, 0% CPU). See `visionOverride`'s doc
                // comment above for why tests stub this instead of relying
                // on VisionGate alone.
                let visionStart = DispatchTime.now()
                _ = (visionOverride ?? { img in VisionGate.run { VisionAnalyzer.analyze(img) } })(image)
                visionMs = elapsedMs(since: visionStart)

                let metricsStart = DispatchTime.now()
                _ = Metrics.sharpness(image)
                _ = Metrics.palette(image, count: 6)
                _ = Metrics.contrast(image)
                _ = Metrics.perceptualHash(image)
                metricsMs = elapsedMs(since: metricsStart)

                if let dir = thumbnailDir {
                    let thumbnailStart = DispatchTime.now()
                    _ = try? ThumbnailWriter.write(image: image, hash: hash, directory: dir, maxPixel: Analyzer.thumbnailMaxPixel)
                    thumbnailWriteMs = elapsedMs(since: thumbnailStart)
                }
            }

            let totalMs = elapsedMs(since: overallStart)
            let ext = (path as NSString).pathExtension.lowercased()

            return .ok(BenchmarkFileResult(
                path: path,
                ext: ext,
                width: width,
                height: height,
                usedFullDecodeFallback: usedFallback,
                timings: StageTimingsMs(
                    hashMs: hashMs, exifMs: exifMs, decodeMs: decodeMs,
                    visionMs: visionMs, metricsMs: metricsMs,
                    thumbnailWriteMs: thumbnailWriteMs, totalMs: totalMs
                )
            ))
        } catch {
            return .failed(path: path, message: String(describing: error))
        }
    }

    /// One record per input path, in input order -- same positional
    /// contract as `Analyzer.analyze`, even though this runs sequentially
    /// rather than concurrently.
    static func benchmark(
        paths: [String],
        thumbnailDir: String?,
        visionOverride: ((CGImage) -> VisionResult)? = nil
    ) -> [BenchmarkRecord] {
        paths.map { benchmarkOne(path: $0, thumbnailDir: thumbnailDir, visionOverride: visionOverride) }
    }
}
