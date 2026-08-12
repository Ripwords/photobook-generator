import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func readsDimensionsForLandscape() throws {
    let exif = try ExifReader.read(path: fixture("landscape.jpg"))
    #expect(exif.pixelWidth == 1200)
    #expect(exif.pixelHeight == 800)
}

@Test func reportsPostOrientationDimensions() throws {
    // Stored 1200x800 with orientation 6; must be reported as 800x1200.
    let exif = try ExifReader.read(path: fixture("portrait-rot90.jpg"))
    #expect(exif.pixelWidth == 800)
    #expect(exif.pixelHeight == 1200)
}

@Test func exifThrowsOnMissingFile() {
    #expect(throws: ExifReader.ReadError.self) {
        try ExifReader.read(path: "/nonexistent/nope.jpg")
    }
}
