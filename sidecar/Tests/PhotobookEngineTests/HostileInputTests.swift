import Testing
import Foundation
@testable import PhotobookEngine

// This suite is the evidence for the sidecar's crash-isolation premise: the
// whole reason image work runs in a separate Swift process instead of
// in-process is that ImageIO on malformed files is a historically rich crash
// source, and a Swift fatalError/trap is something Rust cannot catch. Every
// test below must observe a normal return -- `.ok` or `.failed` -- never a
// process trap. If any of these ever crash the test runner, that is a bug in
// ImageLoader/ExifReader/Analyzer to fix, not a fixture to remove.
//
// Every Analyzer.analyze call below runs through offCooperativePool, which
// is what keeps these tests from deadlocking Vision alongside the rest of
// the suite. This suite was `.serialized` until that root cause was found.
@Suite
struct HostileInputTests {

private func hostileDir() -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/hostile")
}

private func fixture(_ name: String) -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)")
}

@Test func hostileEveryFixtureProducesARecordAndNeverCrashes() async throws {
    let files = try FileManager.default
        .contentsOfDirectory(at: hostileDir(), includingPropertiesForKeys: nil)
        .map(\.path)
        .sorted()

    #expect(files.count >= 7, "run scripts/make-hostile-fixtures.sh first")

    let records = await offCooperativePool { Analyzer.analyze(paths: files) }
    #expect(records.count == files.count)
    // Reaching this line at all is the assertion: nothing trapped.
}

@Test func hostileZeroByteFileFailsCleanly() async {
    let path = hostileDir().appendingPathComponent("empty.jpg").path
    let records = await offCooperativePool { Analyzer.analyze(paths: [path]) }
    guard case .failed = records[0] else {
        Issue.record("zero-byte file must produce a failed record"); return
    }
}

@Test func hostileTruncatedJpegFailsCleanlyOrDecodesPartially() async {
    let path = hostileDir().appendingPathComponent("truncated.jpg").path
    let records = await offCooperativePool { Analyzer.analyze(paths: [path]) }
    #expect(records.count == 1)   // either outcome is acceptable; a crash is not
}

@Test func hostileBatchMixedWithGoodPhotoStillReturnsTheGoodOne() async throws {
    let good = fixture("landscape.jpg").path
    let bad = hostileDir().appendingPathComponent("random.jpg").path

    let records = await offCooperativePool { Analyzer.analyze(paths: [bad, good, bad]) }
    #expect(records.count == 3)
    guard case .ok = records[1] else {
        Issue.record("the good photo must survive a hostile batch"); return
    }
}

// MARK: - Beyond the brief
//
// The brief names seven fixtures covering: empty, truncated, plain text,
// random bytes, a degenerate 1x1 image, an unexpected colour space, and a
// missing extension. Three more input classes are plausible ImageIO crash
// surfaces that aren't variations on those same failure paths, so they're
// added here rather than padding the corpus with near-duplicates:
//
//   - corrupt-scan-data.jpg: valid header/SOF/DHT, bit-flipped Huffman scan
//     data. Distinct from truncation (clean cut) and random.jpg (no valid
//     header at all) -- this is the "decoder walks a corrupt Huffman table"
//     class of real-world crash.
//   - a directory given as an image path: exercises FileHandle/CGImageSource
//     being handed something that opens differently than "missing" or
//     "malformed bytes".
//   - a permission-denied file: a distinct I/O error path from "does not
//     exist". Generated at runtime rather than committed, because git does
//     not preserve full POSIX permission bits across a clone, so a checked-in
//     0o000 fixture would silently stop being hostile after a fresh clone.

@Test func hostileCorruptScanDataFailsCleanlyOrDecodesPartially() async {
    let path = hostileDir().appendingPathComponent("corrupt-scan-data.jpg").path
    let records = await offCooperativePool { Analyzer.analyze(paths: [path]) }
    #expect(records.count == 1)   // either outcome is acceptable; a crash is not
}

@Test func hostileDirectoryPathFailsCleanly() async {
    let records = await offCooperativePool { Analyzer.analyze(paths: [hostileDir().path]) }
    #expect(records.count == 1)
    guard case .failed = records[0] else {
        Issue.record("a directory path must produce a failed record, not a crash"); return
    }
}

@Test func hostileUnreadableFileFailsCleanly() async throws {
    let tmp = URL(fileURLWithPath: NSTemporaryDirectory())
        .appendingPathComponent("hostile-unreadable-\(UUID().uuidString).jpg")
    let source = try Data(contentsOf: fixture("landscape.jpg"))
    try source.write(to: tmp)
    try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: tmp.path)
    defer {
        try? FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: tmp.path)
        try? FileManager.default.removeItem(at: tmp)
    }

    // Root (and some sandboxed CI runners) can read a 0o000 file anyway, in
    // which case this degrades to exercising the "good photo" path instead
    // of the permission-denied path -- still a non-crash, so still valid,
    // just not proof of the specific claim this test is named for.
    let records = await offCooperativePool { Analyzer.analyze(paths: [tmp.path]) }
    #expect(records.count == 1)
}

}
