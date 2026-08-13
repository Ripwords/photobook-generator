import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Writes small JPEG thumbnails for the results-view contact sheet. The
/// webview cannot decode RAW/HEIC originals at all, and loading hundreds of
/// full-size decoded images is not viable, so `Analyzer` reuses the CGImage
/// it already decoded for analysis and hands it here to be downsampled once
/// more and written to disk as a JPEG small enough for a grid tile.
enum ThumbnailWriter {
    enum WriteError: Error {
        case resizeFailed
        case destinationCreationFailed
        case finalizeFailed
    }

    /// Writes `image` as a JPEG to `<directory>/<hash>.jpg`, resized so its
    /// long edge is `maxPixel`. Keyed by content hash (not source path) so a
    /// moved or re-imported file reuses the existing thumbnail, matching how
    /// the feature cache in Rust is keyed. If a file already exists at that
    /// path, it is trusted as-is and NOT re-encoded -- that existence check
    /// is far cheaper than decoding + resizing + re-compressing a JPEG that
    /// is presumably already correct.
    static func write(image: CGImage, hash: String, directory: String, maxPixel: Int) throws -> String {
        let path = (directory as NSString).appendingPathComponent("\(hash).jpg")
        if FileManager.default.fileExists(atPath: path) {
            return path
        }

        try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)

        let resized = try resize(image, maxPixel: maxPixel)
        let url = URL(fileURLWithPath: path) as CFURL
        guard let destination = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil) else {
            throw WriteError.destinationCreationFailed
        }
        let options: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: 0.8]
        CGImageDestinationAddImage(destination, resized, options as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw WriteError.finalizeFailed
        }

        return path
    }

    private static func resize(_ image: CGImage, maxPixel: Int) throws -> CGImage {
        let longEdge = max(image.width, image.height)
        guard longEdge > 0 else { throw WriteError.resizeFailed }

        let scale = Double(maxPixel) / Double(longEdge)
        let newWidth = max(1, Int((Double(image.width) * scale).rounded()))
        let newHeight = max(1, Int((Double(image.height) * scale).rounded()))

        guard let context = CGContext(
            data: nil,
            width: newWidth,
            height: newHeight,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            throw WriteError.resizeFailed
        }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: newWidth, height: newHeight))

        guard let resized = context.makeImage() else { throw WriteError.resizeFailed }
        return resized
    }
}
