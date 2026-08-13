### Task 6: Classical metrics — sharpness, palette, pHash

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/Metrics.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`

**Interfaces:**
- Consumes: `CGImage` from `ImageLoader`
- Produces:
  - `Metrics.sharpness(_ image: CGImage) -> Double` — contrast-normalised tile-max Laplacian variance over a 4×4 grid
  - `Metrics.palette(_ image: CGImage, count: Int) -> [PaletteColor]` where `struct PaletteColor: Codable { r, g, b: Double; weight: Double }`
  - `Metrics.perceptualHash(_ image: CGImage) -> UInt64` — 8×8 DCT-free average hash over a 32×32 grey downscale
  - `Metrics.warmth(_ palette: [PaletteColor]) -> Double` and `Metrics.contrast(_ image: CGImage) -> Double`

Sharpness is **tile-max**, not whole-image: a sharp photo of a blank wall scores as blurry under plain Laplacian variance. Taking the max over tiles answers the better question — is the *subject* in focus.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`:

```swift
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `Metrics` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/Metrics.swift`:

```swift
import Foundation
import CoreGraphics

struct PaletteColor: Codable {
    let r: Double
    let g: Double
    let b: Double
    let weight: Double
}

enum Metrics {
    /// Draws `image` into a tightly-packed RGBA8 buffer of the given size.
    private static func rgbaBuffer(_ image: CGImage, width: Int, height: Int) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        bytes.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(
                data: raw.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return }
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
        return bytes
    }

    private static func luma(_ bytes: [UInt8], _ i: Int) -> Double {
        0.2126 * Double(bytes[i]) + 0.7152 * Double(bytes[i + 1]) + 0.0722 * Double(bytes[i + 2])
    }

    /// Contrast-normalised tile-max Laplacian variance over a 4x4 grid.
    static func sharpness(_ image: CGImage) -> Double {
        let side = 256
        let bytes = rgbaBuffer(image, width: side, height: side)
        var grey = [Double](repeating: 0, count: side * side)
        for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }

        let tiles = 4
        let tile = side / tiles
        var best = 0.0

        for ty in 0..<tiles {
            for tx in 0..<tiles {
                var values: [Double] = []
                values.reserveCapacity(tile * tile)
                for y in (ty * tile + 1)..<((ty + 1) * tile - 1) {
                    for x in (tx * tile + 1)..<((tx + 1) * tile - 1) {
                        let c = grey[y * side + x]
                        let lap = grey[(y - 1) * side + x] + grey[(y + 1) * side + x]
                                + grey[y * side + x - 1] + grey[y * side + x + 1] - 4 * c
                        values.append(lap)
                    }
                }
                guard values.count > 1 else { continue }
                let mean = values.reduce(0, +) / Double(values.count)
                let variance = values.reduce(0) { $0 + ($1 - mean) * ($1 - mean) } / Double(values.count)

                // Normalise by local contrast so dark tiles aren't unfairly penalised.
                var lo = 255.0, hi = 0.0
                for y in (ty * tile)..<((ty + 1) * tile) {
                    for x in (tx * tile)..<((tx + 1) * tile) {
                        let v = grey[y * side + x]
                        lo = min(lo, v); hi = max(hi, v)
                    }
                }
                let contrast = max(hi - lo, 1.0)
                best = max(best, variance / contrast)
            }
        }
        return best
    }

    /// Uniform 4x4x4 RGB bucket histogram, returning the heaviest buckets.
    static func palette(_ image: CGImage, count: Int) -> [PaletteColor] {
        let side = 128
        let bytes = rgbaBuffer(image, width: side, height: side)
        var buckets = [Int: (r: Double, g: Double, b: Double, n: Int)]()

        for p in 0..<(side * side) {
            let i = p * 4
            let r = Double(bytes[i]), g = Double(bytes[i + 1]), b = Double(bytes[i + 2])
            let key = (Int(r) >> 6) << 4 | (Int(g) >> 6) << 2 | (Int(b) >> 6)
            var e = buckets[key] ?? (0, 0, 0, 0)
            e.r += r; e.g += g; e.b += b; e.n += 1
            buckets[key] = e
        }

        let total = Double(side * side)
        return buckets.values
            .sorted { $0.n > $1.n }
            .prefix(count)
            .map { e in
                PaletteColor(
                    r: e.r / Double(e.n) / 255.0,
                    g: e.g / Double(e.n) / 255.0,
                    b: e.b / Double(e.n) / 255.0,
                    weight: Double(e.n) / total
                )
            }
            .normalisedWeights()
    }

    /// Average hash over a 8x8 grey downscale. 64 bits, Hamming-comparable.
    static func perceptualHash(_ image: CGImage) -> UInt64 {
        let side = 8
        let bytes = rgbaBuffer(image, width: side, height: side)
        var grey = [Double](repeating: 0, count: side * side)
        for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }
        let mean = grey.reduce(0, +) / Double(grey.count)

        var hash: UInt64 = 0
        for (i, v) in grey.enumerated() where v > mean {
            hash |= (1 << UInt64(i))
        }
        return hash
    }

    /// Warm colours (red-dominant) score toward 1, cool (blue-dominant) toward 0.
    static func warmth(_ palette: [PaletteColor]) -> Double {
        guard !palette.isEmpty else { return 0.5 }
        return palette.reduce(0.0) { acc, c in
            acc + c.weight * (0.5 + (c.r - c.b) / 2.0)
        }.clamped(0, 1)
    }

    /// Global luma spread, normalised to 0...1.
    static func contrast(_ image: CGImage) -> Double {
        let side = 128
        let bytes = rgbaBuffer(image, width: side, height: side)
        var lo = 255.0, hi = 0.0
        for p in 0..<(side * side) {
            let v = luma(bytes, p * 4)
            lo = min(lo, v); hi = max(hi, v)
        }
        return ((hi - lo) / 255.0).clamped(0, 1)
    }
}

private extension Array where Element == PaletteColor {
    func normalisedWeights() -> [PaletteColor] {
        let total = reduce(0.0) { $0 + $1.weight }
        guard total > 0 else { return self }
        return map { PaletteColor(r: $0.r, g: $0.g, b: $0.b, weight: $0.weight / total) }
    }
}

extension Double {
    func clamped(_ lo: Double, _ hi: Double) -> Double { Swift.min(Swift.max(self, lo), hi) }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 5 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add sharpness, palette, and perceptual hash metrics"
```

---

