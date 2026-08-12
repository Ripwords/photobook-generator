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

@Test func exifReadsSouthernLatitudeAsNegative() throws {
    let exif = try ExifReader.read(path: fixture("gps-south-west.jpg"))
    let lat = try #require(exif.latitude)
    #expect(lat < 0)
    #expect(abs(lat - (-33.8688)) < 0.0001)
}

@Test func exifReadsWesternLongitudeAsNegative() throws {
    let exif = try ExifReader.read(path: fixture("gps-south-west.jpg"))
    let lon = try #require(exif.longitude)
    #expect(lon < 0)
    #expect(abs(lon - (-151.2093)) < 0.0001)
}

@Test func exifParsesCaptureDate() throws {
    let exif = try ExifReader.read(path: fixture("gps-south-west.jpg"))
    let date = try #require(exif.captureDate)
    // 2024-06-15 10:30:00 UTC, the exact timestamp written into the fixture's
    // DateTimeOriginal tag by scripts/make-fixtures.swift.
    #expect(date.timeIntervalSince1970 == 1718447400)
}

@Test func exifFixturesWithoutGpsReportNilCoordinates() throws {
    let landscape = try ExifReader.read(path: fixture("landscape.jpg"))
    #expect(landscape.latitude == nil)
    #expect(landscape.longitude == nil)

    let portrait = try ExifReader.read(path: fixture("portrait-rot90.jpg"))
    #expect(portrait.latitude == nil)
    #expect(portrait.longitude == nil)
}
