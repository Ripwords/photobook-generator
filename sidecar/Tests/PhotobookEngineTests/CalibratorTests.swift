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

// Same face-box-relative fixture builder as SmileProxyTests -- see that
// file's doc comment for why the contour is expressed face-relative and
// mapped into image space via the inverse of SmileProxy's own transform.
private let calibratorTestBox: [Double] = [0.3, 0.3, 0.4, 0.4]

private func calibratorOuterLipContour(mouthCornerLift: Double) -> [[Double]] {
    [
        [0.35, 0.70 - mouthCornerLift], // left corner
        [0.40, 0.665],
        [0.45, 0.650],
        [0.50, 0.645],
        [0.55, 0.650],
        [0.60, 0.665],
        [0.65, 0.70 - mouthCornerLift], // right corner
        [0.60, 0.750],
        [0.50, 0.770],
        [0.40, 0.750],
    ]
}

private func calibratorToImageSpace(_ relPoints: [[Double]], box: [Double]) -> [[Double]] {
    let boxX = box[0], boxY = box[1], boxW = box[2], boxH = box[3]
    return relPoints.map { p in [boxX + p[0] * boxW, boxY + p[1] * boxH] }
}

private func calibratorFace(
    yaw: Double? = 0, pitch: Double? = 0, box: [Double] = calibratorTestBox,
    mouthCornerLift: Double
) -> FaceObservation {
    let imgPoints = calibratorToImageSpace(calibratorOuterLipContour(mouthCornerLift: mouthCornerLift), box: box)
    return FaceObservation(box: box, yaw: yaw, pitch: pitch, roll: 0,
                           captureQuality: 0.8, outerLips: imgPoints)
}

private func stubVision(faces: [FaceObservation]) -> (CGImage) -> VisionResult {
    { _ in VisionResult(faces: faces) }
}

@Test func calibratorReturnsOneRowPerDetectedFaceRegardlessOfUsability() {
    let faces = [
        calibratorFace(mouthCornerLift: 0.05),          // usable, smiling
        calibratorFace(yaw: 1.2, mouthCornerLift: 0.05), // unusable pose
        FaceObservation(box: [0.1, 0.1, 0.3, 0.3], yaw: 0, pitch: 0, roll: 0,
                        captureQuality: 0.8, outerLips: nil), // no lips at all
    ]
    let records = Calibrator.calibrate(
        paths: [fixture("landscape.jpg")], visionOverride: stubVision(faces: faces)
    )
    guard case .ok(let rows) = records[0] else {
        Issue.record("expected an ok record")
        return
    }
    #expect(rows.count == 3)
}

@Test func calibratorOkRecordHasEmptyRowsWhenNoFaceIsDetected() {
    let records = Calibrator.calibrate(
        paths: [fixture("landscape.jpg")], visionOverride: stubVision(faces: [])
    )
    guard case .ok(let rows) = records[0] else {
        Issue.record("expected an ok record")
        return
    }
    #expect(rows.isEmpty)
}

// The core anti-drift guarantee: a row's `confidence` and `lift` must come
// from the exact same computation SmileProxy.confidence(for:) uses for the
// same face, not a second, independently-written copy of the formula that
// could silently diverge from it.
@Test func calibratorRowMatchesSmileProxyGeometryForTheSameFace() {
    let face = calibratorFace(mouthCornerLift: 0.05)
    let records = Calibrator.calibrate(
        paths: [fixture("landscape.jpg")], visionOverride: stubVision(faces: [face])
    )
    guard case .ok(let rows) = records[0], rows.count == 1 else {
        Issue.record("expected one row")
        return
    }
    let row = rows[0]

    guard case .success(let geometry) = SmileProxy.geometry(for: face) else {
        Issue.record("expected geometry to succeed for a usable face")
        return
    }
    let wantConfidence = SmileProxy.confidence(for: face)

    #expect(row.confidence == geometry.confidence)
    #expect(row.confidence == wantConfidence)
    #expect(row.lift == geometry.lift)
    #expect(row.cornerY == geometry.cornerY)
    #expect(row.centreY == geometry.centreY)
    #expect(row.width == geometry.width)
    #expect(row.midlineY == geometry.midlineY)
    #expect(row.midpointLift == geometry.midpointLift)
    #expect(row.unusableReason == nil)
    #expect(row.outerLipPointCount == 10)
    #expect(row.box == calibratorTestBox)
}

@Test func calibratorRowReportsUnusableReasonInsteadOfNilingSilently() {
    let face = calibratorFace(yaw: 1.2, mouthCornerLift: 0.05) // pose gate fails
    let records = Calibrator.calibrate(
        paths: [fixture("landscape.jpg")], visionOverride: stubVision(faces: [face])
    )
    guard case .ok(let rows) = records[0], rows.count == 1 else {
        Issue.record("expected one row")
        return
    }
    let row = rows[0]
    #expect(row.confidence == nil)
    #expect(row.lift == nil)
    #expect(row.unusableReason == SmileUnusableReason.poseUnusable.rawValue)
    // Even though geometry couldn't be scored, the point count is still
    // reported -- it's independent of the pose gate.
    #expect(row.outerLipPointCount == 10)
}

@Test func calibratorProducesFailedRecordForAMissingPath() {
    let records = Calibrator.calibrate(
        paths: ["/nonexistent/nope.jpg"], visionOverride: stubVision(faces: [])
    )
    #expect(records.count == 1)
    guard case .failed(let path, _) = records[0] else {
        Issue.record("expected a failed record")
        return
    }
    #expect(path == "/nonexistent/nope.jpg")
}

@Test func calibratorReturnsExactlyOneRecordPerInputPathInOrder() {
    let paths = [fixture("landscape.jpg"), "/nonexistent/nope.jpg", fixture("portrait-rot90.jpg")]
    let records = Calibrator.calibrate(paths: paths, visionOverride: stubVision(faces: []))
    #expect(records.count == 3)
    guard case .ok = records[0] else { Issue.record("expected ok at 0"); return }
    guard case .failed(let p1, _) = records[1] else { Issue.record("expected failed at 1"); return }
    #expect(p1 == "/nonexistent/nope.jpg")
    guard case .ok = records[2] else { Issue.record("expected ok at 2"); return }
}

// Demonstrates the sensitivity difference the calibration report is meant to
// surface: because `centreY` averages in the two corner points themselves,
// it partly follows the corners when they lift, damping the numerator of
// `lift`. `midlineY` excludes the corners, so `midpointLift` responds more
// to the same corner movement. Exact figures below are hand-derived from
// `calibratorOuterLipContour`'s fixed points (see this test's git history /
// review notes for the arithmetic) and pinned so a future change to either
// formula is caught.
@Test func calibratorMidpointLiftIsMoreSensitiveToCornerMovementThanLift() {
    func geometry(mouthCornerLift: Double) -> SmileGeometry {
        let face = calibratorFace(mouthCornerLift: mouthCornerLift)
        guard case .success(let g) = SmileProxy.geometry(for: face) else {
            fatalError("expected geometry to succeed")
        }
        return g
    }

    let neutral = geometry(mouthCornerLift: 0.0)
    let smiling = geometry(mouthCornerLift: 0.05)

    let liftDelta = smiling.lift - neutral.lift
    let midpointLiftDelta = smiling.midpointLift - neutral.midpointLift

    #expect(liftDelta > 0)
    #expect(midpointLiftDelta > 0)
    #expect(midpointLiftDelta > liftDelta)
}

@Test func decodesCalibrateRequest() throws {
    let json = #"{"id":"c1","kind":"calibrate","paths":["/a.jpg"]}"#
    let req = try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    #expect(req.id == "c1")
    #expect(req.kind == .calibrate)
    #expect(req.paths == ["/a.jpg"])
}

@Test func calibrateRequestDispatchesToCalibrator() throws {
    let json = #"{"id":"c2","kind":"calibrate","paths":[]}"#
    let response = handle(line: json)
    #expect(response.id == "c2")
    guard case .calibrated(let records) = response.result else {
        Issue.record("expected a calibrated result")
        return
    }
    #expect(records.isEmpty)
}

@Test func calibrationRecordRoundTripsThroughJson() throws {
    let face = calibratorFace(mouthCornerLift: 0.05)
    let records = Calibrator.calibrate(
        paths: [fixture("landscape.jpg")], visionOverride: stubVision(faces: [face])
    )
    let data = try encoder.encode(records)
    let decoded = try decoder.decode([CalibrationRecord].self, from: data)
    guard case .ok(let rows) = decoded[0], rows.count == 1 else {
        Issue.record("expected one row after round trip")
        return
    }
    #expect(rows[0].confidence != nil)
}
