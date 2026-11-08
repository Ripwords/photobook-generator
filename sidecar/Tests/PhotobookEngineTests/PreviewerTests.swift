import Testing
import Foundation
import ImageIO
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

private func scratchDirectory() -> String {
    NSTemporaryDirectory() + "previewer-tests-\(UUID().uuidString)"
}

private func dimensions(_ path: String) throws -> (width: Int, height: Int) {
    let source = try #require(CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil))
    let props = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
    return (try #require(props[kCGImagePropertyPixelWidth] as? Int), try #require(props[kCGImagePropertyPixelHeight] as? Int))
}

/// landscape.jpg is 1200x800. 600 is neither the thumbnail size nor the
/// source size, so a writer that ignored `maxPixel` cannot land on it.
@Test func previewWritesAtTheRequestedLongEdge() throws {
    let dir = scratchDirectory()
    let paths = Previewer.preview(items: [PreviewItem(path: fixture("landscape.jpg"), hash: "aa")], directory: dir, maxPixel: 600)

    #expect(paths == [dir + "/aa.jpg"])
    let dims = try dimensions(try #require(paths[0]))
    #expect(dims.width == 600)
    #expect(dims.height == 400)
}

/// A preview larger than the photo would be an upscale: more bytes on disk
/// and no sharper on screen.
@Test func previewNeverEnlargesASmallerSource() throws {
    let dir = scratchDirectory()
    let paths = Previewer.preview(items: [PreviewItem(path: fixture("landscape.jpg"), hash: "bb")], directory: dir, maxPixel: 2400)

    let dims = try dimensions(try #require(paths[0]))
    #expect(max(dims.width, dims.height) == 1200)
}

/// The editor shows the photo upright, so the preview carries the EXIF
/// orientation baked in, as the thumbnail does.
@Test func previewIsWrittenUpright() throws {
    let dir = scratchDirectory()
    let paths = Previewer.preview(items: [PreviewItem(path: fixture("portrait-rot90.jpg"), hash: "cc")], directory: dir, maxPixel: 600)

    let dims = try dimensions(try #require(paths[0]))
    #expect(dims.height > dims.width)
}

/// An unplugged drive must cost that one photo its preview, not the batch.
@Test func previewAnswersNullForAMissingFileAndKeepsItsNeighbours() throws {
    let dir = scratchDirectory()
    let paths = Previewer.preview(
        items: [
            PreviewItem(path: "/Volumes/Gone/missing.ARW", hash: "dd"),
            PreviewItem(path: fixture("landscape.jpg"), hash: "ee"),
        ],
        directory: dir,
        maxPixel: 600
    )

    #expect(paths == [nil, dir + "/ee.jpg"])
}

@Test func previewRequestDispatchesToThePreviewer() throws {
    let dir = scratchDirectory()
    let line = """
    {"id":"p1","kind":"preview","preview":{"directory":"\(dir)","maxPixel":600,"items":[{"path":"\(fixture("landscape.jpg"))","hash":"ff"}]}}
    """
    let response = handle(line: line)

    #expect(response.id == "p1")
    guard case .previewed(let paths) = response.result else {
        Issue.record("expected previewed, got \(response.result)")
        return
    }
    #expect(paths == [dir + "/ff.jpg"])
}

@Test func previewRequestWithoutItsPayloadIsAnError() throws {
    let response = handle(line: #"{"id":"p2","kind":"preview"}"#)
    guard case .error = response.result else {
        Issue.record("expected error, got \(response.result)")
        return
    }
}
