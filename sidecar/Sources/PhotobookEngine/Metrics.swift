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
    /// High interpolation quality so large downscales (e.g. ~1536px source to
    /// a much smaller buffer) average source pixels instead of point-sampling
    /// them, which would otherwise alias on fine detail.
    private static func rgbaBuffer(_ image: CGImage, width: Int, height: Int) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        bytes.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(
                data: raw.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return }
            ctx.interpolationQuality = .high
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
        // Sort by count descending, then by bucket key ascending. Dictionary
        // iteration order (and hence a bare `buckets.values.sorted`) varies
        // per process launch under Swift's randomised hash seeding, so a
        // count-only sort is non-deterministic across runs whenever two
        // buckets tie at the `prefix` cutoff. The layout engine downstream
        // depends on the whole pipeline being deterministic (same photos in,
        // same book out), so ties need an explicit, stable tie-break.
        return buckets
            .sorted { lhs, rhs in
                if lhs.value.n != rhs.value.n { return lhs.value.n > rhs.value.n }
                return lhs.key < rhs.key
            }
            .prefix(count)
            .map { _, e in
                PaletteColor(
                    r: e.r / Double(e.n) / 255.0,
                    g: e.g / Double(e.n) / 255.0,
                    b: e.b / Double(e.n) / 255.0,
                    weight: Double(e.n) / total
                )
            }
            .normalisedWeights()
    }

    /// Average hash over an 8x8 grid, box-averaged from a 32x32 grey
    /// downscale rather than point-sampled directly to 8x8. Point-sampling
    /// straight from a ~1536px source lets a handful of source pixels decide
    /// each bit, so a one-pixel camera-shake shift on fine detail can flip
    /// many bits; averaging a 4x4 block per output cell makes the hash
    /// robust to that. 64 bits, Hamming-comparable.
    static func perceptualHash(_ image: CGImage) -> UInt64 {
        let side = 32
        let bytes = rgbaBuffer(image, width: side, height: side)
        var grey = [Double](repeating: 0, count: side * side)
        for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }

        let gridSide = 8
        let block = side / gridSide
        var small = [Double](repeating: 0, count: gridSide * gridSide)
        for gy in 0..<gridSide {
            for gx in 0..<gridSide {
                var sum = 0.0
                for by in 0..<block {
                    for bx in 0..<block {
                        let y = gy * block + by
                        let x = gx * block + bx
                        sum += grey[y * side + x]
                    }
                }
                small[gy * gridSide + gx] = sum / Double(block * block)
            }
        }
        let mean = small.reduce(0, +) / Double(small.count)

        var hash: UInt64 = 0
        for (i, v) in small.enumerated() where v > mean {
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
