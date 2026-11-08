import Foundation

/// One photo the editor wants a larger preview of. Rust already knows the
/// content hash, so it is passed in rather than re-read from the file.
struct PreviewItem: Codable, Equatable {
    let path: String
    let hash: String
}

struct PreviewRequest: Codable {
    let directory: String
    let maxPixel: Int
    let items: [PreviewItem]
}

/// Writes the editor's larger previews: the same upright, hash-keyed JPEGs
/// as the contact-sheet thumbnails, at the size the Graphics setting asks
/// for. The 400px thumbnail is stretched about 3x on a Retina cover panel,
/// which reads as blur in a photo that is sharp.
///
/// A Sony ARW carries a 1616px preview, so a 1600px request is read from
/// that and costs no RAW decode; a larger one falls back to a full decode
/// (`ImageLoader`'s floor). Measured on an M4: 130 ms versus about 500 ms.
enum Previewer {
    /// One path per item, in order; nil where the source could not be read
    /// or the file could not be written, so one unplugged drive costs only
    /// its own photos.
    static func preview(items: [PreviewItem], directory: String, maxPixel: Int) -> [String?] {
        items.map { item in
            let existing = (directory as NSString).appendingPathComponent("\(item.hash).jpg")
            if FileManager.default.fileExists(atPath: existing) { return existing }
            do {
                let image = try ImageLoader.loadThumbnail(path: item.path, maxPixel: maxPixel)
                // Never enlarge: a source smaller than the request is written
                // at its own size.
                let longEdge = min(maxPixel, max(image.width, image.height))
                return try ThumbnailWriter.write(image: image, hash: item.hash, directory: directory, maxPixel: longEdge)
            } catch {
                FileHandle.standardError.write(Data("PhotobookEngine: preview failed for \(item.path): \(error)\n".utf8))
                return nil
            }
        }
    }
}
