import Foundation
import CryptoKit

struct PhotoFeatures: Codable {
    let path: String
    let hash: String
    let width: Int
    let height: Int
    let exif: ExifData
    let isUtility: Bool
    let aestheticScore: Double
    let sharpness: Double
    let faces: [FaceObservation]
    let faceAreaFraction: Double
    let smileFraction: Double?
    let saliencyBox: [Double]?
    let horizonTiltDeg: Double?
    let sceneTags: [String]
    let hasText: Bool
    let palette: [PaletteColor]
    let warmth: Double
    let contrast: Double
    let phash: UInt64
}

enum PhotoRecord: Codable {
    case ok(PhotoFeatures)
    case failed(path: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, features, path, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let f):
            try c.encode("ok", forKey: .status)
            try c.encode(f, forKey: .features)
        case .failed(let path, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(path, forKey: .path)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(try c.decode(PhotoFeatures.self, forKey: .features))
        } else {
            self = .failed(path: try c.decode(String.self, forKey: .path),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

enum Analyzer {
    // Metrics internally resample to 256/128/8 (see Metrics.swift); this must
    // stay comfortably above all of those or they'd be upscaling instead of
    // downscaling, which distorts the Laplacian-based sharpness metric.
    static let analysisMaxPixel = 1536

    // Caps the number of VisionAnalyzer.analyze CALLS in flight at once --
    // and *only* that call, not the metrics that follow it. The guard below
    // must stay scoped to the immediately-invoked closure that wraps
    // `VisionAnalyzer.analyze(image)` and nothing else: an earlier version of
    // this fix scoped the wait()/signal() pair to the entire enclosing
    // autoreleasepool closure, which meant Metrics.palette/sharpness/contrast/
    // perceptualHash and the PhotoFeatures construction all ran while still
    // holding a permit, serializing pure-CPU work that has no business being
    // gated -- a real throughput regression on multi-core machines, caught in
    // review, not just a style nit.
    //
    // DispatchQueue.concurrentPerform below sizes its fan-out to the CPU core
    // count, and each iteration's VisionAnalyzer.analyze makes synchronously-
    // blocking VNImageRequestHandler.perform() calls, which internally wait
    // on Vision's own VNControlledCapacityTasksQueue via the shared libdispatch
    // global concurrent queue. Letting core-count-many of those block at once
    // can starve the very queue Vision needs to service them: this is a real,
    // reproduced deadlock (zero CPU progress, confirmed via `sample`), not a
    // hypothetical one -- see sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift
    // and task-10-report.md. Decode (ImageLoader) and metrics (Metrics.swift)
    // are CPU-bound and stay at full concurrentPerform width; only the Vision
    // call itself is gated. DO NOT remove this as a "pessimisation" on a
    // many-core machine -- removing it reintroduces the deadlock. 4 was
    // chosen after re-measuring throughput against 2 once the guard was
    // narrowed to just the Vision call (see task-10-report.md); if you need
    // to change it, re-run that comparison and the stress test in
    // AnalyzerTests.swift first.
    private static let visionConcurrencyLimit = 4
    private static let visionSemaphore = DispatchSemaphore(value: visionConcurrencyLimit)

    private static func contentHash(path: String) throws -> String {
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    static func analyzeOne(path: String) throws -> PhotoFeatures {
        let exif = try ExifReader.read(path: path)
        let hash = try contentHash(path: path)

        // The pool must enclose the decode itself, not just what runs after
        // it. loadThumbnail's kCGImageSourceShouldCacheImmediately option
        // forces the full JPEG/HEIC decompression, colour management, and
        // downsample to happen synchronously right there — it is the
        // largest allocation site in this whole per-photo pipeline, larger
        // than the Vision pass or metrics that follow. concurrentPerform
        // worker threads have no autorelease pool of their own, so if the
        // decode happened outside this block those allocations would
        // accumulate unbounded across a large batch — precisely the failure
        // mode autoreleasepool exists here to prevent. autoreleasepool
        // rethrows, so `try` moves inside with it.
        return try autoreleasepool {
            let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)

            // The permit is held for exactly the Vision call and nothing
            // else -- Metrics below runs at full concurrentPerform width,
            // unthrottled. Signalled on every exit from this closure,
            // including a future throwing change to VisionAnalyzer.analyze --
            // a leaked permit here would starve the semaphore down to zero
            // and turn this fix into a guaranteed hang instead of a bounded
            // one.
            let vision: VisionResult = {
                visionSemaphore.wait()
                defer { visionSemaphore.signal() }
                return VisionAnalyzer.analyze(image)
            }()

            let palette = Metrics.palette(image, count: 6)
            let faceArea = vision.faces.reduce(0.0) { $0 + $1.box[2] * $1.box[3] }

            return PhotoFeatures(
                path: path,
                hash: hash,
                width: exif.pixelWidth,
                height: exif.pixelHeight,
                exif: exif,
                isUtility: vision.isUtility,
                aestheticScore: vision.aestheticScore,
                sharpness: Metrics.sharpness(image),
                faces: vision.faces,
                faceAreaFraction: min(faceArea, 1.0),
                smileFraction: SmileProxy.fraction(faces: vision.faces, threshold: 0.5),
                saliencyBox: vision.saliencyBox,
                horizonTiltDeg: vision.horizonTiltDeg,
                sceneTags: vision.sceneTags,
                hasText: vision.hasText,
                palette: palette,
                warmth: Metrics.warmth(palette),
                contrast: Metrics.contrast(image),
                phash: Metrics.perceptualHash(image)
            )
        }
    }

    /// Analyses every path in `paths` concurrently. Completion order under
    /// `concurrentPerform` is arbitrary, but the returned array is always in
    /// input order — callers correlate records to paths positionally, and a
    /// mismatch would silently attribute one photo's analysis to another.
    /// Every path yields exactly one record, `.ok` or `.failed`; a failure on
    /// one photo never aborts the batch and never throws out of this function.
    static func analyze(paths: [String]) -> [PhotoRecord] {
        var results = [PhotoRecord?](repeating: nil, count: paths.count)
        let lock = NSLock()

        // The compiler warns about mutating a captured `var` from concurrent
        // closures (#SendableClosureCaptures) here; it is a false positive
        // in practice, not a real data race. Every write to `results` is
        // serialized by `lock`, and `concurrentPerform` blocks the calling
        // thread until *all* iterations have returned before this function
        // reads `results` below — that combination gives a real
        // happens-before edge from every write to the final read. If this
        // package is ever opted into full Swift 6 language mode, this
        // pattern becomes a hard error and would need `Mutex` (or wrapping
        // `results` in an `@unchecked Sendable` box) to satisfy the checker.
        DispatchQueue.concurrentPerform(iterations: paths.count) { index in
            let path = paths[index]
            let record: PhotoRecord
            do {
                record = .ok(try analyzeOne(path: path))
            } catch {
                record = .failed(path: path, message: String(describing: error))
            }
            lock.lock()
            results[index] = record
            lock.unlock()
        }

        return results.compactMap { $0 }
    }
}
