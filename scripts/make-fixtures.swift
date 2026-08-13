import Foundation
import ImageIO
import CoreGraphics
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let dir = root.appendingPathComponent("sidecar/Fixtures")
try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

/// Writes a solid-colour JPEG of the given stored pixel size, tagging it with
/// `orientation` (1 = normal, 6 = rotate 90 CW on display).
func write(_ name: String, width: Int, height: Int,
           rgb: (Double, Double, Double), orientation: Int) {
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.setFillColor(red: rgb.0, green: rgb.1, blue: rgb.2, alpha: 1)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = dir.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
    let props: [CFString: Any] = [kCGImagePropertyOrientation: orientation]
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote \(name) (\(width)x\(height), orientation \(orientation))")
}

write("landscape.jpg", width: 1200, height: 800, rgb: (0.1, 0.1, 0.5), orientation: 1)
write("portrait-rot90.jpg", width: 1200, height: 800, rgb: (0.1, 0.4, 0.1), orientation: 6)

/// Writes a solid-colour JPEG carrying a GPS block (South/West hemisphere) and an
/// EXIF DateTimeOriginal, so ExifReaderTests can exercise hemisphere-sign negation
/// and date parsing end-to-end (both are otherwise untested by the plain fixtures above).
func writeWithGpsAndDate(_ name: String, width: Int, height: Int, rgb: (Double, Double, Double)) {
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.setFillColor(red: rgb.0, green: rgb.1, blue: rgb.2, alpha: 1)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = dir.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!

    let gps: [CFString: Any] = [
        kCGImagePropertyGPSLatitude: 33.8688,
        kCGImagePropertyGPSLatitudeRef: "S",
        kCGImagePropertyGPSLongitude: 151.2093,
        kCGImagePropertyGPSLongitudeRef: "W",
    ]
    let exif: [CFString: Any] = [
        kCGImagePropertyExifDateTimeOriginal: "2024:06:15 10:30:00",
    ]
    let props: [CFString: Any] = [
        kCGImagePropertyOrientation: 1,
        kCGImagePropertyGPSDictionary: gps,
        kCGImagePropertyExifDictionary: exif,
    ]
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote \(name) (\(width)x\(height), GPS S/W + DateTimeOriginal)")
}

writeWithGpsAndDate("gps-south-west.jpg", width: 1200, height: 800, rgb: (0.5, 0.1, 0.1))

// MARK: - Hostile fixtures (Task 10)
//
// These three need real image data (a colour space / decode path ImageIO
// plausibly mishandles), so they're generated here rather than by pure byte
// manipulation. The other four hostile fixtures (empty/truncated/text/random)
// are pure bytes and live in scripts/make-hostile-fixtures.sh instead.
//
// NOTE: this file is append-only in effect -- `landscape.jpg`,
// `portrait-rot90.jpg`, and `gps-south-west.jpg` above are committed and
// depended on byte-for-byte by earlier tests. If you edit anything below
// this line, rerun `swift scripts/make-fixtures.swift` from the repo root
// and re-commit the regenerated files in sidecar/Fixtures/hostile/ -- nothing
// checks automatically that the committed bytes still match this script.

let hostile = dir.appendingPathComponent("hostile")
try? FileManager.default.createDirectory(at: hostile, withIntermediateDirectories: true)

/// Writes a solid-colour image into the hostile directory, optionally in a
/// colour space or with a filename that downstream code may not expect.
func writeHostile(_ name: String, width: Int, height: Int, cmyk: Bool) {
    let space = cmyk ? CGColorSpaceCreateDeviceCMYK() : CGColorSpaceCreateDeviceRGB()
    let info = cmyk
        ? CGImageAlphaInfo.none.rawValue
        : CGImageAlphaInfo.premultipliedLast.rawValue
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: space, bitmapInfo: info
    )!
    ctx.setFillColor(CGColor(colorSpace: space,
                             components: cmyk ? [0, 1, 1, 0, 1] : [1, 0, 0, 1])!)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = hostile.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote hostile/\(name)")
}

writeHostile("one-pixel.jpg", width: 1, height: 1, cmyk: false)
writeHostile("cmyk.jpg", width: 64, height: 64, cmyk: true)
writeHostile("no-extension", width: 64, height: 64, cmyk: false)

// MARK: - Two-pass thumbnail fixtures (RAW/HEIC decode performance fix)
//
// Real camera JPEGs and HEICs embed a small preview/thumbnail alongside the
// full image -- for an out-of-camera JPEG that's typically the classic
// ~160x120 EXIF thumbnail. ImageLoader's size-floor check must detect when
// that embedded preview is too small and fall back to a full decode. These
// fixtures reproduce that shape with `kCGImageDestinationEmbedThumbnail`,
// which makes ImageIO generate a real embedded thumbnail distinct from (and
// much smaller than) the main image -- confirmed by probing real iPhone
// JPEGs during this fix: `kCGImageSourceCreateThumbnailFromImageIfAbsent`
// returns that embedded ~160px thumbnail, not a downscale of the real image.
func writeWithEmbeddedThumbnail(_ name: String, width: Int, height: Int,
                                 rgb: (Double, Double, Double), orientation: Int) {
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.setFillColor(red: rgb.0, green: rgb.1, blue: rgb.2, alpha: 1)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = dir.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
    let props: [CFString: Any] = [
        kCGImagePropertyOrientation: orientation,
        kCGImageDestinationEmbedThumbnail: true,
        kCGImageDestinationLossyCompressionQuality: 0.85,
    ]
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote \(name) (\(width)x\(height), orientation \(orientation), embedded thumbnail)")
}

// Main image (2000x1333) is well above `analysisMaxPixel` (1536) on its long
// edge, but the embedded thumbnail ImageIO generates is ~160px -- below any
// sane floor. Used to prove the floor check actually fires the fallback.
writeWithEmbeddedThumbnail("postage-stamp-source.jpg", width: 2000, height: 1333,
                           rgb: (0.2, 0.3, 0.6), orientation: 1)

// Same shape, but stored landscape with EXIF orientation 6 (rotate 90 CW),
// so it can prove orientation is applied correctly on BOTH the accepted-
// preview path (low floor, no fallback) and the fallback-to-full-decode path
// (default floor) -- not just one of them.
writeWithEmbeddedThumbnail("portrait-rot90-tiny-thumb.jpg", width: 2000, height: 1333,
                           rgb: (0.4, 0.15, 0.15), orientation: 6)

// No embedded thumbnail at all (unlike the two above), and large enough
// (long edge 2000) that decoding it via
// kCGImageSourceCreateThumbnailFromImageIfAbsent alone -- with no smaller
// embedded thumbnail to fall back from -- already meets the default
// analysisMaxPixel floor (1536) on the first pass. `landscape.jpg` above is
// too small (1200x800) to exercise this: its native size is already below
// the 1536 floor, so it always reports a "fallback" even though there is
// nothing smaller to have fallen back from. This fixture is what
// BenchmarkerTests uses to prove the floor is met WITHOUT a wasted second
// decode when the source is genuinely large.
write("large-no-thumbnail.jpg", width: 2000, height: 1333, rgb: (0.15, 0.35, 0.25), orientation: 1)
