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
    /// Fraction of luma below 5% of full scale.
    let clippedLow: Double
    /// Fraction of luma above 95% of full scale.
    let clippedHigh: Double
    let phash: UInt64
    /// Vision's image feature print, encoded by `FeaturePrint.encode`. Nil
    /// when Vision produced none.
    let featurePrint: String?
    /// Path to a small JPEG contact-sheet thumbnail, or nil if none was
    /// requested (`thumbnailDir` was nil) or writing it failed. A missing
    /// thumbnail degrades to a blank grid tile in the UI -- it must never
    /// fail the whole photo record.
    let thumbnailPath: String?
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

    /// Long edge, in pixels, of the contact-sheet thumbnail written alongside
    /// each analysed photo. Sized for a grid tile on a Retina display at
    /// typical tile sizes -- large enough to look sharp, small enough that
    /// writing hundreds of them during a batch import is not itself a
    /// bottleneck.
    static let thumbnailMaxPixel = 400

    // Not private so the cross-language hash-agreement test
    // (`analyzerHashMatchesThePinnedRustLiteral` in AnalyzerTests.swift) can
    // call it directly instead of going through `analyzeOne`/`analyze`,
    // which would invoke Vision unnecessarily -- and, more importantly,
    // would add another independent Vision-calling `@Test` function racing
    // concurrently against the 300-photo deadlock stress test below (see
    // that test's doc comment on why `analyzerThumbnailWriting` deliberately
    // avoids exactly this).
    static func contentHash(path: String) throws -> String {
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    static func analyzeOne(path: String, thumbnailDir: String? = nil) throws -> PhotoFeatures {
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
            // unthrottled. See VisionGate's doc comment: every call site
            // that invokes VisionAnalyzer.analyze must go through it, not
            // call Vision directly.
            let vision: VisionResult = VisionGate.run { VisionAnalyzer.analyze(image) }

            let palette = Metrics.palette(image, count: 6)
            let faceArea = vision.faces.reduce(0.0) { $0 + $1.box[2] * $1.box[3] }
            let clipping = Metrics.clipping(image)

            // Reuses the CGImage already decoded above -- never decodes the
            // source file a second time just to make a thumbnail. A failed
            // write (bad permissions, full disk, whatever) degrades to a nil
            // path rather than failing this photo's whole record: a missing
            // thumbnail is a blank grid tile, not a lost photo.
            let thumbnailPath: String? = thumbnailDir.flatMap { dir in
                do {
                    return try ThumbnailWriter.write(image: image, hash: hash, directory: dir, maxPixel: thumbnailMaxPixel)
                } catch {
                    FileHandle.standardError.write(Data(
                        "PhotobookEngine: thumbnail write failed for \(path): \(error)\n".utf8
                    ))
                    return nil
                }
            }

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
                clippedLow: clipping.low,
                clippedHigh: clipping.high,
                phash: Metrics.perceptualHash(image),
                featurePrint: vision.featurePrint.map(FeaturePrint.encode),
                thumbnailPath: thumbnailPath
            )
        }
    }

    /// Analyses every path in `paths` concurrently. Completion order under
    /// `concurrentPerform` is arbitrary, but the returned array is always in
    /// input order — callers correlate records to paths positionally, and a
    /// mismatch would silently attribute one photo's analysis to another.
    /// Every path yields exactly one record, `.ok` or `.failed`; a failure on
    /// one photo never aborts the batch and never throws out of this function.
    static func analyze(paths: [String], thumbnailDir: String? = nil) -> [PhotoRecord] {
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
                record = .ok(try analyzeOne(path: path, thumbnailDir: thumbnailDir))
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
