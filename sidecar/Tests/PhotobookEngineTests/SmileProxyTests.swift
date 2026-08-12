import Testing
@testable import PhotobookEngine

/// `FaceObservation.landmarks` are image-normalised, top-left-origin points
/// (same space as `box`) — NOT face-box-relative, as of Task 7's coordinate
/// fix (see the `FaceObservation` doc comment in `VisionAnalyzer.swift`).
///
/// This helper states the intended mouth geometry in face-box-relative terms
/// — left corner, centre-top, right corner, centre-bottom, the minimal shape
/// `SmileProxy` reads — and maps each point out into image space via the
/// inverse of the transform `SmileProxy.confidence(for:)` applies
/// (`p_img = box.origin + p_rel * box.size`). That keeps the fixture
/// coherent with the declared `box` (every point genuinely falls inside it)
/// and lets the test express what it means rather than pre-distorted numbers.
private let testBox: [Double] = [0.3, 0.3, 0.4, 0.4]

private func face(yaw: Double?, pitch: Double? = 0, mouthCornerLift: Double) -> FaceObservation {
    // Face-relative points: left corner, centre-top, right corner, centre-bottom.
    let relPoints: [[Double]] = [
        [0.35, 0.70 - mouthCornerLift],
        [0.50, 0.68],
        [0.65, 0.70 - mouthCornerLift],
        [0.50, 0.74],
    ]
    let boxX = testBox[0], boxY = testBox[1], boxW = testBox[2], boxH = testBox[3]
    let imgPoints = relPoints.map { p in [boxX + p[0] * boxW, boxY + p[1] * boxH] }
    return FaceObservation(box: testBox, yaw: yaw, pitch: pitch, roll: 0,
                           captureQuality: 0.8, landmarks: imgPoints)
}

@Test func smileProxyReturnsNilWhenHeadIsTurnedTooFar() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.9, mouthCornerLift: 0.05)) == nil)
}

@Test func smileProxyReturnsNilWhenPitchIsTooSteep() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.0, pitch: -0.9, mouthCornerLift: 0.05)) == nil)
}

@Test func smileProxyReturnsNilWhenLandmarksAreMissing() {
    let f = FaceObservation(box: [0, 0, 1, 1], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8, landmarks: nil)
    #expect(SmileProxy.confidence(for: f) == nil)
}

// Regression guard for a zero box width or height (a malformed or degenerate
// face box): must fail closed to nil rather than dividing by zero.
@Test func smileProxyReturnsNilWhenBoxHasZeroWidth() {
    let f = FaceObservation(box: [0.3, 0.3, 0.0, 0.4], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8,
                            landmarks: [[0.35, 0.65], [0.5, 0.68], [0.65, 0.65], [0.5, 0.74]])
    #expect(SmileProxy.confidence(for: f) == nil)
}

// Regression guard for a *negative* box width (a malformed face box). Unlike
// the zero-width case, a negative width does not produce NaN downstream — it
// silently flips the sign of the face-relative x coordinates and produces a
// numerically plausible-looking (but meaningless) score. Only the explicit
// `boxW > 0.0001` guard catches this; verified by temporarily loosening that
// guard, which made this exact fixture return `Optional(0.4999...)` instead
// of nil.
@Test func smileProxyReturnsNilWhenBoxHasNegativeWidth() {
    let f = FaceObservation(box: [0.7, 0.3, -0.4, 0.4], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8,
                            landmarks: [[0.44, 0.56], [0.5, 0.572], [0.56, 0.56], [0.5, 0.596]])
    #expect(SmileProxy.confidence(for: f) == nil)
}

@Test func smileProxyLiftedMouthCornersScoreHigherThanFlat() {
    let smiling = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.05))
    let neutral = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.0))
    #expect(smiling != nil && neutral != nil)
    #expect(smiling! > neutral!)
}

// The whole point of mapping landmarks back into face-box-relative space
// before computing the lift/width ratio (see SmileProxy's doc comment) is
// that the result must not depend on the face box's aspect ratio. This test
// uses two *non-square* boxes with different aspect ratios but identical
// face-relative mouth geometry: if the box-relative conversion were skipped
// (or done wrong), the image-space ratio would be distorted by
// box.height/box.width, which differs between the two boxes here, and the
// scores would diverge instead of matching.
@Test func smileProxyConfidenceIsInvariantToFaceBoxAspectRatio() {
    func faceWithBox(_ box: [Double], mouthCornerLift: Double) -> FaceObservation {
        let relPoints: [[Double]] = [
            [0.35, 0.70 - mouthCornerLift],
            [0.50, 0.68],
            [0.65, 0.70 - mouthCornerLift],
            [0.50, 0.74],
        ]
        let boxX = box[0], boxY = box[1], boxW = box[2], boxH = box[3]
        let imgPoints = relPoints.map { p in [boxX + p[0] * boxW, boxY + p[1] * boxH] }
        return FaceObservation(box: box, yaw: 0, pitch: 0, roll: 0,
                               captureQuality: 0.8, landmarks: imgPoints)
    }

    let wide = SmileProxy.confidence(for: faceWithBox([0.1, 0.4, 0.6, 0.2], mouthCornerLift: 0.05))
    let tall = SmileProxy.confidence(for: faceWithBox([0.4, 0.1, 0.2, 0.6], mouthCornerLift: 0.05))
    #expect(wide != nil && tall != nil)
    #expect(abs(wide! - tall!) < 1e-9)
}

// Explicitly distinguishes "could not tell" (nil) from "told us zero/low"
// (a real Double). A test that only asserted "not greater than some
// threshold" would pass whether confidence returned nil or 0, which is
// exactly the confusion this design exists to prevent.
@Test func smileProxyDistinguishesNilFromALowRealScore() {
    let unusable = SmileProxy.confidence(for: face(yaw: 1.2, mouthCornerLift: 0.05))
    let neutral = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.0))
    #expect(unusable == nil)
    #expect(neutral != nil)
}

@Test func smileProxyFractionIsNilWhenNoFaceHasUsablePose() {
    let faces = [face(yaw: 1.2, mouthCornerLift: 0.05), face(yaw: -1.1, mouthCornerLift: 0.05)]
    #expect(SmileProxy.fraction(faces: faces, threshold: 0.5) == nil)
}

@Test func smileProxyFractionCountsOnlyUsableFaces() {
    let faces = [
        face(yaw: 0.0, mouthCornerLift: 0.08),   // usable, smiling
        face(yaw: 0.0, mouthCornerLift: 0.0),    // usable, not smiling
        face(yaw: 1.2, mouthCornerLift: 0.08),   // unusable, excluded entirely
    ]
    let f = SmileProxy.fraction(faces: faces, threshold: 0.5)
    #expect(f != nil)
    #expect(abs(f! - 0.5) < 0.001)
}

@Test func smileProxyFractionIsNilForEmptyFaceList() {
    #expect(SmileProxy.fraction(faces: [], threshold: 0.5) == nil)
}
