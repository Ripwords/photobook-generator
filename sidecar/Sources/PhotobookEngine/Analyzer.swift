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
        let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)

        // autoreleasepool matters: CGImage/CIImage allocations do not get an
        // autorelease pool on threads spawned by concurrentPerform, so
        // without this memory grows unbounded across a large batch.
        return autoreleasepool {
            let vision = VisionAnalyzer.analyze(image)
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
