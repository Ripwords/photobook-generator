### Task 4: ImageIO decode with orientation

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/ImageLoader.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift`
- Create fixtures: `sidecar/Fixtures/landscape.jpg`, `sidecar/Fixtures/portrait-rot90.jpg`

**Interfaces:**
- Consumes: nothing
- Produces: `ImageLoader.loadThumbnail(path: String, maxPixel: Int) throws -> CGImage` returning an image with EXIF orientation **already applied**, and `ImageLoader.LoadError`.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift`:

```swift
import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()   // PhotobookEngineTests
        .deletingLastPathComponent()   // Tests
        .deletingLastPathComponent()   // sidecar
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func loadsLandscapeAtRequestedSize() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 512)
    #expect(max(img.width, img.height) <= 512)
    #expect(img.width > img.height)
}

@Test func appliesExifOrientationSoPortraitIsTaller() throws {
    // File is stored landscape with EXIF orientation 6 (rotate 90 CW).
    let img = try ImageLoader.loadThumbnail(path: fixture("portrait-rot90.jpg"), maxPixel: 512)
    #expect(img.height > img.width, "orientation must be applied before we report dimensions")
}

@Test func throwsOnMissingFile() {
    #expect(throws: ImageLoader.LoadError.self) {
        try ImageLoader.loadThumbnail(path: "/nonexistent/nope.jpg", maxPixel: 512)
    }
}
```

Create the fixtures with a Swift script. **ImageMagick and exiftool are not installed on
this machine**, and requiring them would make the test suite unreproducible. ImageIO can
write both the pixels and the EXIF orientation tag, with no external dependency.

Create `scripts/make-fixtures.swift`:

```swift
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
```

Run it from the repo root:

```bash
swift scripts/make-fixtures.swift
```

`portrait-rot90.jpg` stores 1200×800 pixels but is tagged orientation 6, so a correct
reader displays it as 800×1200. That is exactly what the second test checks — and it fails
if `kCGImageSourceCreateThumbnailWithTransform` is omitted, which is the point.

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `ImageLoader` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/ImageLoader.swift`:

```swift
import Foundation
import ImageIO
import CoreGraphics

enum ImageLoader {
    enum LoadError: Error {
        case unreadable(String)
        case decodeFailed(String)
    }

    /// Decodes at most `maxPixel` on the long edge, with EXIF orientation applied.
    /// `kCGImageSourceCreateThumbnailWithTransform` reconciles EXIF orientation and
    /// HEIC container `irot`/`imir` properties, which can disagree.
    static func loadThumbnail(path: String, maxPixel: Int) throws -> CGImage {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil) else {
            throw LoadError.unreadable(path)
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            throw LoadError.decodeFailed(path)
        }
        return image
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add ImageIO thumbnail decode with orientation applied"
```

---

