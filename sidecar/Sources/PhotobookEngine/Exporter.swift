import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Writes the files the user actually uploads to the printer.
///
/// This crops the user's own photograph and re-encodes it. It does NOT
/// composite, synthesise, or place anything on a page canvas. An earlier
/// design rendered full-page images with the photo positioned on them; that
/// was abandoned because it re-encodes the photograph into a flattened page,
/// so the book would print a picture of a picture. The photobook uses the
/// camera's images directly, as a memory. Decode, crop, re-encode, write --
/// nothing else.
///
/// **Format rule.** Lossy source (JPEG, HEIC) exports as JPEG at quality
/// 0.95; lossless source (PNG, TIFF, RAW) exports as PNG. Re-encoding a
/// camera JPEG to PNG inflates it roughly fivefold without recovering
/// quality that was already lost, and encoding a RAW-derived crop to JPEG
/// introduces the first generation of loss for no reason.
///
/// **Colour.** Output is sRGB with an embedded ICC profile, converted at
/// export from whatever the source is. A wide-gamut source (Display P3, Adobe
/// RGB) that is re-encoded without conversion keeps its original profile, and
/// a print pipeline expecting sRGB then renders it wrong.
///
/// **Orientation before cropping.** EXIF orientations 5-8 swap width and
/// height. Cropping the stored pixels and orienting afterwards silently crops
/// the wrong region of every portrait photo, and the result does not look
/// broken -- it is just the wrong part of the picture. `ImageLoader
/// .loadOriented` applies orientation during the decode, so the crop window
/// below is always resolved against the DISPLAYED image.
enum Exporter {
    /// Which container the crop is written into.
    enum OutputFormat: Equatable {
        case jpeg
        case png

        var utType: UTType { self == .jpeg ? .jpeg : .png }
        var pathExtension: String { self == .jpeg ? "jpg" : "png" }

        /// JPEG's only meaningful knob. 0.95 is high enough that a second
        /// generation of JPEG on an already-lossy source is not visible at
        /// 300 DPI, without the file-size cliff of 1.0.
        var lossyQuality: Double? { self == .jpeg ? 0.95 : nil }
    }

    enum ExportError: Error, CustomStringConvertible {
        case emptyCropWindow(String)
        case cropWindowOutOfRange(String)
        case duplicateOutputPath(String)
        case cropFailed
        case colorConversionFailed
        case destinationCreationFailed
        case encodeFailed

        var description: String {
            switch self {
            case .emptyCropWindow(let detail): return "crop window is empty (\(detail))"
            case .cropWindowOutOfRange(let detail):
                return "crop window is outside the photo's normalised bounds (\(detail))"
            case .duplicateOutputPath(let path):
                return "another item in this export already wrote \(path)"
            case .cropFailed: return "crop window fell outside the decoded image"
            case .colorConversionFailed: return "could not convert the crop to sRGB"
            case .destinationCreationFailed: return "could not create the output file"
            case .encodeFailed: return "could not encode the output file"
            }
        }
    }

    /// Chooses the output container for a source's type.
    ///
    /// Pure and separated from any file access so the rule is testable for
    /// RAW and TIFF, which have no fixture in this repo. (HEIC does --
    /// `export-quadrants.heic` -- because it is the default iPhone format and
    /// so plausibly the commonest real input.)
    ///
    /// `nil` (an unrecognised container) falls to PNG: PNG never adds a
    /// generation of loss the source did not already have, so it is the safe
    /// answer when we cannot tell.
    ///
    /// Note that `public.heic` does NOT declare conformance to `public.heif`
    /// in the system UTI database -- measured, not assumed -- so both are
    /// listed explicitly. Conformance (rather than equality) is what catches
    /// vendor subtypes such as `com.sony.arw-raw-image`, which conforms to
    /// `public.camera-raw-image`.
    static func outputFormat(for sourceType: UTType?) -> OutputFormat {
        guard let sourceType else { return .png }
        let lossy: [UTType] = [.jpeg, .heic, .heif, .webP]
        // `conforms(to:)` is reflexive, so this also matches the types exactly.
        return lossy.contains(where: { sourceType.conforms(to: $0) }) ? .jpeg : .png
    }

    /// Exports every item, returning exactly one record per item, in input
    /// order. A missing or undecodable source produces a `.failed` record; it
    /// never throws out of this function and never terminates the process.
    ///
    /// **Filename collisions fail, they do not overwrite.** Two items whose
    /// output paths collide would otherwise both report `.ok` with the same
    /// path, and a 60-photo book would quietly ship 59 files with nothing in
    /// the response saying which photo vanished — a loss the user could only
    /// find by counting files. The first item wins (so the outcome is
    /// deterministic and order-stable); every later claimant gets `.failed`
    /// naming the path it collided on.
    ///
    /// Failing rather than uniquifying is deliberate: filenames come from the
    /// layout engine, which numbers them by page and slot, so a collision is
    /// an upstream bug. A silently renamed `p01-1.jpg` in a print upload is
    /// worse than a reported failure, because it looks like it worked.
    ///
    /// The claim set is per-request, not per-directory: re-exporting a book
    /// over a previous run's output must still overwrite, since "regenerate"
    /// is expected to replace what is there.
    ///
    /// **Items run in parallel, claims do not.** Paths are claimed in input
    /// order before any item is decoded, so "first wins" still means first in
    /// the request, not first to finish. A claim is taken whether or not the
    /// item's source then turns out to be readable. At most
    /// `concurrencyLimit` items are in flight, because each holds a
    /// full-resolution decode plus its sRGB copy in memory.
    static func export(_ request: ExportRequest) -> [ExportRecord] {
        do {
            try FileManager.default.createDirectory(
                atPath: request.outputDir, withIntermediateDirectories: true
            )
        } catch {
            // A directory we cannot create fails every item, but each item
            // still gets its own record so the caller can report per photo.
            return request.items.map {
                .failed(filename: $0.filename, message: "could not create \(request.outputDir): \(error)")
            }
        }

        var claimed = Set<String>()
        let claims: [Result<(path: String, format: OutputFormat), ExportError>] = request.items.map { item in
            let format = sourceFormat(of: item.sourcePath)
            let path = outputPath(for: item.filename, format: format, in: request.outputDir)
            return claimed.insert(path).inserted
                ? .success((path, format))
                : .failure(.duplicateOutputPath(path))
        }

        var records = [ExportRecord?](repeating: nil, count: request.items.count)
        let lock = NSLock()
        let slots = DispatchSemaphore(value: concurrencyLimit)
        // Same pattern, and the same false-positive capture warning, as
        // `Analyzer.analyze`: every write is under `lock`, and
        // `concurrentPerform` returns only after every iteration has.
        DispatchQueue.concurrentPerform(iterations: request.items.count) { index in
            let item = request.items[index]
            let record: ExportRecord
            switch claims[index] {
            case .failure(let error):
                record = .failed(filename: item.filename, message: error.description)
            case .success(let claim):
                slots.wait()
                defer { slots.signal() }
                do {
                    record = try exportOne(item, to: claim.path, format: claim.format)
                } catch {
                    record = .failed(filename: item.filename, message: describe(error))
                }
            }
            lock.lock()
            records[index] = record
            lock.unlock()
        }
        return records.compactMap { $0 }
    }

    /// Items exported at once. Measured on 27 camera JPEGs; see
    /// `docs/PROJECT-STATUS.md` before changing it.
    static let concurrencyLimit = 4

    private static func describe(_ error: Error) -> String {
        switch error {
        case ImageLoader.LoadError.unreadable(let path): return "unreadable: \(path)"
        case ImageLoader.LoadError.decodeFailed(let path): return "could not decode: \(path)"
        case let e as ExportError: return e.description
        default: return "\(error)"
        }
    }

    /// `path` is already claimed by `export`, so a collision never truncates
    /// the file the first item wrote. The claim is on the resolved path, so
    /// the same stem from a JPEG and a PNG source is not a collision -- they
    /// become `p01.jpg` and `p01.png` and both survive.
    private static func exportOne(
        _ item: ExportItem, to path: String, format: OutputFormat
    ) throws -> ExportRecord {
        // The autoreleasepool encloses the DECODE, not just the encode. Full
        // decompression happens inside `loadOriented`, and without a pool
        // around it the temporary surfaces accumulate for the whole batch.
        let image = try autoreleasepool { () throws -> CGImage in
            let decoded = try ImageLoader.loadOriented(path: item.sourcePath)
            let rect = try cropRect(for: item, in: decoded)
            guard let cropped = decoded.cropping(to: rect) else { throw ExportError.cropFailed }
            return cropped
        }

        let srgb = try convertToSRGB(image, format: format)
        let bytes = try encode(srgb, to: path, format: format)
        return .ok(path: path, width: srgb.width, height: srgb.height, bytes: bytes)
    }

    private static func sourceFormat(of path: String) -> OutputFormat {
        guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let identifier = CGImageSourceGetType(source) as String?
        else {
            // Fall back to the filename's extension only when ImageIO cannot
            // name the container it just decoded, which in practice does not
            // happen -- but guessing from the extension is better than
            // defaulting a JPEG to PNG.
            return outputFormat(for: UTType(filenameExtension: (path as NSString).pathExtension))
        }
        return outputFormat(for: UTType(identifier))
    }

    /// Resolves the item's crop window into pixels of the DECODED (already
    /// oriented) image, top-left origin.
    ///
    /// The window arrives normalised to 0...1 of the oriented photo, which is
    /// exactly what the Rust layout engine's `book::crop::choose_crop`
    /// produces, so no coordinate conversion happens on the wire.
    ///
    /// Each edge is rounded to the nearest whole pixel and the rect is then
    /// `.integral` (a no-op on already-integral values, kept so the rect
    /// handed to `CGImage.cropping(to:)` is integral by construction).
    /// Rounding rather than relying on `.integral` alone is deliberate:
    /// `.integral` floors the origin and ceils the far edge, so floating
    /// point noise on an exact half turns a 360px window into 361px, and the
    /// extra row is content the layout engine chose to exclude.
    ///
    /// **Every component is validated, none is silently clamped.** An earlier
    /// version rejected a bad width but quietly clamped a negative origin into
    /// range, which would have let a wiring error on the Rust side shift the
    /// crop window instead of reporting itself. Rust already guarantees
    /// `0...1` (`book::pace` asserts it with a 1e-9 epsilon), so anything
    /// outside that is a bug worth surfacing rather than absorbing.
    private static func cropRect(for item: ExportItem, in image: CGImage) throws -> CGRect {
        let width = Double(image.width)
        let height = Double(image.height)

        // NaN fails every comparison, so this rejects it rather than letting
        // it propagate into a rect that silently "passes" a > 0 guard.
        guard item.cropW > 0, item.cropH > 0 else {
            throw ExportError.emptyCropWindow("w=\(item.cropW) h=\(item.cropH)")
        }
        guard item.cropX.isFinite, item.cropY.isFinite else {
            throw ExportError.emptyCropWindow("x=\(item.cropX) y=\(item.cropY)")
        }
        // Matched to Rust's own containment epsilon, loosened by three orders
        // of magnitude so a crop that arrives as 1.0 + 2e-16 from an f64
        // round-trip is absorbed as noise while a real out-of-range value is
        // not. Only sub-tolerance drift is clamped away below.
        let tolerance = 1e-6
        guard item.cropX >= -tolerance, item.cropY >= -tolerance,
              item.cropX + item.cropW <= 1 + tolerance,
              item.cropY + item.cropH <= 1 + tolerance else {
            throw ExportError.cropWindowOutOfRange(
                "x=\(item.cropX) y=\(item.cropY) w=\(item.cropW) h=\(item.cropH)"
            )
        }

        let x = (item.cropX.clamped(to: 0...1) * width).rounded()
        let y = (item.cropY.clamped(to: 0...1) * height).rounded()
        // An origin ON the far edge leaves no pixels to take. Checked before
        // the size is computed, because `max(1, ...)` below would otherwise
        // manufacture a 1px window out of a wholly out-of-bounds origin.
        guard x < width, y < height else {
            throw ExportError.emptyCropWindow("origin \(x),\(y) in \(image.width)x\(image.height)")
        }
        // `max(1, ...)` clamps a sub-pixel-but-positive window up to one
        // pixel: a degenerate layout, not a broken source.
        let w = max(1, min((item.cropW * width).rounded(), width - x))
        let h = max(1, min((item.cropH * height).rounded(), height - y))
        return CGRect(x: x, y: y, width: w, height: h).integral
    }

    /// Redraws the crop into an sRGB context at its own exact size.
    ///
    /// Same size in, same size out -- this converts colour, it does not
    /// resample. `CGImage.copy(colorSpace:)` would be cheaper but it
    /// REINTERPRETS the existing samples in the new space rather than
    /// converting them, which shifts every colour on a wide-gamut source.
    private static func convertToSRGB(_ image: CGImage, format: OutputFormat) throws -> CGImage {
        // JPEG has no alpha channel; keeping one would make the encoder
        // composite against an arbitrary background. PNG keeps it.
        let alpha: CGImageAlphaInfo = format == .jpeg ? .noneSkipLast : .premultipliedLast
        guard let context = CGContext(
            data: nil, width: image.width, height: image.height,
            bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: alpha.rawValue
        ) else {
            throw ExportError.colorConversionFailed
        }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        guard let converted = context.makeImage() else { throw ExportError.colorConversionFailed }
        return converted
    }

    /// `<outputDir>/<filename without extension>.<format extension>`.
    ///
    /// The filename is reduced to its last path component first, so an item
    /// can never write outside the directory the caller named. Any extension
    /// the caller supplied is replaced: the format follows the SOURCE, and a
    /// `.png` name on a JPEG file would be a lie.
    private static func outputPath(for filename: String, format: OutputFormat, in outputDir: String) -> String {
        let base = (filename as NSString).lastPathComponent
        let stem = (base as NSString).deletingPathExtension
        let safe = stem.isEmpty ? "export" : stem
        return (outputDir as NSString).appendingPathComponent("\(safe).\(format.pathExtension)")
    }

    /// Returns the byte count of the file written, read back from disk rather
    /// than estimated, so a record can never claim a size the file does not
    /// have.
    private static func encode(_ image: CGImage, to path: String, format: OutputFormat) throws -> Int {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let destination = CGImageDestinationCreateWithURL(
            url, format.utType.identifier as CFString, 1, nil
        ) else {
            throw ExportError.destinationCreationFailed
        }
        var options: [CFString: Any] = [:]
        if let quality = format.lossyQuality {
            options[kCGImageDestinationLossyCompressionQuality] = quality
        }
        CGImageDestinationAddImage(destination, image, options as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw ExportError.encodeFailed }

        let size = try FileManager.default.attributesOfItem(atPath: path)[.size] as? Int
        return size ?? 0
    }
}

private extension Double {
    func clamped(to range: ClosedRange<Double>) -> Double {
        Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
    }
}
