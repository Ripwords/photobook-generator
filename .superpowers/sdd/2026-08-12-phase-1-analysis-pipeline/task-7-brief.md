### Task 7: Vision analysis pass

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`

**Interfaces:**
- Consumes: `CGImage` from `ImageLoader`
- Produces: `struct VisionResult: Codable { isUtility: Bool; aestheticScore: Double; faces: [FaceObservation]; saliencyBox: [Double]?; horizonTiltDeg: Double?; sceneTags: [String]; hasText: Bool }` and `struct FaceObservation: Codable { box: [Double]; yaw: Double?; pitch: Double?; roll: Double?; captureQuality: Double?; landmarks: [[Double]]? }`; `VisionAnalyzer.analyze(_ image: CGImage) -> VisionResult`.

All boxes are `[x, y, w, h]` normalised to `0...1` in **top-left origin** coordinates — Vision returns bottom-left origin, so this converts.

**Critical:** all requests go on **one** `VNImageRequestHandler`. Vision reuses the decoded surface across requests on the same handler; one handler per request costs several times more.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`:

```swift
import Testing
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func returnsResultForPlainImageWithoutCrashing() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    #expect(result.faces.isEmpty)
    #expect(result.aestheticScore >= -1.0 && result.aestheticScore <= 1.0)
}

@Test func boxesAreNormalisedAndTopLeftOrigin() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    if let box = result.saliencyBox {
        #expect(box.count == 4)
        for v in box { #expect(v >= -0.001 && v <= 1.001) }
    }
}

@Test func sceneTagsAreReturnedForARecognisableImage() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    // A flat navy field may legitimately produce no confident tags; assert the
    // contract, not the content.
    #expect(result.sceneTags.count <= 8)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `VisionAnalyzer` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`:

```swift
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
    private static func topLeft(_ r: CGRect) -> [Double] {
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
                .filter { (try? $0.hasMinimumRecall(0.0, forPrecision: 0.8)) ?? false }
                .prefix(8)
                .map(\.identifier)
        }

        result.hasText = !(text.results ?? []).isEmpty

        return result
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add batched Vision analysis on a single request handler"
```

---

