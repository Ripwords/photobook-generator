### Task 9: Wire the analyze request end-to-end in Swift

**Files:**
- Modify: `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/main.swift`
- Create: `sidecar/Sources/PhotobookEngine/Analyzer.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`

**Interfaces:**
- Consumes: `ImageLoader`, `ExifReader`, `Metrics`, `VisionAnalyzer`, `SmileProxy`
- Produces: `struct PhotoFeatures: Codable` (the full per-photo record) and `Analyzer.analyze(paths: [String]) -> [PhotoRecord]` where `enum PhotoRecord: Codable` is either `.ok(PhotoFeatures)` or `.failed(path: String, message: String)`. Adds `ResponseResult.analyzed([PhotoRecord])`.

Analysis runs under `DispatchQueue.concurrentPerform`. A failure on one photo produces a `.failed` record — **never** a crash and never an aborted batch.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func analysesAGoodPhoto() {
    let records = Analyzer.analyze(paths: [fixture("landscape.jpg")])
    #expect(records.count == 1)
    guard case .ok(let f) = records[0] else {
        Issue.record("expected ok record"); return
    }
    #expect(f.width == 1200)
    #expect(f.height == 800)
    #expect(!f.hash.isEmpty)
    #expect(f.phash != 0)
}

@Test func reportsFailureForMissingFileWithoutCrashing() {
    let records = Analyzer.analyze(paths: ["/nonexistent/nope.jpg"])
    #expect(records.count == 1)
    guard case .failed(let path, _) = records[0] else {
        Issue.record("expected failed record"); return
    }
    #expect(path == "/nonexistent/nope.jpg")
}

@Test func oneBadPhotoDoesNotAbortTheBatch() {
    let records = Analyzer.analyze(paths: [
        "/nonexistent/nope.jpg",
        fixture("landscape.jpg"),
    ])
    #expect(records.count == 2)
    // Order must be preserved so callers can correlate with their input.
    guard case .failed = records[0] else { Issue.record("expected failure first"); return }
    guard case .ok = records[1] else { Issue.record("expected success second"); return }
}

@Test func preservesInputOrderUnderConcurrency() {
    let paths = (0..<8).map { _ in fixture("landscape.jpg") } + ["/nonexistent/a.jpg"]
    let records = Analyzer.analyze(paths: paths)
    #expect(records.count == 9)
    guard case .failed = records[8] else { Issue.record("last must be the failure"); return }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `Analyzer` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/Analyzer.swift`:

```swift
import Foundation
import CryptoKit

struct PhotoFeatures: Codable {
    let path: String
    let hash: String
    let width: Int
    let height: Int
    let exif: ExifData
    let isUtility: Bool
    let aestheticScore: Double
    let sharpness: Double
    let faces: [FaceObservation]
    let faceAreaFraction: Double
    let smileFraction: Double?
    let saliencyBox: [Double]?
    let horizonTiltDeg: Double?
    let sceneTags: [String]
    let hasText: Bool
    let palette: [PaletteColor]
    let warmth: Double
    let contrast: Double
    let phash: UInt64
}

enum PhotoRecord: Codable {
    case ok(PhotoFeatures)
    case failed(path: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, features, path, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let f):
            try c.encode("ok", forKey: .status)
            try c.encode(f, forKey: .features)
        case .failed(let path, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(path, forKey: .path)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(try c.decode(PhotoFeatures.self, forKey: .features))
        } else {
            self = .failed(path: try c.decode(String.self, forKey: .path),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

enum Analyzer {
    static let analysisMaxPixel = 1536

    private static func contentHash(path: String) throws -> String {
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    static func analyzeOne(path: String) throws -> PhotoFeatures {
        let exif = try ExifReader.read(path: path)
        let hash = try contentHash(path: path)
        let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)

        // autoreleasepool matters: CGImage allocations otherwise accumulate
        // across a large concurrent batch.
        return autoreleasepool {
            let vision = VisionAnalyzer.analyze(image)
            let palette = Metrics.palette(image, count: 6)
            let faceArea = vision.faces.reduce(0.0) { $0 + $1.box[2] * $1.box[3] }

            return PhotoFeatures(
                path: path,
                hash: hash,
                width: exif.pixelWidth,
                height: exif.pixelHeight,
                exif: exif,
                isUtility: vision.isUtility,
                aestheticScore: vision.aestheticScore,
                sharpness: Metrics.sharpness(image),
                faces: vision.faces,
                faceAreaFraction: min(faceArea, 1.0),
                smileFraction: SmileProxy.fraction(faces: vision.faces, threshold: 0.5),
                saliencyBox: vision.saliencyBox,
                horizonTiltDeg: vision.horizonTiltDeg,
                sceneTags: vision.sceneTags,
                hasText: vision.hasText,
                palette: palette,
                warmth: Metrics.warmth(palette),
                contrast: Metrics.contrast(image),
                phash: Metrics.perceptualHash(image)
            )
        }
    }

    static func analyze(paths: [String]) -> [PhotoRecord] {
        var results = [PhotoRecord?](repeating: nil, count: paths.count)
        let lock = NSLock()

        DispatchQueue.concurrentPerform(iterations: paths.count) { index in
            let path = paths[index]
            let record: PhotoRecord
            do {
                record = .ok(try analyzeOne(path: path))
            } catch {
                record = .failed(path: path, message: String(describing: error))
            }
            lock.lock()
            results[index] = record
            lock.unlock()
        }

        return results.compactMap { $0 }
    }
}
```

Add to `Protocol.swift`'s `ResponseResult`:

```swift
    case analyzed([PhotoRecord])
```

with encode/decode arms:

```swift
        case .analyzed(let v):
            try c.encode("analyzed", forKey: .type)
            try c.encode(v, forKey: .data)
```

```swift
        case "analyzed": self = .analyzed(try c.decode([PhotoRecord].self, forKey: .data))
```

Replace the `.analyze` arm in `main.swift`:

```swift
    case .analyze:
        let paths = request.paths ?? []
        emit(Response(id: request.id, result: .analyzed(Analyzer.analyze(paths: paths))))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 4 new tests

Verify by hand:

```bash
swift build --package-path sidecar -c release
echo '{"id":"1","kind":"analyze","paths":["sidecar/Fixtures/landscape.jpg"]}' \
  | ./sidecar/.build/release/PhotobookEngine
```

Expected: one line of JSON containing `"status":"ok"` and `"width":1200`.

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add concurrent analyze pipeline with per-photo failure records"
```

---

