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
