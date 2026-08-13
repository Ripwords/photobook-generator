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

// MARK: - Size-aware two-pass loading (RAW/HEIC decode performance fix)
//
// `postage-stamp-source.jpg` has a 2000x1333 main image but an ImageIO-
// generated embedded thumbnail of only ~160x106 (see
// scripts/make-fixtures.swift). This is the exact shape a real iPhone JPEG
// has, and it is the case `kCGImageSourceCreateThumbnailFromImageIfAbsent`
// alone gets wrong: it happily returns that ~160px thumbnail as "the"
// thumbnail, which would silently degrade every classical metric.

@Test func fallsBackToFullDecodeWhenEmbeddedPreviewIsBelowTheFloor() throws {
    let img = try ImageLoader.loadThumbnail(
        path: fixture("postage-stamp-source.jpg"), maxPixel: 1536, floorPixel: 1536
    )
    // The embedded thumbnail alone is ~160x106. If the floor check were
    // missing (i.e. this just used `IfAbsent` unconditionally), `img` would
    // be that tiny embedded thumbnail and this assertion would fail --
    // that's the mutation this test exists to catch.
    #expect(max(img.width, img.height) > 1000,
             "must fall back to a full decode, not settle for the ~160px embedded thumbnail")
}

@Test func acceptsEmbeddedPreviewWhenItAlreadyMeetsTheFloor() throws {
    // landscape.jpg has no embedded thumbnail at all, so
    // kCGImageSourceCreateThumbnailFromImageIfAbsent decodes the full 1200x800
    // image itself -- there is nothing smaller to fall back from, so the
    // floor is met on the first pass.
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 512, floorPixel: 512)
    #expect(max(img.width, img.height) <= 512)
    #expect(img.width > img.height)
}

@Test func defaultFloorIsMaxPixelWhenNotSpecified() throws {
    // Same fixture/assertion as the explicit-floor fallback test above, but
    // omitting `floorPixel` entirely -- pins the documented default
    // (floor == maxPixel) so a caller like Analyzer that only passes
    // `maxPixel` still gets the fallback protection, not silently no floor
    // at all.
    let img = try ImageLoader.loadThumbnail(path: fixture("postage-stamp-source.jpg"), maxPixel: 1536)
    #expect(max(img.width, img.height) > 1000)
}

@Test func orientationIsAppliedOnTheFallbackPath() throws {
    // portrait-rot90-tiny-thumb.jpg is stored landscape (2000x1333) tagged
    // orientation 6, with a tiny embedded thumbnail -- floor forces the
    // fallback (kCGImageSourceCreateThumbnailFromImageAlways) path.
    let img = try ImageLoader.loadThumbnail(
        path: fixture("portrait-rot90-tiny-thumb.jpg"), maxPixel: 1536, floorPixel: 1536
    )
    #expect(img.height > img.width, "orientation must still be applied on the full-decode fallback path")
}

@Test func orientationIsAppliedOnTheAcceptedPreviewPath() throws {
    // Same fixture, but with a floor low enough (50px) that the ~106x160
    // embedded thumbnail itself satisfies it -- no fallback fires. Proves
    // kCGImageSourceCreateThumbnailWithTransform reconciles orientation on
    // the embedded-preview path too, not just the full-decode path.
    let img = try ImageLoader.loadThumbnail(
        path: fixture("portrait-rot90-tiny-thumb.jpg"), maxPixel: 1536, floorPixel: 50
    )
    #expect(img.height > img.width, "orientation must be applied on the accepted-preview path too")
    #expect(max(img.width, img.height) < 300, "sanity check: this path should have accepted the small embedded preview")
}

@Test func detailedLoadReportsWhetherTheFallbackFired() throws {
    let fellBack = try ImageLoader.loadThumbnailDetailed(
        path: fixture("postage-stamp-source.jpg"), maxPixel: 1536, floorPixel: 1536
    )
    #expect(fellBack.usedFullDecodeFallback == true)

    let tookPreview = try ImageLoader.loadThumbnailDetailed(
        path: fixture("postage-stamp-source.jpg"), maxPixel: 1536, floorPixel: 50
    )
    #expect(tookPreview.usedFullDecodeFallback == false)
}
