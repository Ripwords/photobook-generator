import Foundation
import Vision
import CoreGraphics

struct FaceObservation: Codable {
    let box: [Double]
    let yaw: Double?
    let pitch: Double?
    let roll: Double?
    let captureQuality: Double?
    let landmarks: [[Double]]?
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
            let points: [[Double]]? = index < landmarkResults.count
                ? landmarkResults[index].landmarks?.allPoints?.normalizedPoints
                    .map { [Double($0.x), Double(1.0 - $0.y)] }
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
                landmarks: points
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
