import Foundation
import ImageIO
import CoreGraphics

enum ImageLoader {
    enum LoadError: Error {
        case unreadable(String)
        case decodeFailed(String)
    }

    /// Decodes at most `maxPixel` on the long edge, with EXIF orientation applied.
    /// `kCGImageSourceCreateThumbnailWithTransform` reconciles EXIF orientation and
    /// HEIC container `irot`/`imir` properties, which can disagree.
    static func loadThumbnail(path: String, maxPixel: Int) throws -> CGImage {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil) else {
            throw LoadError.unreadable(path)
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            throw LoadError.decodeFailed(path)
        }
        return image
    }
}
