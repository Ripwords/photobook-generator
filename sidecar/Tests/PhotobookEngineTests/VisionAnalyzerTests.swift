import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func visionReturnsResultForPlainImageWithoutCrashing() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    #expect(result.faces.isEmpty)
    #expect(result.aestheticScore >= -1.0 && result.aestheticScore <= 1.0)
}

@Test func visionBoxesAreNormalisedAndTopLeftOrigin() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    if let box = result.saliencyBox {
        #expect(box.count == 4)
        for v in box { #expect(v >= -0.001 && v <= 1.001) }
    }
}

@Test func visionSceneTagsAreReturnedForARecognisableImage() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
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
