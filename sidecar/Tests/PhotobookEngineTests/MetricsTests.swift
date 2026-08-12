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

/// Builds an RGBA image from a per-pixel colour generator (unlike `makeImage`,
/// channels are independent rather than a single replicated grey value).
private func makeColorImage(width: Int, height: Int, _ pixel: (Int, Int) -> (r: UInt8, g: UInt8, b: UInt8)) -> CGImage {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let (r, g, b) = pixel(x, y)
            let i = (y * width + x) * 4
            bytes[i] = r; bytes[i + 1] = g; bytes[i + 2] = b; bytes[i + 3] = 255
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

/// Hamming distance between two 64-bit hashes. Task 12 implements the real
/// one in Rust; this is just enough to make the clustering-relevant property
/// testable here.
private func hamming(_ a: UInt64, _ b: UInt64) -> Int {
    (a ^ b).nonzeroBitCount
}

@Test func sharpEdgesScoreHigherThanFlatField() {
    let flat = makeImage(width: 128, height: 128) { _, _ in 128 }
    let checker = makeImage(width: 128, height: 128) { x, y in
        ((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255
    }
    #expect(Metrics.sharpness(checker) > Metrics.sharpness(flat))
}

@Test func metricsTileMaxIgnoresEmptyAreaAroundASharpSubject() {
    // Detail everywhere.
    let fullyDetailed = makeImage(width: 128, height: 128) { x, y in
        ((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255
    }
    // The same detail confined to the top-left quarter-width square — one tile
    // of the 4x4 grid — with the remaining fifteen sixteenths uniform.
    let localDetail = makeImage(width: 128, height: 128) { x, y in
        (x < 32 && y < 32) ? (((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255) : 200
    }

    let full = Metrics.sharpness(fullyDetailed)
    let local = Metrics.sharpness(localDetail)

    // Tile-max: the best tile is equally detailed in both, so these are close.
    // Whole-image variance: local would be roughly full/16.
    let message = "confined detail scored \(local) against \(full) for full-frame detail; "
        + "a ratio near 1/16 means sharpness is averaging over the whole frame "
        + "instead of taking the best tile"
    #expect(local > full * 0.5, "\(message)")
}

@Test func paletteReturnsRequestedCountAndWeightsSumToOne() {
    let img = makeImage(width: 64, height: 64) { x, _ in x < 32 ? 20 : 220 }
    let palette = Metrics.palette(img, count: 4)
    #expect(palette.count <= 4)
    #expect(palette.count >= 1)
    let total = palette.reduce(0.0) { $0 + $1.weight }
    #expect(abs(total - 1.0) < 0.001)
}

@Test func metricsPaletteReflectsActualImageComposition() {
    // Left three quarters solid blue, right quarter solid orange: a 3:1 area split.
    let blue = (r: 30.0, g: 60.0, b: 200.0)
    let orange = (r: 230.0, g: 120.0, b: 20.0)
    let img = makeColorImage(width: 128, height: 128) { x, _ in
        x < 96 ? (UInt8(blue.r), UInt8(blue.g), UInt8(blue.b)) : (UInt8(orange.r), UInt8(orange.g), UInt8(orange.b))
    }
    let palette = Metrics.palette(img, count: 4)

    #expect(palette.count <= 4)
    #expect(palette.count >= 1)
    let total = palette.reduce(0.0) { $0 + $1.weight }
    #expect(abs(total - 1.0) < 0.001)

    let tolerance = 0.1
    func isClose(_ c: PaletteColor, to target: (r: Double, g: Double, b: Double)) -> Bool {
        abs(c.r - target.r / 255.0) < tolerance
            && abs(c.g - target.g / 255.0) < tolerance
            && abs(c.b - target.b / 255.0) < tolerance
    }

    let heaviest = palette.max { $0.weight < $1.weight }!
    let heaviestDescription = "heaviest entry was r=\(heaviest.r) g=\(heaviest.g) b=\(heaviest.b) weight=\(heaviest.weight), expected close to blue \(blue)"
    #expect(isClose(heaviest, to: blue), "\(heaviestDescription)")

    let orangeEntry = palette.first { isClose($0, to: orange) }
    let orangeDescription = "no palette entry close to orange \(orange) found among \(palette)"
    #expect(orangeEntry != nil, "\(orangeDescription)")

    if let orangeEntry {
        let ratioDescription = "heaviest weight \(heaviest.weight) not meaningfully larger than orange weight \(orangeEntry.weight) for a 3:1 area split"
        #expect(heaviest.weight > orangeEntry.weight * 2, "\(ratioDescription)")
    }
}

@Test func metricsPaletteIsStableAcrossRepeatedCalls() {
    let img = makeColorImage(width: 128, height: 128) { x, _ in
        x < 96 ? (30, 60, 200) : (230, 120, 20)
    }
    let first = Metrics.palette(img, count: 4)
    let second = Metrics.palette(img, count: 4)
    #expect(first.count == second.count)
    for (a, b) in zip(first, second) {
        #expect(a.r == b.r && a.g == b.g && a.b == b.b && a.weight == b.weight)
    }
}

@Test func metricsPaletteBreaksTiesDeterministicallyByBucketKey() {
    // Three solid-colour bands: a clear majority (64/128 columns), and two
    // minority bands (32 columns each) tied exactly on pixel count, forcing
    // the tie to land right at the `count: 2` cutoff.
    //   majority (10,10,10)   -> bucket key (0,0,0)  = 0
    //   bandB    (200,10,10)  -> bucket key (3,0,0)  = 48
    //   bandC    (10,200,10)  -> bucket key (0,3,0)  = 12
    // key(bandC) < key(bandB), so the documented ascending-key tie-break
    // means bandC must win the second slot over bandB, deterministically.
    let img = makeColorImage(width: 128, height: 128) { x, _ in
        if x < 64 { return (10, 10, 10) }
        else if x < 96 { return (200, 10, 10) }
        else { return (10, 200, 10) }
    }
    let palette = Metrics.palette(img, count: 2)
    #expect(palette.count == 2)

    let majorityDescription = "expected slot 0 to be the majority grey (10,10,10), got \(palette.first as Any)"
    #expect(
        abs(palette[0].r - 10.0 / 255.0) < 0.05
            && abs(palette[0].g - 10.0 / 255.0) < 0.05
            && abs(palette[0].b - 10.0 / 255.0) < 0.05,
        "\(majorityDescription)"
    )

    let tieBreakDescription = "expected slot 1 to be bandC (10,200,10) — the smaller bucket key — under the documented tie-break, got \(palette.count > 1 ? "\(palette[1])" : "nothing")"
    #expect(
        palette.count > 1
            && abs(palette[1].r - 10.0 / 255.0) < 0.05
            && abs(palette[1].g - 200.0 / 255.0) < 0.05
            && abs(palette[1].b - 10.0 / 255.0) < 0.05,
        "\(tieBreakDescription)"
    )
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

@Test func metricsHashHammingDistanceIsSmallForNearDuplicatesAndLargeForUnrelated() {
    // Base: a structured, non-trivial pattern (like a real photo, not a flat field).
    let base = makeImage(width: 64, height: 64) { x, y in UInt8((x ^ y) & 0xFF) }
    // Near-duplicate: the same structure with a uniform brightness shift, the way
    // consecutive burst frames or slightly different exposures differ.
    let nearDuplicate = makeImage(width: 64, height: 64) { x, y in
        UInt8(min(255, Int((x ^ y) & 0xFF) + 12))
    }
    // Structurally unrelated: a coarse quadrant pattern sharing no structure with base.
    let unrelated = makeImage(width: 64, height: 64) { x, y in
        ((x < 32) != (y < 32)) ? 0 : 255
    }

    let hBase = Metrics.perceptualHash(base)
    let hNear = Metrics.perceptualHash(nearDuplicate)
    let hUnrelated = Metrics.perceptualHash(unrelated)

    let distNear = hamming(hBase, hNear)
    let distUnrelated = hamming(hBase, hUnrelated)

    let orderingDescription = "hamming(base, nearDuplicate)=\(distNear) was not < hamming(base, unrelated)=\(distUnrelated)"
    #expect(distNear < distUnrelated, "\(orderingDescription)")

    let absoluteDescription = "hamming(base, nearDuplicate)=\(distNear) of 64 bits was not small enough for a brightness-shifted near-duplicate"
    #expect(distNear < 10, "\(absoluteDescription)")
}
