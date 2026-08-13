### Task 5: EXIF extraction

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/ExifReader.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift`

**Interfaces:**
- Consumes: nothing (reads metadata only, never decodes pixels)
- Produces: `struct ExifData: Codable { captureDate: Date?; latitude: Double?; longitude: Double?; pixelWidth: Int; pixelHeight: Int; make: String?; model: String?; flashFired: Bool? }` and `ExifReader.read(path: String) throws -> ExifData`. Dimensions are **post-orientation**.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func readsDimensionsForLandscape() throws {
    let exif = try ExifReader.read(path: fixture("landscape.jpg"))
    #expect(exif.pixelWidth == 1200)
    #expect(exif.pixelHeight == 800)
}

@Test func reportsPostOrientationDimensions() throws {
    // Stored 1200x800 with orientation 6; must be reported as 800x1200.
    let exif = try ExifReader.read(path: fixture("portrait-rot90.jpg"))
    #expect(exif.pixelWidth == 800)
    #expect(exif.pixelHeight == 1200)
}

@Test func throwsOnMissingFile() {
    #expect(throws: ExifReader.ReadError.self) {
        try ExifReader.read(path: "/nonexistent/nope.jpg")
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `ExifReader` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/ExifReader.swift`:

```swift
import Foundation
import ImageIO

struct ExifData: Codable {
    var captureDate: Date?
    var latitude: Double?
    var longitude: Double?
    var pixelWidth: Int
    var pixelHeight: Int
    var make: String?
    var model: String?
    var flashFired: Bool?
}

enum ExifReader {
    enum ReadError: Error { case unreadable(String) }

    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy:MM:dd HH:mm:ss"
        f.timeZone = TimeZone(secondsFromGMT: 0)
        return f
    }()

    /// Orientations 5-8 are the four that swap width and height.
    private static func swapsAxes(_ orientation: Int) -> Bool {
        (5...8).contains(orientation)
    }

    static func read(path: String) throws -> ExifData {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        else { throw ReadError.unreadable(path) }

        let rawW = props[kCGImagePropertyPixelWidth] as? Int ?? 0
        let rawH = props[kCGImagePropertyPixelHeight] as? Int ?? 0
        let orientation = props[kCGImagePropertyOrientation] as? Int ?? 1
        let (w, h) = swapsAxes(orientation) ? (rawH, rawW) : (rawW, rawH)

        let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        let gps = props[kCGImagePropertyGPSDictionary] as? [CFString: Any]

        var lat = gps?[kCGImagePropertyGPSLatitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLatitudeRef] as? String, ref == "S", let v = lat {
            lat = -v
        }
        var lon = gps?[kCGImagePropertyGPSLongitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLongitudeRef] as? String, ref == "W", let v = lon {
            lon = -v
        }

        var flash: Bool?
        if let raw = exif?[kCGImagePropertyExifFlash] as? Int { flash = (raw & 1) == 1 }

        return ExifData(
            captureDate: (exif?[kCGImagePropertyExifDateTimeOriginal] as? String).flatMap(formatter.date(from:)),
            latitude: lat,
            longitude: lon,
            pixelWidth: w,
            pixelHeight: h,
            make: tiff?[kCGImagePropertyTIFFMake] as? String,
            model: tiff?[kCGImagePropertyTIFFModel] as? String,
            flashFired: flash
        )
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add metadata-only EXIF reader with post-orientation dimensions"
```

---

