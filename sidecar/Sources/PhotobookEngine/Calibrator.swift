import Foundation
import CoreGraphics

/// One detected face's raw geometry, face-box-relative -- matching where
/// `SmileProxy.geometry(for:)` does its arithmetic. Every field beyond
/// `path`/`box`/`outerLipPointCount` is `nil` together, with `unusableReason`
/// set, when `SmileProxy.geometry(for:)` could not score the face -- a row is
/// still emitted (see `Calibrator`'s doc comment for why a missing row would
/// be worse than a row full of nils).
struct CalibrationFaceRow: Codable {
    let path: String
    let box: [Double]
    let outerLipPointCount: Int
    let cornerY: Double?
    let centreY: Double?
    let width: Double?
    let lift: Double?
    let midlineY: Double?
    let midpointLift: Double?
    let confidence: Double?
    let unusableReason: String?
}

enum CalibrationRecord: Codable {
    /// `faces` may be empty -- Vision detected zero faces in this photo.
    /// That is a legitimate, common outcome (most fixtures and many real
    /// photos have no face in them at all) and must not be conflated with
    /// `.failed`, which means the file itself could not be read/decoded.
    case ok(faces: [CalibrationFaceRow])
    case failed(path: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, faces, path, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let faces):
            try c.encode("ok", forKey: .status)
            try c.encode(faces, forKey: .faces)
        case .failed(let path, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(path, forKey: .path)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(faces: try c.decode([CalibrationFaceRow].self, forKey: .faces))
        } else {
            self = .failed(path: try c.decode(String.self, forKey: .path),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

/// Dumps `SmileProxy`'s raw per-face geometry so the `confidence(for:)`
/// calibration constants -- `[-0.15, 0.35]` mapped onto `0...1`, and the
/// `0.5` smiling threshold in `SmileProxy.fraction` -- can be set from real
/// faces instead of guessed a second time. See `SmileProxy.swift`'s doc
/// comment: those constants were never measured, and the comment claiming
/// they map "roughly [-0.15, 0.35]" is not backed by any observation.
///
/// This is diagnostic-only. It must never be consulted by production
/// analysis (`Analyzer.analyzeOne` does not call it), and it must never
/// compute lift itself -- every geometry value in a `CalibrationFaceRow`
/// comes from `SmileProxy.geometry(for:)`, the exact function
/// `confidence(for:)` is a thin wrapper over, so the diagnostic can never
/// silently drift from the thing it is measuring. `CalibratorTests` asserts
/// this directly by comparing a row's `confidence` against
/// `SmileProxy.confidence(for:)` for the same input.
enum Calibrator {
    /// Builds one row per face already present in `faces` (e.g. from a
    /// decoded `VisionResult.faces`), stamping each with `path`. Not private
    /// so tests can exercise it directly without going through image decode.
    static func rows(for faces: [FaceObservation], path: String) -> [CalibrationFaceRow] {
        faces.map { face in
            switch SmileProxy.geometry(for: face) {
            case .success(let g):
                return CalibrationFaceRow(
                    path: path,
                    box: face.box,
                    outerLipPointCount: face.outerLips?.count ?? 0,
                    cornerY: g.cornerY,
                    centreY: g.centreY,
                    width: g.width,
                    lift: g.lift,
                    midlineY: g.midlineY,
                    midpointLift: g.midpointLift,
                    confidence: g.confidence,
                    unusableReason: nil
                )
            case .failure(let reason):
                return CalibrationFaceRow(
                    path: path,
                    box: face.box,
                    outerLipPointCount: face.outerLips?.count ?? 0,
                    cornerY: nil,
                    centreY: nil,
                    width: nil,
                    lift: nil,
                    midlineY: nil,
                    midpointLift: nil,
                    confidence: nil,
                    unusableReason: reason.rawValue
                )
            }
        }
    }

    /// Test-only seam, same purpose and shape as `Benchmarker.benchmarkOne`'s
    /// `visionOverride` -- see that function's doc comment for why real
    /// Vision calls are stubbed out in tests rather than exercised here.
    /// Production (`Handler.swift`) never passes this, so a real request
    /// always runs the true `VisionGate.run { VisionAnalyzer.analyze(image) }`
    /// path, identical to `Analyzer.analyzeOne`'s.
    static func calibrateOne(
        path: String,
        visionOverride: ((CGImage) -> VisionResult)? = nil
    ) -> CalibrationRecord {
        do {
            return try autoreleasepool {
                let image = try ImageLoader.loadThumbnail(path: path, maxPixel: Analyzer.analysisMaxPixel)
                let vision = (visionOverride ?? { img in VisionGate.run { VisionAnalyzer.analyze(img) } })(image)
                return .ok(faces: rows(for: vision.faces, path: path))
            }
        } catch {
            return .failed(path: path, message: String(describing: error))
        }
    }

    /// One record per input path, in input order -- same positional contract
    /// as `Analyzer.analyze` and `Benchmarker.benchmark`. Runs sequentially,
    /// like `Benchmarker.benchmark`: this is a one-off calibration run over a
    /// user-supplied folder, not a throughput-sensitive batch, so there is no
    /// reason to take on `concurrentPerform`'s complexity here.
    static func calibrate(
        paths: [String],
        visionOverride: ((CGImage) -> VisionResult)? = nil
    ) -> [CalibrationRecord] {
        paths.map { calibrateOne(path: $0, visionOverride: visionOverride) }
    }
}
