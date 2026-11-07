import Testing
import Foundation
import ImageIO
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

// MARK: - UTC offset (OffsetTimeOriginal)

/// Writes a small JPEG carrying the given EXIF dictionary into a fresh temp
/// directory, so the offset tests read a real file through ImageIO rather
/// than poking the parser directly.
private func exifTaggedJPEG(_ exif: [CFString: Any]) throws -> String {
    let dir = (NSTemporaryDirectory() as NSString)
        .appendingPathComponent("exif-offset-\(UUID().uuidString)")
    try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
    let path = (dir as NSString).appendingPathComponent("tagged.jpg")
    let context = try #require(CGContext(
        data: nil, width: 64, height: 48, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
    ))
    context.setFillColor(red: 0.5, green: 0.5, blue: 0.5, alpha: 1)
    context.fill(CGRect(x: 0, y: 0, width: 64, height: 48))
    let destination = try #require(CGImageDestinationCreateWithURL(
        URL(fileURLWithPath: path) as CFURL, "public.jpeg" as CFString, 1, nil
    ))
    CGImageDestinationAddImage(destination, try #require(context.makeImage()),
                               [kCGImagePropertyExifDictionary: exif] as CFDictionary)
    #expect(CGImageDestinationFinalize(destination))
    return path
}

@Test func exifOffsetParsesSignedHoursAndMinutes() {
    #expect(ExifReader.offsetSeconds("+09:00") == 32_400)
    #expect(ExifReader.offsetSeconds("-05:30") == -19_800)
    #expect(ExifReader.offsetSeconds("+00:00") == 0)
    #expect(ExifReader.offsetSeconds("+05:45") == 20_700)
}

@Test func exifOffsetRefusesWhatIsNotAnOffset() {
    #expect(ExifReader.offsetSeconds("") == nil)
    #expect(ExifReader.offsetSeconds("   :  ") == nil)
    #expect(ExifReader.offsetSeconds("09:00") == nil)
    #expect(ExifReader.offsetSeconds("+9") == nil)
    #expect(ExifReader.offsetSeconds("+15:00") == nil)
    #expect(ExifReader.offsetSeconds("+09:60") == nil)
}

@Test func exifReadsOffsetTimeOriginalAndKeepsTheWallClock() throws {
    let path = try exifTaggedJPEG([
        kCGImagePropertyExifDateTimeOriginal: "2026:03:12 18:30:00",
        kCGImagePropertyExifOffsetTimeOriginal: "+09:00",
    ])
    defer { try? FileManager.default.removeItem(atPath: (path as NSString).deletingLastPathComponent) }
    let exif = try ExifReader.read(path: path)
    #expect(exif.captureOffsetSec == 32_400)
    // The wall clock is still read as if it were UTC: titles and hours read
    // it as local time, and the offset is applied on the Rust side only to
    // order photos.
    #expect(try #require(exif.captureDate).timeIntervalSince1970 == 1_773_340_200)
}

@Test func exifFallsBackToOffsetTimeWhenTheOriginalIsMissing() throws {
    let path = try exifTaggedJPEG([
        kCGImagePropertyExifDateTimeOriginal: "2026:03:12 18:30:00",
        kCGImagePropertyExifOffsetTime: "-03:00",
    ])
    defer { try? FileManager.default.removeItem(atPath: (path as NSString).deletingLastPathComponent) }
    #expect(try ExifReader.read(path: path).captureOffsetSec == -10_800)
}

@Test func exifReportsNoOffsetWhenTheFileHasNone() throws {
    #expect(try ExifReader.read(path: fixture("gps-south-west.jpg")).captureOffsetSec == nil)
}
