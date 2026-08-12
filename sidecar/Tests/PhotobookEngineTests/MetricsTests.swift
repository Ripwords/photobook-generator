import Testing
import CoreGraphics
@testable import PhotobookEngine

/// Builds a greyscale-ish RGBA image from a pixel generator.
private func makeImage(width: Int, height: Int, _ pixel: (Int, Int) -> UInt8) -> CGImage {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let v = pixel(x, y)
            let i = (y * width + x) * 4
            bytes[i] = v; bytes[i + 1] = v; bytes[i + 2] = v; bytes[i + 3] = 255
        }
    }
    let ctx = CGContext(
        data: &bytes, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: width * 4,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    return ctx.makeImage()!
}

@Test func sharpEdgesScoreHigherThanFlatField() {
    let flat = makeImage(width: 128, height: 128) { _, _ in 128 }
    let checker = makeImage(width: 128, height: 128) { x, y in
        ((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255
    }
    #expect(Metrics.sharpness(checker) > Metrics.sharpness(flat))
}

@Test func sharpSubjectOnBlankBackgroundIsNotScoredBlurry() {
    // Detail confined to one tile; whole-image variance would wash this out.
    let flat = makeImage(width: 128, height: 128) { _, _ in 200 }
    let localDetail = makeImage(width: 128, height: 128) { x, y in
        (x < 32 && y < 32) ? (((x + y) % 2 == 0) ? 0 : 255) : 200
    }
    #expect(Metrics.sharpness(localDetail) > Metrics.sharpness(flat) * 5)
}

@Test func paletteReturnsRequestedCountAndWeightsSumToOne() {
    let img = makeImage(width: 64, height: 64) { x, _ in x < 32 ? 20 : 220 }
    let palette = Metrics.palette(img, count: 4)
    #expect(palette.count <= 4)
    #expect(palette.count >= 1)
    let total = palette.reduce(0.0) { $0 + $1.weight }
    #expect(abs(total - 1.0) < 0.001)
}

@Test func identicalImagesHaveIdenticalHash() {
    let a = makeImage(width: 64, height: 64) { x, y in UInt8((x ^ y) & 0xFF) }
    let b = makeImage(width: 64, height: 64) { x, y in UInt8((x ^ y) & 0xFF) }
    #expect(Metrics.perceptualHash(a) == Metrics.perceptualHash(b))
}

@Test func differentImagesHaveDifferentHash() {
    let a = makeImage(width: 64, height: 64) { x, _ in x < 32 ? 0 : 255 }
    let b = makeImage(width: 64, height: 64) { _, y in y < 32 ? 0 : 255 }
    #expect(Metrics.perceptualHash(a) != Metrics.perceptualHash(b))
}
