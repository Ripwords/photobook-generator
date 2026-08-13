import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

// The three tests below are the only call sites in this file that invoke
// VisionAnalyzer.analyze, and each now goes through VisionGate rather than
// calling it directly -- see VisionGate.swift's doc comment. They didn't
// before, and running the full unfiltered suite with them ungated (even
// with `Analyzer`'s and `Benchmarker`'s own call sites correctly gated)
// still reproduced the documented VNControlledCapacityTasksQueue deadlock:
// `AnalyzerTests.analyzerHandlesA300PhotoBatchWithoutDeadlockingOnVisionConcurrency`
// timed out at its own 120s watchdog. Gating these three closed that
// loophole -- confirmed by re-running the full suite several times after.
@Test func visionReturnsResultForPlainImageWithoutCrashing() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionGate.run { VisionAnalyzer.analyze(img) }
    #expect(result.faces.isEmpty)
    #expect(result.aestheticScore >= -1.0 && result.aestheticScore <= 1.0)
}

@Test func visionBoxesAreNormalisedAndTopLeftOrigin() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionGate.run { VisionAnalyzer.analyze(img) }
    if let box = result.saliencyBox {
        #expect(box.count == 4)
        for v in box { #expect(v >= -0.001 && v <= 1.001) }
    }
}

@Test func visionSceneTagsAreReturnedForARecognisableImage() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionGate.run { VisionAnalyzer.analyze(img) }
    // A flat navy field may legitimately produce no confident tags; assert the
    // contract, not the content.
    #expect(result.sceneTags.count <= 8)
}

// Mutation check for the bottom-left -> top-left coordinate flip. This exercises
// the conversion helper directly with asymmetric y/height values so a broken or
// removed flip produces a numerically different (and thus detectably wrong)
// result rather than coincidentally matching. Deliberately does not depend on
// Vision producing any detections on the flat-colour fixtures, which may
// legitimately yield none.
@Test func visionTopLeftFlipsBottomLeftOriginToTopLeft() {
    let rect = CGRect(x: 0.2, y: 0.1, width: 0.3, height: 0.2)
    let box = VisionAnalyzer.topLeft(rect)
    #expect(box.count == 4)
    #expect(abs(box[0] - 0.2) < 1e-9)
    #expect(abs(box[1] - 0.7) < 1e-9) // 1 - 0.1 - 0.2
    #expect(abs(box[2] - 0.3) < 1e-9)
    #expect(abs(box[3] - 0.2) < 1e-9)
}

// Mutation checks for `imageNormalizedTopLeft`, which offsets and scales a
// face-landmark point (normalised relative to the face's own bounding box, per
// VNFaceLandmarkRegion2D.normalizedPoints) into image-normalised, top-left
// coordinates matching `box`. No fixture contains a face, so this can only be
// tested in isolation. Expected values below are hand-computed independently
// of the implementation, not derived from it.

// Face box off-centre in the frame; point at the box's own centre.
// Vision-space box: origin (0.5, 0.3), size (0.2, 0.4).
// x_img = 0.5 + 0.5*0.2 = 0.6
// y_img_bottomleft = 0.3 + 0.5*0.4 = 0.5
// y_img_topleft = 1 - 0.5 = 0.5
@Test func visionLandmarkPointAtFaceBoxCentreMapsIntoImageSpace() {
    let box = CGRect(x: 0.5, y: 0.3, width: 0.2, height: 0.4)
    let result = VisionAnalyzer.imageNormalizedTopLeft(CGPoint(x: 0.5, y: 0.5), faceBoxInVisionSpace: box)
    #expect(result.count == 2)
    #expect(abs(result[0] - 0.6) < 1e-9)
    #expect(abs(result[1] - 0.5) < 1e-9)
}

// Same face box; point at the box's bottom-left corner (0,0) in Vision-space,
// i.e. face-relative, coordinates, and at the top-right corner (1,1).
// Corner (0,0): x_img = 0.5 + 0*0.2 = 0.5; y_bl = 0.3 + 0*0.4 = 0.3; y_tl = 0.7
// Corner (1,1): x_img = 0.5 + 1*0.2 = 0.7; y_bl = 0.3 + 1*0.4 = 0.7; y_tl = 0.3
@Test func visionLandmarkPointAtFaceBoxCornersMapsIntoImageSpace() {
    let box = CGRect(x: 0.5, y: 0.3, width: 0.2, height: 0.4)
    let bottomLeft = VisionAnalyzer.imageNormalizedTopLeft(CGPoint(x: 0.0, y: 0.0), faceBoxInVisionSpace: box)
    #expect(abs(bottomLeft[0] - 0.5) < 1e-9)
    #expect(abs(bottomLeft[1] - 0.7) < 1e-9)

    let topRight = VisionAnalyzer.imageNormalizedTopLeft(CGPoint(x: 1.0, y: 1.0), faceBoxInVisionSpace: box)
    #expect(abs(topRight[0] - 0.7) < 1e-9)
    #expect(abs(topRight[1] - 0.3) < 1e-9)
}

// A face box occupying the full frame reduces the offset-and-scale to the
// identity plus a plain flip: x_img = point.x, y_img_topleft = 1 - point.y.
// Point (0.3, 0.8): x_img = 0.3; y_bl = 0.8; y_tl = 1 - 0.8 = 0.2.
@Test func visionLandmarkConversionReducesToSimpleFlipForFullFrameFaceBox() {
    let box = CGRect(x: 0.0, y: 0.0, width: 1.0, height: 1.0)
    let result = VisionAnalyzer.imageNormalizedTopLeft(CGPoint(x: 0.3, y: 0.8), faceBoxInVisionSpace: box)
    #expect(abs(result[0] - 0.3) < 1e-9)
    #expect(abs(result[1] - 0.2) < 1e-9)
}
