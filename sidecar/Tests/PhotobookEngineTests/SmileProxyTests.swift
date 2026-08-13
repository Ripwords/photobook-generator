import Testing
@testable import PhotobookEngine

/// `FaceObservation.outerLips` are image-normalised, top-left-origin points
/// (same space as `box`) — NOT face-box-relative, as of Task 7's coordinate
/// fix (see the `FaceObservation` doc comment in `VisionAnalyzer.swift`) —
/// and contain ONLY the outer lip contour, not Vision's full face-mesh point
/// set (see the `SmileProxy` doc comment; this was a real bug found in
/// review, see `smileProxyProducesWrongSignalWhenFedFullFaceContourInsteadOfLips`
/// below for a worked demonstration).
///
/// This helper states the intended mouth geometry in face-box-relative terms
/// — a closed 10-point outer-lip contour, comparable in shape and count
/// (8-12 points) to what `VNFaceLandmarks2D.outerLips` actually reports —
/// and maps each point out into image space via the inverse of the
/// transform `SmileProxy.confidence(for:)` applies
/// (`p_img = box.origin + p_rel * box.size`). That keeps the fixture
/// coherent with the declared `box` (every point genuinely falls inside it)
/// and lets the test express what it means rather than pre-distorted numbers.
private let testBox: [Double] = [0.3, 0.3, 0.4, 0.4]

/// A realistic closed outer-lip contour, face-relative, 10 points running
/// left corner -> upper lip (left to right) -> right corner -> lower lip
/// (right to left) -> back to left corner. Only the two corner points move
/// with `mouthCornerLift`; the rest of the contour shape is held fixed, same
/// as a person raising the corners of their mouth while the rest of the lip
/// shape stays roughly put.
private func outerLipContour(mouthCornerLift: Double) -> [[Double]] {
    [
        [0.35, 0.70 - mouthCornerLift], // left corner
        [0.40, 0.665],                  // upper lip
        [0.45, 0.650],                  // upper lip
        [0.50, 0.645],                  // upper lip, cupid's bow
        [0.55, 0.650],                  // upper lip
        [0.60, 0.665],                  // upper lip
        [0.65, 0.70 - mouthCornerLift], // right corner
        [0.60, 0.750],                  // lower lip
        [0.50, 0.770],                  // lower lip
        [0.40, 0.750],                  // lower lip
    ]
}

private func toImageSpace(_ relPoints: [[Double]], box: [Double]) -> [[Double]] {
    let boxX = box[0], boxY = box[1], boxW = box[2], boxH = box[3]
    return relPoints.map { p in [boxX + p[0] * boxW, boxY + p[1] * boxH] }
}

private func face(yaw: Double?, pitch: Double? = 0, mouthCornerLift: Double) -> FaceObservation {
    let imgPoints = toImageSpace(outerLipContour(mouthCornerLift: mouthCornerLift), box: testBox)
    return FaceObservation(box: testBox, yaw: yaw, pitch: pitch, roll: 0,
                           captureQuality: 0.8, outerLips: imgPoints)
}

@Test func smileProxyReturnsNilWhenHeadIsTurnedTooFar() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.9, mouthCornerLift: 0.05)) == nil)
}

@Test func smileProxyReturnsNilWhenPitchIsTooSteep() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.0, pitch: -0.9, mouthCornerLift: 0.05)) == nil)
}

// A missing (nil) yaw is treated the same as an excessive one: "cannot tell"
// either way, and per review, an unreported pose plausibly correlates with a
// poor detection generally (occlusion, extreme angle) so scoring it as if
// frontal would feed a wrong signal into culling.
@Test func smileProxyReturnsNilWhenYawIsMissing() {
    #expect(SmileProxy.confidence(for: face(yaw: nil, mouthCornerLift: 0.05)) == nil)
}

@Test func smileProxyReturnsNilWhenPitchIsMissing() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.0, pitch: nil, mouthCornerLift: 0.05)) == nil)
}

@Test func smileProxyReturnsNilWhenOuterLipsAreMissing() {
    let f = FaceObservation(box: [0, 0, 1, 1], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8, outerLips: nil)
    #expect(SmileProxy.confidence(for: f) == nil)
}

// Vision's outerLips reliably reports roughly 8-12 points; a handful of
// stray points (fewer than SmileProxy.minOuterLipPoints) is a degenerate
// contour, not a real detection, and must fail closed to nil.
@Test func smileProxyReturnsNilWhenTooFewLipPointsAreProvided() {
    let f = FaceObservation(box: testBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                            outerLips: [[0.44, 0.56], [0.5, 0.572], [0.56, 0.56], [0.5, 0.596]])
    #expect(SmileProxy.confidence(for: f) == nil)
}

@Test func smileProxyReturnsNilWhenBoxIsMalformed() {
    let f = FaceObservation(box: [0.3, 0.3, 0.4], yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                            outerLips: toImageSpace(outerLipContour(mouthCornerLift: 0.05), box: testBox))
    #expect(SmileProxy.confidence(for: f) == nil)
}

// Regression guard for a zero box width or height (a malformed or degenerate
// face box): must fail closed to nil rather than dividing by zero.
@Test func smileProxyReturnsNilWhenBoxHasZeroWidth() {
    let box: [Double] = [0.3, 0.3, 0.0, 0.4]
    let f = FaceObservation(box: box, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                            outerLips: toImageSpace(outerLipContour(mouthCornerLift: 0.05), box: testBox))
    #expect(SmileProxy.confidence(for: f) == nil)
}

// Regression guard for a *negative* box width (a malformed face box). Unlike
// the zero-width case, a negative width does not produce NaN downstream — it
// silently flips the sign of the face-relative x coordinates and produces a
// numerically plausible-looking (but meaningless) score. Only the explicit
// `boxW > 0.0001` guard catches this; verified by temporarily loosening that
// guard, which made an equivalent fixture return a real (non-nil) score.
@Test func smileProxyReturnsNilWhenBoxHasNegativeWidth() {
    let malformedBox: [Double] = [0.7, 0.3, -0.4, 0.4]
    let f = FaceObservation(box: malformedBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                            outerLips: toImageSpace(outerLipContour(mouthCornerLift: 0.05), box: testBox))
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
    let wideBox: [Double] = [0.1, 0.4, 0.6, 0.2]
    let tallBox: [Double] = [0.4, 0.1, 0.2, 0.6]
    let wideFace = FaceObservation(box: wideBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                                   outerLips: toImageSpace(outerLipContour(mouthCornerLift: 0.05), box: wideBox))
    let tallFace = FaceObservation(box: tallBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
                                   outerLips: toImageSpace(outerLipContour(mouthCornerLift: 0.05), box: tallBox))

    let wide = SmileProxy.confidence(for: wideFace)
    let tall = SmileProxy.confidence(for: tallFace)
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

// --- Regression demonstration for the Task 7/8 review finding ---
//
// This is the bug the review caught: VisionAnalyzer used to populate
// `landmarks` (now `outerLips`) from Vision's *entire* ~65-76 point face
// mesh (`allPoints`) rather than the outer lip contour alone. SmileProxy's
// min/max-by-x and mean-y geometry is only meaningful over lip points; fed a
// full-face point set, the x-extremes become jaw/ear contour points instead
// of mouth corners, and the mean y sits near mid-face instead of the mouth.
//
// This test builds exactly that contaminated input — the same lip contour
// plus jaw, eyebrow, and nose points a full `allPoints` set would include —
// and shows the smile signal doesn't just weaken, it inverts: the
// "frowning" fixture scores *higher* than the "smiling" one. Contrast with
// `smileProxyLiftedMouthCornersScoreHigherThanFlat` above, which uses the
// same two mouthCornerLift values on lips-only input and correctly orders
// them the other way. This is why `VisionAnalyzer` must populate
// `outerLips` from Vision's `outerLips` region specifically, never `allPoints`.
@Test func smileProxyProducesWrongSignalWhenFedFullFaceContourInsteadOfLips() {
    func fullFaceContaminatedPoints(mouthCornerLift: Double) -> [[Double]] {
        let lips = outerLipContour(mouthCornerLift: mouthCornerLift)
        let nonLipFacePoints: [[Double]] = [
            [0.05, 0.50], // jaw contour, left
            [0.95, 0.50], // jaw contour, right
            [0.30, 0.15], // eyebrow, left
            [0.70, 0.15], // eyebrow, right
            [0.50, 0.45], // nose tip
        ]
        return toImageSpace(lips + nonLipFacePoints, box: testBox)
    }

    let smilingContaminated = FaceObservation(
        box: testBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
        outerLips: fullFaceContaminatedPoints(mouthCornerLift: 0.05))
    let neutralContaminated = FaceObservation(
        box: testBox, yaw: 0, pitch: 0, roll: 0, captureQuality: 0.8,
        outerLips: fullFaceContaminatedPoints(mouthCornerLift: 0.0))

    let smiling = SmileProxy.confidence(for: smilingContaminated)
    let neutral = SmileProxy.confidence(for: neutralContaminated)
    #expect(smiling != nil && neutral != nil)

    // Wrong on contaminated input: the "smiling" fixture does NOT score
    // higher, unlike the correctly-scoped lips-only case above.
    #expect(smiling! <= neutral!)
}
