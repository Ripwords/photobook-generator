import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()   // PhotobookEngineTests
        .deletingLastPathComponent()   // Tests
        .deletingLastPathComponent()   // sidecar
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func loadsLandscapeAtRequestedSize() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 512)
    #expect(max(img.width, img.height) <= 512)
    #expect(img.width > img.height)
}

@Test func appliesExifOrientationSoPortraitIsTaller() throws {
    // File is stored landscape with EXIF orientation 6 (rotate 90 CW).
    let img = try ImageLoader.loadThumbnail(path: fixture("portrait-rot90.jpg"), maxPixel: 512)
    #expect(img.height > img.width, "orientation must be applied before we report dimensions")
}

@Test func throwsOnMissingFile() {
    #expect(throws: ImageLoader.LoadError.self) {
        try ImageLoader.loadThumbnail(path: "/nonexistent/nope.jpg", maxPixel: 512)
    }
}
