### Task 8: Smile proxy from landmarks

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/SmileProxy.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`

**Interfaces:**
- Consumes: `FaceObservation` from Task 7
- Produces: `SmileProxy.confidence(for face: FaceObservation) -> Double?` (nil when head pose is unusable) and `SmileProxy.fraction(faces: [FaceObservation], threshold: Double) -> Double?` (nil when no face has usable pose).

Vision has no expression classifier at any macOS version, so this is geometric. It returns **nil, never 0**, when it cannot tell — "nobody smiling" and "couldn't tell" must not collapse.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`:

```swift
import Testing
@testable import PhotobookEngine

/// Vision's allPoints ordering places the outer lip contour last; these helpers
/// build the minimal shape the proxy reads.
private func face(yaw: Double, mouthCornerLift: Double) -> FaceObservation {
    // Points: left corner, centre-top, right corner, centre-bottom.
    let pts: [[Double]] = [
        [0.35, 0.70 - mouthCornerLift],
        [0.50, 0.68],
        [0.65, 0.70 - mouthCornerLift],
        [0.50, 0.74],
    ]
    return FaceObservation(box: [0.3, 0.3, 0.4, 0.4], yaw: yaw, pitch: 0, roll: 0,
                           captureQuality: 0.8, landmarks: pts)
}

@Test func returnsNilWhenHeadIsTurnedTooFar() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.9, mouthCornerLift: 0.05)) == nil)
}

@Test func returnsNilWhenLandmarksAreMissing() {
    let f = FaceObservation(box: [0, 0, 1, 1], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8, landmarks: nil)
    #expect(SmileProxy.confidence(for: f) == nil)
}

@Test func liftedMouthCornersScoreHigherThanFlat() {
    let smiling = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.05))
    let neutral = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.0))
    #expect(smiling != nil && neutral != nil)
    #expect(smiling! > neutral!)
}

@Test func fractionIsNilWhenNoFaceHasUsablePose() {
    let faces = [face(yaw: 1.2, mouthCornerLift: 0.05), face(yaw: -1.1, mouthCornerLift: 0.05)]
    #expect(SmileProxy.fraction(faces: faces, threshold: 0.5) == nil)
}

@Test func fractionCountsOnlyUsableFaces() {
    let faces = [
        face(yaw: 0.0, mouthCornerLift: 0.08),   // usable, smiling
        face(yaw: 0.0, mouthCornerLift: 0.0),    // usable, not smiling
        face(yaw: 1.2, mouthCornerLift: 0.08),   // unusable, excluded entirely
    ]
    let f = SmileProxy.fraction(faces: faces, threshold: 0.5)
    #expect(f != nil)
    #expect(abs(f! - 0.5) < 0.001)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `SmileProxy` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/SmileProxy.swift`:

```swift
import Foundation

enum SmileProxy {
    /// Beyond roughly 30 degrees of yaw the mouth contour foreshortens enough
    /// that corner elevation is no longer meaningful. 0.52 rad ~= 30 degrees.
    static let maxYawRadians = 0.52
    static let maxPitchRadians = 0.52

    static func confidence(for face: FaceObservation) -> Double? {
        guard let points = face.landmarks, points.count >= 4 else { return nil }
        if let yaw = face.yaw, abs(yaw) > maxYawRadians { return nil }
        if let pitch = face.pitch, abs(pitch) > maxPitchRadians { return nil }

        // Outer lip contour: leftmost and rightmost points are the corners,
        // and the vertical midpoint of the contour is the reference line.
        guard let left = points.min(by: { $0[0] < $1[0] }),
              let right = points.max(by: { $0[0] < $1[0] })
        else { return nil }

        let width = right[0] - left[0]
        guard width > 0.0001 else { return nil }

        let cornerY = (left[1] + right[1]) / 2.0
        let centreY = points.reduce(0.0) { $0 + $1[1] } / Double(points.count)

        // Top-left origin: corners *above* the contour centre means a smaller y.
        let lift = (centreY - cornerY) / width

        // Map roughly [-0.15, 0.35] of normalised lift onto 0...1.
        return ((lift + 0.15) / 0.5).clamped(0, 1)
    }

    static func fraction(faces: [FaceObservation], threshold: Double) -> Double? {
        let usable = faces.compactMap { confidence(for: $0) }
        guard !usable.isEmpty else { return nil }
        let smiling = usable.filter { $0 > threshold }.count
        return Double(smiling) / Double(usable.count)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 5 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add pose-gated smile proxy from face landmarks"
```

---

