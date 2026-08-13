### Task 10: Hostile-input fixture corpus

**Files:**
- Create: `sidecar/Fixtures/hostile/` (7 files), `sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift`
- Create: `scripts/make-hostile-fixtures.sh`

**Interfaces:**
- Consumes: `Analyzer.analyze` from Task 9
- Produces: no new API. This task's deliverable is proof that malformed input degrades to `.failed` records rather than crashing the process.

This is the task that makes the crash-isolation argument real. If any of these crash the test runner, that is the bug the whole sidecar architecture exists to contain.

- [ ] **Step 1: Write the failing test**

`scripts/make-hostile-fixtures.sh`:

ImageMagick is not installed, so the three fixtures that need real image data are produced
by extending `scripts/make-fixtures.swift` from Task 4 rather than shelling out.

Add to `scripts/make-fixtures.swift`, after the two existing `write` calls:

```swift
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
```

Then `scripts/make-hostile-fixtures.sh` covers only the four that are pure byte
manipulation and need no image library:

```bash
#!/usr/bin/env bash
set -euo pipefail
DIR="sidecar/Fixtures/hostile"
mkdir -p "$DIR"

: > "$DIR/empty.jpg"                                    # zero bytes
head -c 400 sidecar/Fixtures/landscape.jpg > "$DIR/truncated.jpg"
printf 'not an image at all, just text' > "$DIR/text.jpg"
head -c 2000 /dev/urandom > "$DIR/random.jpg"
echo "created byte-level hostile fixtures in $DIR"
echo "run 'swift scripts/make-fixtures.swift' for the image-based ones"
```

If `CGColorSpaceCreateDeviceCMYK` refuses the JPEG destination on this macOS version,
substitute any other awkward-but-writable colour space and note the substitution — the
point of the fixture is that the analyser meets a colour space it did not expect, not CMYK
specifically.

`sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func hostileDir() -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/hostile")
}

@Test func everyHostileFixtureProducesARecordAndNeverCrashes() throws {
    let files = try FileManager.default
        .contentsOfDirectory(at: hostileDir(), includingPropertiesForKeys: nil)
        .map(\.path)
        .sorted()

    #expect(files.count >= 7, "run scripts/make-hostile-fixtures.sh first")

    let records = Analyzer.analyze(paths: files)
    #expect(records.count == files.count)
    // Reaching this line at all is the assertion: nothing trapped.
}

@Test func zeroByteFileFailsCleanly() {
    let path = hostileDir().appendingPathComponent("empty.jpg").path
    let records = Analyzer.analyze(paths: [path])
    guard case .failed = records[0] else {
        Issue.record("zero-byte file must produce a failed record"); return
    }
}

@Test func truncatedJpegFailsCleanlyOrDecodesPartially() {
    let path = hostileDir().appendingPathComponent("truncated.jpg").path
    let records = Analyzer.analyze(paths: [path])
    #expect(records.count == 1)   // either outcome is acceptable; a crash is not
}

@Test func hostileBatchMixedWithGoodPhotoStillReturnsTheGoodOne() throws {
    let good = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/landscape.jpg").path
    let bad = hostileDir().appendingPathComponent("random.jpg").path

    let records = Analyzer.analyze(paths: [bad, good, bad])
    #expect(records.count == 3)
    guard case .ok = records[1] else {
        Issue.record("the good photo must survive a hostile batch"); return
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — `files.count >= 7` fails because the fixtures don't exist yet

- [ ] **Step 3: Generate the fixtures**

```bash
chmod +x scripts/make-hostile-fixtures.sh
bash scripts/make-hostile-fixtures.sh
```

If any test now *crashes* rather than fails, that is a real bug in `Analyzer.analyzeOne` — the fix is to add the missing `guard`/`throws` path in `ImageLoader` or `ExifReader`, not to remove the fixture.

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 4 new tests, process exits 0

- [ ] **Step 5: Commit**

```bash
git add sidecar scripts
git commit -m "test(sidecar): add hostile input corpus proving crash isolation"
```

---

