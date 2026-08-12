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

@Test func metricsHashHammingDistanceModelsRealisticBurstVariation() {
    // Real near-duplicates (consecutive burst frames) differ by small camera
    // shake, a slight exposure change, and minor subject movement, over an
    // image with structure at many scales — not a single pathological
    // spatial frequency (see the doc comment on Metrics.perceptualHash for
    // why a single-frequency near-Nyquist checkerboard is the wrong model).
    // Build a scene with a smooth diagonal luminance gradient, four soft
    // gaussian "blobs" (~120-200px radius i.e. ~200-400px across at 1536px
    // scale), and a low-amplitude fine texture layer.
    let width = 1536, height = 1536
    let w = Double(width - 1), h = Double(height - 1)

    func scene(_ x: Int, _ y: Int, gradientReversed: Bool, blobs: [(cx: Double, cy: Double, sigma: Double, amp: Double)]) -> UInt8 {
        let fx = Double(x), fy = Double(y)
        let t = gradientReversed ? ((w - fx) + fy) / (w + h) : (fx + fy) / (w + h)
        var v = 50.0 + t * 140.0 // smooth diagonal gradient, ~50...190
        for blob in blobs {
            let dx = fx - blob.cx, dy = fy - blob.cy
            v += blob.amp * exp(-(dx * dx + dy * dy) / (2 * blob.sigma * blob.sigma)) // soft falloff, no hard edges
        }
        v += 10.0 * sin(fx * 0.5) * cos(fy * 0.5) // low-amplitude fine texture
        return UInt8(v.clamped(0, 255))
    }

    let baseBlobs: [(cx: Double, cy: Double, sigma: Double, amp: Double)] = [
        (cx: 0.25 * w, cy: 0.30 * h, sigma: 90, amp: 45),
        (cx: 0.70 * w, cy: 0.25 * h, sigma: 70, amp: 40),
        (cx: 0.40 * w, cy: 0.75 * h, sigma: 100, amp: 35),
        (cx: 0.80 * w, cy: 0.80 * h, sigma: 60, amp: 30),
    ]
    // Structurally unrelated: reversed gradient direction, blobs relocated.
    let unrelatedBlobs: [(cx: Double, cy: Double, sigma: Double, amp: Double)] = [
        (cx: 0.75 * w, cy: 0.70 * h, sigma: 80, amp: 45),
        (cx: 0.30 * w, cy: 0.80 * h, sigma: 65, amp: 40),
        (cx: 0.60 * w, cy: 0.20 * h, sigma: 95, amp: 35),
        (cx: 0.15 * w, cy: 0.15 * h, sigma: 55, amp: 30),
    ]

    let shiftPixels = width / 100 // ~1% of width: the camera-shake case

    let base = makeImage(width: width, height: height) { x, y in
        scene(x, y, gradientReversed: false, blobs: baseBlobs)
    }
    let shifted = makeImage(width: width, height: height) { x, y in
        scene(x - shiftPixels, y, gradientReversed: false, blobs: baseBlobs)
    }
    let exposed = makeImage(width: width, height: height) { x, y in
        let v = scene(x, y, gradientReversed: false, blobs: baseBlobs)
        return UInt8((Double(v) * 1.1).clamped(0, 255)) // ~10% brighter, clamped
    }
    let unrelated = makeImage(width: width, height: height) { x, y in
        scene(x, y, gradientReversed: true, blobs: unrelatedBlobs)
    }

    let hBase = Metrics.perceptualHash(base)
    let hShift = Metrics.perceptualHash(shifted)
    let hExposed = Metrics.perceptualHash(exposed)
    let hUnrelated = Metrics.perceptualHash(unrelated)

    let distShift = hamming(hBase, hShift)
    let distExposed = hamming(hBase, hExposed)
    let distUnrelated = hamming(hBase, hUnrelated)

    // Bound set just above what was actually measured on this scene
    // (distShift=0, distExposed=0, distUnrelated=30 of 64) — see task-6-report.md
    // for the measurement this was calibrated against.
    let shiftDescription = "hamming(base, shifted)=\(distShift) of 64 bits was not small for a ~1%-width camera-shake shift"
    #expect(distShift < 5, "\(shiftDescription)")

    let exposedDescription = "hamming(base, exposed)=\(distExposed) of 64 bits was not small for a ~10% exposure change"
    #expect(distExposed < 5, "\(exposedDescription)")

    let shiftOrderingDescription = "hamming(base, shifted)=\(distShift) was not clearly smaller than hamming(base, unrelated)=\(distUnrelated)"
    #expect(distShift < distUnrelated, "\(shiftOrderingDescription)")

    let exposedOrderingDescription = "hamming(base, exposed)=\(distExposed) was not clearly smaller than hamming(base, unrelated)=\(distUnrelated)"
    #expect(distExposed < distUnrelated, "\(exposedOrderingDescription)")
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
