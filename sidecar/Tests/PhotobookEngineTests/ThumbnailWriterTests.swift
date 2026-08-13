import Testing
import Foundation
import ImageIO
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

/// A fresh scratch directory per test so parallel test runs never collide on
/// the same thumbnail path, and each test starts from a clean slate.
private func scratchDirectory() -> String {
    let dir = NSTemporaryDirectory() + "thumbnail-writer-tests-\(UUID().uuidString)"
    return dir
}

private func loadedDimensions(atPath path: String) throws -> (width: Int, height: Int) {
    let url = URL(fileURLWithPath: path) as CFURL
    let source = try #require(CGImageSourceCreateWithURL(url, nil))
    let props = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
    let width = try #require(props[kCGImagePropertyPixelWidth] as? Int)
    let height = try #require(props[kCGImagePropertyPixelHeight] as? Int)
    return (width, height)
}

@Test func thumbnailWriterWritesJpegAtHashKeyedPathWithinLongEdgeBudget() throws {
    let image = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1536)
    let dir = scratchDirectory()

    let path = try ThumbnailWriter.write(image: image, hash: "abc123", directory: dir, maxPixel: 400)

    #expect(path == dir + "/abc123.jpg")
    #expect(FileManager.default.fileExists(atPath: path))

    let dims = try loadedDimensions(atPath: path)
    // landscape.jpg is 1200x800 -- wider than tall -- so the long edge (width)
    // must land exactly on the 400px budget, not merely under it.
    #expect(dims.width == 400)
    #expect(dims.height < dims.width)
    #expect(max(dims.width, dims.height) <= 400)
}

@Test func thumbnailWriterDoesNotRewriteAnExistingThumbnail() throws {
    let image = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1536)
    let dir = scratchDirectory()
    try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
    let existingPath = dir + "/deadbeef.jpg"
    // Sentinel bytes that are NOT a valid JPEG. If `write` skips re-encoding
    // as it must when the file already exists, these bytes survive
    // untouched -- if a mutant re-encodes unconditionally, this sentinel
    // would be overwritten with real JPEG bytes and the test would fail.
    let sentinel = Data("not-a-real-jpeg".utf8)
    try sentinel.write(to: URL(fileURLWithPath: existingPath))

    let path = try ThumbnailWriter.write(image: image, hash: "deadbeef", directory: dir, maxPixel: 400)

    #expect(path == existingPath)
    let contents = try Data(contentsOf: URL(fileURLWithPath: existingPath))
    #expect(contents == sentinel, "existing thumbnail must not be re-encoded")
}

@Test func thumbnailWriterThrowsWhenTargetDirectoryCannotBeCreated() throws {
    let image = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1536)
    // A regular file sitting where the directory needs to be created makes
    // `FileManager.createDirectory` fail -- a reliable, portable way to force
    // the failure path without relying on filesystem permissions.
    let blockerPath = scratchDirectory()
    try Data().write(to: URL(fileURLWithPath: blockerPath))
    defer { try? FileManager.default.removeItem(atPath: blockerPath) }

    #expect(throws: (any Error).self) {
        try ThumbnailWriter.write(image: image, hash: "x", directory: blockerPath, maxPixel: 400)
    }
}
