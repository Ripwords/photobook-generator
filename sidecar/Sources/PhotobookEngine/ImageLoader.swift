import Foundation
import ImageIO
import CoreGraphics

enum ImageLoader {
    enum LoadError: Error {
        case unreadable(String)
        case decodeFailed(String)
    }

    /// Result of `loadThumbnailDetailed`, carrying the decoded image plus
    /// which of the two passes produced it -- used by the `benchmark`
    /// request to report per-format fallback rates without callers having
    /// to re-derive it from timing alone.
    struct LoadResult {
        let image: CGImage
        /// `true` if the embedded preview's long edge was below the floor
        /// and a second, full (`...FromImageAlways`) decode was required.
        let usedFullDecodeFallback: Bool
    }

    /// Decodes at most `maxPixel` on the long edge, with EXIF orientation applied.
    /// `kCGImageSourceCreateThumbnailWithTransform` reconciles EXIF orientation and
    /// HEIC container `irot`/`imir` properties, which can disagree.
    ///
    /// Convenience wrapper over `loadThumbnailDetailed` for the common case
    /// (`Analyzer`) that only needs the image, not which path produced it.
    static func loadThumbnail(path: String, maxPixel: Int, floorPixel: Int? = nil) throws -> CGImage {
        try loadThumbnailDetailed(path: path, maxPixel: maxPixel, floorPixel: floorPixel).image
    }

    /// Size-aware two-pass decode. This exists because the single-pass
    /// alternatives are both wrong for some format in this app's actual
    /// input mix:
    ///
    /// - `...FromImageAlways` unconditionally forces a full decode -- for a
    ///   RAW file, a complete demosaic of the full sensor image -- which is
    ///   then thrown away by downscaling to `maxPixel`. For a 24-61MP ARW
    ///   this is the whole performance bug this type exists to fix.
    /// - `...FromImageIfAbsent` alone is the naive fix, and it is wrong in
    ///   the other direction: for an out-of-camera JPEG or a HEIC written by
    ///   iOS, the "embedded thumbnail" it returns is typically the classic
    ///   ~160x120 EXIF thumbnail, not a usable preview. Silently analysing
    ///   postage stamps degrades sharpness/palette/saliency/face detection
    ///   catastrophically, with no test noticing because flat-colour
    ///   fixtures make those metrics degenerate anyway.
    ///
    /// So: request the embedded-preview thumbnail first (cheap -- it does
    /// NOT decode the full RAW/HEIC data), inspect its size, and only pay
    /// for a full decode if that preview's long edge falls short of
    /// `floorPixel` (defaults to `maxPixel`). A RAW's embedded preview is
    /// normally large enough that this never falls back; a plain JPEG's
    /// EXIF thumbnail normally always does -- both are expected, see
    /// `docs/PROJECT-STATUS.md` / the raw-performance report for measured
    /// per-format fallback rates.
    ///
    /// `kCGImageSourceCreateThumbnailWithTransform` is kept on BOTH passes.
    /// It reconciles EXIF orientation (and, for HEIC, the container's
    /// `irot`/`imir` against EXIF) -- dropping it on either pass would
    /// silently put portrait photos in landscape layout slots.
    static func loadThumbnailDetailed(path: String, maxPixel: Int, floorPixel: Int? = nil) throws -> LoadResult {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil) else {
            throw LoadError.unreadable(path)
        }
        let floor = floorPixel ?? maxPixel

        let previewOptions: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageIfAbsent: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        if let preview = CGImageSourceCreateThumbnailAtIndex(source, 0, previewOptions as CFDictionary),
           max(preview.width, preview.height) >= floor {
            return LoadResult(image: preview, usedFullDecodeFallback: false)
        }

        let fullOptions: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let full = CGImageSourceCreateThumbnailAtIndex(source, 0, fullOptions as CFDictionary) else {
            throw LoadError.decodeFailed(path)
        }
        return LoadResult(image: full, usedFullDecodeFallback: true)
    }
}
