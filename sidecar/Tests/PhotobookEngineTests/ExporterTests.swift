import Testing
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
@testable import PhotobookEngine

// Every `@Test func` in this target shares ONE module namespace, so every
// name here is prefixed `exporter`. A generic name is a compile error, not a
// test failure.

private func exporterFixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()   // PhotobookEngineTests
        .deletingLastPathComponent()   // Tests
        .deletingLastPathComponent()   // sidecar
        .appendingPathComponent("Fixtures/\(name)").path
}

/// A fresh, not-yet-created output directory. Export must create it.
private func exporterTempDir(_ label: String) -> String {
    (NSTemporaryDirectory() as NSString)
        .appendingPathComponent("photobook-export-tests/\(label)-\(UUID().uuidString)")
}

private func exporterItem(
    _ source: String, _ filename: String,
    _ x: Double, _ y: Double, _ w: Double, _ h: Double
) -> ExportItem {
    ExportItem(sourcePath: source, filename: filename, cropX: x, cropY: y, cropW: w, cropH: h)
}

/// Reads one pixel out of a written file in DISPLAY coordinates (top-left
/// origin), converted into sRGB so the comparison is against a known space.
///
/// This is the assertion that dimensions cannot make: a crop of the wrong
/// region can have exactly the right size, so proving the *right* region was
/// taken requires looking at the content.
private func exporterSamplePixel(_ path: String, x: Int, y: Int) -> (r: Int, g: Int, b: Int)? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return nil }
    var px = [UInt8](repeating: 0, count: 4)
    guard let ctx = CGContext(
        data: &px, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
    ) else { return nil }
    // Translate so display pixel (x, y) lands in the 1x1 context. CGContext's
    // origin is bottom-left, hence the height flip on y.
    ctx.draw(image, in: CGRect(
        x: -CGFloat(x), y: -CGFloat(image.height - 1 - y),
        width: CGFloat(image.width), height: CGFloat(image.height)
    ))
    return (Int(px[0]), Int(px[1]), Int(px[2]))
}

/// JPEG at q0.95 reproduces a flat colour block to within a couple of levels;
/// measured on these fixtures the worst channel error is 1. 30 leaves room
/// without letting a genuinely different quadrant through -- the four
/// quadrant colours are 255 apart on at least one channel.
private func exporterExpectColor(
    _ path: String, x: Int, y: Int,
    _ expected: (r: Int, g: Int, b: Int), _ label: String,
    sourceLocation: SourceLocation = #_sourceLocation
) {
    guard let got = exporterSamplePixel(path, x: x, y: y) else {
        Issue.record("could not sample \(path)", sourceLocation: sourceLocation)
        return
    }
    let close = abs(got.r - expected.r) <= 30 && abs(got.g - expected.g) <= 30 && abs(got.b - expected.b) <= 30
    #expect(close, "\(label): pixel (\(x),\(y)) was \(got), expected \(expected)", sourceLocation: sourceLocation)
}

private let exporterRed = (r: 255, g: 0, b: 0)
private let exporterGreen = (r: 0, g: 255, b: 0)
private let exporterBlue = (r: 0, g: 0, b: 255)
private let exporterYellow = (r: 255, g: 255, b: 0)

private func exporterOnlyOk(_ records: [ExportRecord], _ label: String,
                            sourceLocation: SourceLocation = #_sourceLocation)
    -> (path: String, width: Int, height: Int, bytes: Int)? {
    guard records.count == 1 else {
        Issue.record("\(label): expected 1 record, got \(records.count)", sourceLocation: sourceLocation)
        return nil
    }
    guard case .ok(let path, let width, let height, let bytes) = records[0] else {
        Issue.record("\(label): expected .ok, got \(records[0])", sourceLocation: sourceLocation)
        return nil
    }
    return (path, width, height, bytes)
}

private func exporterProfileName(_ path: String) -> String? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
    else { return nil }
    return props[kCGImagePropertyProfileName] as? String
}

private func exporterContainerType(_ path: String) -> String? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let type = CGImageSourceGetType(source) else { return nil }
    return type as String
}

// MARK: - Crop region: coordinate space and origin

/// Guards: "the crop window is applied in the wrong coordinate space, or with
/// a flipped origin."
///
/// All four quadrants are exported and sampled. One quadrant alone cannot
/// distinguish a y-flip from an x-flip from a transpose; four can, because
/// each mis-mapping sends at least one quadrant to a different colour.
@Test func exporterCropsTheRegionNamedInTopLeftNormalisedCoordinates() throws {
    let dir = exporterTempDir("quadrants")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let src = exporterFixture("export-quadrants.jpg")

    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(src, "tl", 0.0, 0.0, 0.5, 0.5),
        exporterItem(src, "tr", 0.5, 0.0, 0.5, 0.5),
        exporterItem(src, "bl", 0.0, 0.5, 0.5, 0.5),
        exporterItem(src, "br", 0.5, 0.5, 0.5, 0.5),
    ]))

    #expect(records.count == 4)
    let expected: [(String, (r: Int, g: Int, b: Int))] = [
        ("top-left", exporterRed), ("top-right", exporterGreen),
        ("bottom-left", exporterBlue), ("bottom-right", exporterYellow),
    ]
    for (index, record) in records.enumerated() {
        guard case .ok(let path, let width, let height, _) = record else {
            Issue.record("record \(index) was not .ok: \(record)")
            continue
        }
        #expect(width == 600 && height == 400,
                "\(expected[index].0): quarter of a 1200x800 source is 600x400, got \(width)x\(height)")
        // Sample the middle of the exported quarter, far from any JPEG
        // block boundary with a neighbouring quadrant.
        exporterExpectColor(path, x: 300, y: 200, expected[index].1, expected[index].0)
    }
}

/// A crop that is not centred and not symmetric on either axis: an x/y
/// transposition or an origin flip moves it into a different quadrant.
@Test func exporterCropOffCentreLandsInTheNamedQuadrant() throws {
    let dir = exporterTempDir("offcentre")
    defer { try? FileManager.default.removeItem(atPath: dir) }

    // x 0.60-0.85, y 0.10-0.30 -> firmly inside the top-right (green)
    // quadrant. Transposed it would read x 0.10-0.30 / y 0.60-0.85, which is
    // bottom-left (blue). Y-flipped it would be bottom-right (yellow).
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "off", 0.60, 0.10, 0.25, 0.20),
    ]))
    guard let ok = exporterOnlyOk(records, "off-centre crop") else { return }
    #expect(ok.width == 300 && ok.height == 160, "got \(ok.width)x\(ok.height)")
    exporterExpectColor(ok.path, x: 150, y: 80, exporterGreen, "off-centre crop")
}

// MARK: - EXIF orientation must be applied BEFORE cropping

/// Guards: "a portrait photo (EXIF orientation 5-8) has the wrong region
/// cropped because orientation was applied after cropping instead of before."
///
/// `export-quadrants-rot90.jpg` stores 1200x800 landscape pixels tagged EXIF
/// orientation 6 (rotate 90 CW on display), so DISPLAYED it is 800x1200 and
/// the quadrants rotate with it:
///
///   displayed top-left = stored bottom-left = BLUE
///   displayed top-right = stored top-left   = RED
///
/// Orient-then-crop of the displayed top-left half gives 400x600 of BLUE.
/// Crop-then-orient gives the stored top-left, 600x400 (rotated to 400x600)
/// of RED. Colour is what separates them when the dimensions happen to agree.
@Test func exporterAppliesExifOrientationBeforeCropping() throws {
    let dir = exporterTempDir("orientation")
    defer { try? FileManager.default.removeItem(atPath: dir) }

    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants-rot90.jpg"), "tl", 0.0, 0.0, 0.5, 0.5),
        exporterItem(exporterFixture("export-quadrants-rot90.jpg"), "tr", 0.5, 0.0, 0.5, 0.5),
    ]))
    #expect(records.count == 2)

    guard case .ok(let tlPath, let tlW, let tlH, _) = records[0] else {
        Issue.record("orientation top-left record was not .ok: \(records[0])")
        return
    }
    #expect(tlW == 400 && tlH == 600,
            "the oriented source is 800x1200, so a quarter is 400x600; got \(tlW)x\(tlH)")
    exporterExpectColor(tlPath, x: 200, y: 300, exporterBlue,
                        "displayed top-left of an orientation-6 source is the STORED bottom-left")

    guard case .ok(let trPath, _, _, _) = records[1] else {
        Issue.record("orientation top-right record was not .ok: \(records[1])")
        return
    }
    exporterExpectColor(trPath, x: 200, y: 300, exporterRed,
                        "displayed top-right of an orientation-6 source is the STORED top-left")
}

/// The fixture must genuinely carry an EXIF orientation tag rather than
/// pre-rotated pixels -- otherwise the test above would pass under an
/// implementation that ignores orientation entirely.
@Test func exporterOrientationFixtureReallyCarriesAnExifOrientationTag() throws {
    let url = URL(fileURLWithPath: exporterFixture("export-quadrants-rot90.jpg")) as CFURL
    let source = try #require(CGImageSourceCreateWithURL(url, nil))
    let props = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
    #expect(props[kCGImagePropertyOrientation] as? Int == 6,
            "fixture must be tagged orientation 6, got \(String(describing: props[kCGImagePropertyOrientation]))")
    #expect(props[kCGImagePropertyPixelWidth] as? Int == 1200)
    #expect(props[kCGImagePropertyPixelHeight] as? Int == 800,
            "stored pixels must still be landscape -- if they were pre-rotated the orientation test proves nothing")
}

// MARK: - Format: lossy in, JPEG out; lossless in, PNG out

@Test func exporterWritesJpegForALossySource() throws {
    let dir = exporterTempDir("lossy")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "photo", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "lossy source") else { return }
    #expect((ok.path as NSString).pathExtension == "jpg", "got \(ok.path)")
    #expect(exporterContainerType(ok.path) == UTType.jpeg.identifier,
            "the file's actual container must be JPEG, got \(String(describing: exporterContainerType(ok.path)))")
}

@Test func exporterWritesPngForALosslessSource() throws {
    let dir = exporterTempDir("lossless")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.png"), "photo", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "lossless source") else { return }
    #expect((ok.path as NSString).pathExtension == "png", "got \(ok.path)")
    #expect(exporterContainerType(ok.path) == UTType.png.identifier,
            "the file's actual container must be PNG, got \(String(describing: exporterContainerType(ok.path)))")
}

/// The two format tests above both use a filename with no extension, so they
/// cannot tell whether the choice follows the SOURCE or the requested output
/// name. This one hands a `.png`-suffixed name to a JPEG source and a
/// `.jpg`-suffixed name to a PNG source: the source must win.
@Test func exporterFormatFollowsTheSourceNotTheRequestedFilename() throws {
    let dir = exporterTempDir("format-name")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "misleading.png", 0.0, 0.0, 0.5, 0.5),
        exporterItem(exporterFixture("export-quadrants.png"), "misleading.jpg", 0.0, 0.0, 0.5, 0.5),
    ]))
    #expect(records.count == 2)
    guard case .ok(let jpegOut, _, _, _) = records[0], case .ok(let pngOut, _, _, _) = records[1] else {
        Issue.record("expected two .ok records, got \(records)")
        return
    }
    #expect(exporterContainerType(jpegOut) == UTType.jpeg.identifier, "JPEG source must stay JPEG: \(jpegOut)")
    #expect(exporterContainerType(pngOut) == UTType.png.identifier, "PNG source must stay PNG: \(pngOut)")
}

/// Formats with no fixture in this repo (HEIC, RAW, TIFF, WebP) are pinned
/// through the pure decision function instead, so the rule is covered for
/// every input the app actually accepts rather than only the two on disk.
@Test func exporterFormatRuleCoversHeicRawAndTiff() {
    #expect(Exporter.outputFormat(for: .jpeg) == .jpeg)
    #expect(Exporter.outputFormat(for: .heic) == .jpeg)
    #expect(Exporter.outputFormat(for: .heif) == .jpeg)
    #expect(Exporter.outputFormat(for: .webP) == .jpeg)
    #expect(Exporter.outputFormat(for: .png) == .png)
    #expect(Exporter.outputFormat(for: .tiff) == .png)
    #expect(Exporter.outputFormat(for: .rawImage) == .png)
    #expect(Exporter.outputFormat(for: UTType("com.sony.arw-raw-image")) == .png)
    #expect(Exporter.outputFormat(for: UTType("com.adobe.raw-image")) == .png)
    // Unknown container: PNG is the safe default -- it never adds a
    // generation of loss the source did not already have.
    #expect(Exporter.outputFormat(for: nil) == .png)
}

// MARK: - Colour management

/// Guards: "the output carries no embedded colour profile, or the wrong one."
///
/// `export-p3.jpg` is tagged Display P3. Re-encoding the cropped CGImage
/// straight through preserves that tag, so this fails unless the export
/// genuinely converts into sRGB.
@Test func exporterConvertsAWideGamutSourceToSrgb() throws {
    let dir = exporterTempDir("p3")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-p3.jpg"), "wide", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "P3 source") else { return }
    #expect(exporterProfileName(ok.path) == "sRGB IEC61966-2.1",
            "a Display P3 source must be converted, got \(String(describing: exporterProfileName(ok.path)))")
}

@Test func exporterEmbedsAnSrgbProfileInBothOutputFormats() throws {
    let dir = exporterTempDir("profile")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "j", 0.0, 0.0, 0.5, 0.5),
        exporterItem(exporterFixture("export-quadrants.png"), "p", 0.0, 0.0, 0.5, 0.5),
    ]))
    #expect(records.count == 2)
    for record in records {
        guard case .ok(let path, _, _, _) = record else {
            Issue.record("expected .ok, got \(record)")
            continue
        }
        #expect(exporterProfileName(path) == "sRGB IEC61966-2.1",
                "\(path) profile was \(String(describing: exporterProfileName(path)))")
    }
}

// MARK: - No rescaling

/// Guards: "the output is rescaled rather than being exactly the crop
/// window's pixels."
///
/// 0.1/0.2/0.3/0.4 of 1200x800 is exactly 120,160 360x320 -- integral with no
/// rounding ambiguity, so the assertion is on an exact number, not a range.
/// Both the record and the file on disk are checked: a record can report a
/// size the file does not have.
@Test func exporterOutputIsExactlyTheCropWindowInSourcePixels() throws {
    let dir = exporterTempDir("noscale")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "exact", 0.1, 0.2, 0.3, 0.4),
    ]))
    guard let ok = exporterOnlyOk(records, "exact crop") else { return }
    #expect(ok.width == 360, "record width was \(ok.width)")
    #expect(ok.height == 320, "record height was \(ok.height)")

    let source = try #require(CGImageSourceCreateWithURL(URL(fileURLWithPath: ok.path) as CFURL, nil))
    let props = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
    #expect(props[kCGImagePropertyPixelWidth] as? Int == 360, "file width")
    #expect(props[kCGImagePropertyPixelHeight] as? Int == 320, "file height")
}

/// A full-frame crop must reproduce the source's full resolution, not a
/// downscaled analysis-sized decode. `ImageLoader.loadThumbnail`'s default
/// `analysisMaxPixel` path would silently cap this.
@Test func exporterFullFrameCropKeepsTheSourceResolution() throws {
    let dir = exporterTempDir("fullframe")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "full", 0.0, 0.0, 1.0, 1.0),
    ]))
    guard let ok = exporterOnlyOk(records, "full frame") else { return }
    #expect(ok.width == 1200 && ok.height == 800, "got \(ok.width)x\(ok.height)")
}

@Test func exporterReportsTheByteCountOfTheFileItWrote() throws {
    let dir = exporterTempDir("bytes")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "b", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "bytes") else { return }
    let onDisk = try FileManager.default.attributesOfItem(atPath: ok.path)[.size] as? Int
    #expect(ok.bytes > 0)
    #expect(ok.bytes == onDisk, "record said \(ok.bytes) bytes, file is \(String(describing: onDisk))")
}

// MARK: - Failure records, never a crash

/// Guards: "a missing or undecodable source crashes the process instead of
/// producing a failure record."
@Test func exporterReturnsAFailedRecordForAMissingSource() throws {
    let dir = exporterTempDir("missing")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem("/nonexistent/nope.jpg", "gone", 0.0, 0.0, 0.5, 0.5),
    ]))
    #expect(records.count == 1)
    guard case .failed(let filename, let message) = records[0] else {
        Issue.record("expected .failed, got \(records[0])")
        return
    }
    #expect(filename == "gone")
    #expect(!message.isEmpty)
}

@Test func exporterReturnsFailedRecordsForEveryUndecodableSource() throws {
    let dir = exporterTempDir("hostile")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let hostile = ["empty.jpg", "truncated.jpg", "text.jpg", "random.jpg", "corrupt-scan-data.jpg"]
    let records = Exporter.export(ExportRequest(outputDir: dir, items: hostile.map {
        exporterItem(exporterFixture("hostile/\($0)"), $0, 0.0, 0.0, 0.5, 0.5)
    }))
    #expect(records.count == hostile.count)
    for (index, record) in records.enumerated() {
        switch record {
        case .failed(let filename, _):
            #expect(filename == hostile[index], "failure records must name the item that failed")
        case .ok(let path, let width, let height, _):
            // corrupt-scan-data.jpg has a valid header and ImageIO may
            // recover a garbage-but-decodable image from it. That is fine;
            // what must never happen is a crash or a zero-sized output.
            #expect(width > 0 && height > 0, "\(hostile[index]) produced \(width)x\(height) at \(path)")
        }
    }
}

/// A crop that resolves to zero pixels must be a failure record, not a
/// zero-byte file or a crash. Uses a NEGATIVE width: a zero width can "pass"
/// a naive guard through NaN propagation.
@Test func exporterReturnsAFailedRecordForADegenerateCropWindow() throws {
    let dir = exporterTempDir("degenerate")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let src = exporterFixture("export-quadrants.jpg")
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(src, "negative", 0.5, 0.5, -0.25, 0.25),
        exporterItem(src, "nan", 0.0, 0.0, Double.nan, 0.5),
        exporterItem(src, "tiny", 0.5, 0.5, 0.0001, 0.0001),
    ]))
    #expect(records.count == 3)
    for (index, record) in records.enumerated() where index < 2 {
        guard case .failed = record else {
            Issue.record("item \(index) should have failed, got \(record)")
            continue
        }
    }
    // A sub-pixel-but-positive window is clamped up to 1x1 rather than
    // failing -- it is a degenerate layout, not a broken source.
    guard case .ok(_, let w, let h, _) = records[2] else {
        Issue.record("a tiny positive crop should still export, got \(records[2])")
        return
    }
    #expect(w >= 1 && h >= 1)
}

@Test func exporterReturnsOneRecordPerItemInInputOrder() throws {
    let dir = exporterTempDir("ordering")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let good = exporterFixture("export-quadrants.jpg")
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(good, "first", 0.0, 0.0, 0.5, 0.5),
        exporterItem("/nonexistent/middle.jpg", "second", 0.0, 0.0, 0.5, 0.5),
        exporterItem(good, "third", 0.5, 0.5, 0.5, 0.5),
    ]))
    #expect(records.count == 3)
    guard case .ok(let firstPath, _, _, _) = records[0] else {
        Issue.record("record 0: \(records[0])"); return
    }
    guard case .failed(let secondName, _) = records[1] else {
        Issue.record("record 1: \(records[1])"); return
    }
    guard case .ok(let thirdPath, _, _, _) = records[2] else {
        Issue.record("record 2: \(records[2])"); return
    }
    #expect((firstPath as NSString).lastPathComponent == "first.jpg")
    #expect(secondName == "second")
    #expect((thirdPath as NSString).lastPathComponent == "third.jpg")
    // Content, not just names: a positional mix-up that still returns three
    // records in the right shape is caught here.
    exporterExpectColor(firstPath, x: 300, y: 200, exporterRed, "first")
    exporterExpectColor(thirdPath, x: 300, y: 200, exporterYellow, "third")
}

@Test func exporterCreatesTheOutputDirectoryIfItDoesNotExist() throws {
    let dir = exporterTempDir("nested/deeper/still")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    #expect(!FileManager.default.fileExists(atPath: dir))
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "made", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "directory creation") else { return }
    #expect(FileManager.default.fileExists(atPath: ok.path))
}

/// Filenames are reduced to their last path component so an item can never
/// write outside the directory the caller named.
@Test func exporterKeepsWritesInsideTheOutputDirectory() throws {
    let dir = exporterTempDir("escape")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let records = Exporter.export(ExportRequest(outputDir: dir, items: [
        exporterItem(exporterFixture("export-quadrants.jpg"), "../../escaped", 0.0, 0.0, 0.5, 0.5),
    ]))
    guard let ok = exporterOnlyOk(records, "path escape") else { return }
    #expect(ok.path.hasPrefix(dir + "/"), "wrote outside the output directory: \(ok.path)")
}

// MARK: - Protocol wiring

@Test func exporterProtocolDecodesAnExportRequest() throws {
    let json = """
    {"id":"e1","kind":"export","export":{"outputDir":"/tmp/out","items":[\
    {"sourcePath":"/a.jpg","filename":"p01","cropX":0.1,"cropY":0.2,"cropW":0.3,"cropH":0.4}]}}
    """
    let request = try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    #expect(request.kind == .export)
    let export = try #require(request.export)
    #expect(export.outputDir == "/tmp/out")
    #expect(export.items.count == 1)
    #expect(export.items[0].sourcePath == "/a.jpg")
    #expect(export.items[0].filename == "p01")
    #expect(export.items[0].cropX == 0.1)
    #expect(export.items[0].cropY == 0.2)
    #expect(export.items[0].cropW == 0.3)
    #expect(export.items[0].cropH == 0.4)
}

@Test func exporterProtocolEncodesAnExportedResult() throws {
    let response = Response(id: "e2", result: .exported([
        .ok(path: "/out/p01.jpg", width: 600, height: 400, bytes: 1234),
        .failed(filename: "p02", message: "unreadable"),
    ]))
    let text = String(decoding: try JSONEncoder().encode(response), as: UTF8.self)
    #expect(text.contains("\"type\":\"exported\""))
    #expect(text.contains("\"path\":\"\\/out\\/p01.jpg\"") || text.contains("\"path\":\"/out/p01.jpg\""))
    #expect(text.contains("\"width\":600"))
    #expect(text.contains("\"height\":400"))
    #expect(text.contains("\"bytes\":1234"))
    #expect(text.contains("\"filename\":\"p02\""))
    #expect(text.contains("\"message\":\"unreadable\""))
}

@Test func exporterProtocolRoundTripsExportedRecords() throws {
    let original: [ExportRecord] = [
        .ok(path: "/out/p01.jpg", width: 600, height: 400, bytes: 1234),
        .failed(filename: "p02", message: "unreadable"),
    ]
    let data = try JSONEncoder().encode(Response(id: "e3", result: .exported(original)))
    let decoded = try JSONDecoder().decode(Response.self, from: data)
    guard case .exported(let records) = decoded.result else {
        Issue.record("expected .exported, got \(decoded.result)")
        return
    }
    #expect(records.count == 2)
    guard case .ok(let path, let width, let height, let bytes) = records[0] else {
        Issue.record("record 0: \(records[0])"); return
    }
    #expect(path == "/out/p01.jpg" && width == 600 && height == 400 && bytes == 1234)
    guard case .failed(let filename, let message) = records[1] else {
        Issue.record("record 1: \(records[1])"); return
    }
    #expect(filename == "p02" && message == "unreadable")
}

@Test func exporterHandlerDispatchesAnExportRequest() throws {
    let dir = exporterTempDir("handler")
    defer { try? FileManager.default.removeItem(atPath: dir) }
    let source = exporterFixture("export-quadrants.jpg")
    let line = """
    {"id":"h1","kind":"export","export":{"outputDir":"\(dir)","items":[\
    {"sourcePath":"\(source)","filename":"h","cropX":0,"cropY":0,"cropW":0.5,"cropH":0.5}]}}
    """
    let response = handle(line: line)
    #expect(response.id == "h1")
    guard case .exported(let records) = response.result else {
        Issue.record("expected .exported, got \(response.result)")
        return
    }
    guard let ok = exporterOnlyOk(records, "handler dispatch") else { return }
    #expect(ok.width == 600 && ok.height == 400)
    exporterExpectColor(ok.path, x: 300, y: 200, exporterRed, "handler dispatch")
}

@Test func exporterHandlerReportsAnExportRequestWithNoPayload() throws {
    let response = handle(line: #"{"id":"h2","kind":"export"}"#)
    #expect(response.id == "h2")
    guard case .error(let err) = response.result else {
        Issue.record("a kind:export with no export payload must be an error, got \(response.result)")
        return
    }
    #expect(err.message.contains("export"))
}
