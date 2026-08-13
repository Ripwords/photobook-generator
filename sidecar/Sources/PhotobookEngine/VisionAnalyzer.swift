import Foundation
import Vision
import CoreGraphics

/// `box` and `outerLips` are both normalised to `0...1` in **image** space with a
/// **top-left** origin. `outerLips` points are NOT face-bounding-box-relative:
/// Vision's `VNFaceLandmarkRegion2D.normalizedPoints` are relative to the face's
/// own bounding box, so they are offset and scaled into image space (via the
/// Vision-space, i.e. bottom-left-origin, face bounding box) before being
/// flipped to top-left — see `VisionAnalyzer.imageNormalizedTopLeft`. Keeping
/// `box` and `outerLips` in the same coordinate space is deliberate: overlaying,
/// cropping to, or rendering landmarks against the image only works if both
/// fields agree on what "normalised" means.
///
/// Only the outer lip contour (`VNFaceLandmarks2D.outerLips`, typically 8-12
/// points) is carried, not Vision's full ~65-76 point `allPoints` set. The
/// only consumer, `SmileProxy`, only ever needed the mouth contour, and
/// carrying every landmark would mean serialising and storing dozens of
/// unused points per face for nothing downstream to read.
struct FaceObservation: Codable {
    let box: [Double]
    let yaw: Double?
    let pitch: Double?
    let roll: Double?
    let captureQuality: Double?
    let outerLips: [[Double]]?
}

struct VisionResult: Codable {
    var isUtility: Bool = false
    var aestheticScore: Double = 0
    var faces: [FaceObservation] = []
    var saliencyBox: [Double]?
    var horizonTiltDeg: Double?
    var sceneTags: [String] = []
    var hasText: Bool = false
}

enum VisionAnalyzer {
    /// Vision uses a bottom-left origin; everything downstream uses top-left.
    /// Not `private` so the coordinate-flip mutation-check test can call it directly.
    static func topLeft(_ r: CGRect) -> [Double] {
        [Double(r.origin.x), Double(1.0 - r.origin.y - r.height),
         Double(r.width), Double(r.height)]
    }

    /// Converts a face-landmark point — normalised relative to the **face's own
    /// bounding box** in `VNFaceLandmarkRegion2D.normalizedPoints` — into
    /// **image**-normalised, top-left-origin coordinates matching `box`.
    ///
    /// `faceBoxInVisionSpace` must be the raw Vision-space (bottom-left-origin)
    /// face bounding box, i.e. `VNFaceObservation.boundingBox` — NOT the
    /// already-flipped output of `topLeft(_:)`. Mixing the two silently produces
    /// a wrong offset because `topLeft` has already re-based `y` to top-left.
    ///
    /// Not `private` so the offset-and-scale mutation-check tests can call it
    /// directly (no fixture contains a face, so this can only be tested in
    /// isolation with hand-computed values).
    static func imageNormalizedTopLeft(_ point: CGPoint, faceBoxInVisionSpace box: CGRect) -> [Double] {
        let xImage = box.origin.x + point.x * box.width
        let yImageBottomLeft = box.origin.y + point.y * box.height
        let yImageTopLeft = 1.0 - yImageBottomLeft
        return [Double(xImage), Double(yImageTopLeft)]
    }

    static func analyze(_ image: CGImage) -> VisionResult {
        var result = VisionResult()

        let aesthetics = VNCalculateImageAestheticsScoresRequest()
        let faces = VNDetectFaceRectanglesRequest()
        let saliency = VNGenerateAttentionBasedSaliencyImageRequest()
        let horizon = VNDetectHorizonRequest()
        let classify = VNClassifyImageRequest()
        let text = VNRecognizeTextRequest()
        text.recognitionLevel = .fast

        // One handler for everything: Vision reuses the decoded surface across
        // requests, so a second perform() on the same handler is nearly free.
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        try? handler.perform([aesthetics, faces, saliency, horizon, classify, text])

        // Landmarks and capture quality must be seeded with the rectangles
        // request's observations. Running them standalone returns their own
        // independently-ordered face lists, and pairing those by array index
        // silently attaches one person's quality score to another person's box.
        let detected = faces.results ?? []
        let landmarks = VNDetectFaceLandmarksRequest()
        let quality = VNDetectFaceCaptureQualityRequest()
        landmarks.inputFaceObservations = detected
        quality.inputFaceObservations = detected
        if !detected.isEmpty {
            try? handler.perform([landmarks, quality])
        }

        if let obs = aesthetics.results?.first {
            result.aestheticScore = Double(obs.overallScore)
            result.isUtility = obs.isUtility
        }

        // Seeded requests return observations in the same order as the input,
        // so index correspondence is now guaranteed rather than assumed.
        let qualityResults = quality.results ?? []
        let landmarkResults = landmarks.results ?? []

        result.faces = detected.enumerated().map { index, face in
            // face.boundingBox is the raw Vision-space (bottom-left-origin) box;
            // it — not the flipped `topLeft(face.boundingBox)` below — is what
            // normalizedPoints are relative to. Only outerLips is kept: it's
            // the mouth contour SmileProxy needs, not Vision's full ~65-76
            // point face mesh (jaw, eyebrows, eyes, nose, etc.).
            let outerLips: [[Double]]? = index < landmarkResults.count
                ? landmarkResults[index].landmarks?.outerLips?.normalizedPoints
                    .map { imageNormalizedTopLeft($0, faceBoxInVisionSpace: face.boundingBox) }
                : nil
            let q: Double? = index < qualityResults.count
                ? qualityResults[index].faceCaptureQuality.map(Double.init)
                : nil
            return FaceObservation(
                box: topLeft(face.boundingBox),
                yaw: face.yaw.map { Double(truncating: $0) },
                pitch: face.pitch.map { Double(truncating: $0) },
                roll: face.roll.map { Double(truncating: $0) },
                captureQuality: q,
                outerLips: outerLips
            )
        }

        if let salient = saliency.results?.first?.salientObjects?.first {
            result.saliencyBox = topLeft(salient.boundingBox)
        }

        if let obs = horizon.results?.first {
            result.horizonTiltDeg = Double(obs.angle) * 180.0 / .pi
        }

        // Per-class calibration varies widely across Vision's 1,303 labels, so
        // filter by precision rather than a flat confidence threshold.
        if let classifications = classify.results {
            result.sceneTags = classifications
                .filter { $0.hasMinimumRecall(0.0, forPrecision: 0.8) }
                .prefix(8)
                .map(\.identifier)
        }

        result.hasText = !(text.results ?? []).isEmpty

        return result
    }
}
