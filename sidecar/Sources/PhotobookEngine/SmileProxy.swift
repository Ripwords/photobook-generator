import Foundation

/// Geometric smile proxy from Apple Vision face landmarks.
///
/// Vision has no expression classifier at any macOS version, so "is this
/// person smiling" has to be inferred from landmark geometry: how far the
/// mouth corners sit above the mouth's own vertical centre, relative to
/// mouth width.
///
/// `FaceObservation.outerLips` are image-normalised, top-left-origin points
/// (see the doc comment on `FaceObservation` in `VisionAnalyzer.swift`) — the
/// same space as `box`, but NOT the same space Vision originally reported
/// them in (`VNFaceLandmarkRegion2D.normalizedPoints` are relative to the
/// face's own bounding box). Computing the lift/width ratio directly in
/// image space would be wrong: x has effectively been scaled by `box.width`
/// and y by `box.height`, so the ratio would be distorted by
/// `box.height / box.width` — a different, spurious factor for every face.
/// `confidence(for:)` therefore first maps each point back into
/// face-box-relative space before doing any ratio maths.
///
/// `outerLips` must contain *only* the mouth contour, not Vision's full
/// face-mesh point set. Feeding the whole ~65-76 point `allPoints` set
/// through this same geometry silently breaks it: the x-extremes become jaw
/// or ear contour points rather than mouth corners, and the mean y sits near
/// mid-face rather than at the mouth, so the resulting ratio stops tracking
/// expression at all (see `SmileProxyTests` for a worked example of exactly
/// how wrong this goes).
enum SmileProxy {
    /// Beyond roughly 30 degrees of yaw or pitch the mouth contour
    /// foreshortens enough that corner elevation is no longer meaningful.
    /// 0.52 rad ~= 30 degrees.
    static let maxYawRadians = 0.52
    static let maxPitchRadians = 0.52

    /// Vision does not publish a per-region landmark count anywhere in its
    /// headers or documentation -- only the *total* face-mesh constellation
    /// size (65 or 76 points, across ALL regions combined) is documented.
    /// "Vision's `outerLips` region reliably reports roughly 8-12 points" is
    /// therefore an ASSUMPTION, carried over from informal observation, not
    /// a measured or verified fact -- see the "NOT verified" list in
    /// docs/PROJECT-STATUS.md. `minOuterLipPoints` is set as a floor meant
    /// to sit below that assumed range: just high enough to reject a
    /// degenerate or malformed contour (e.g. a handful of stray points)
    /// without rejecting anything Vision would actually produce -- but this
    /// has never been checked against real photos. If Vision's real
    /// per-region count for `outerLips` turns out to be lower than assumed
    /// here, this floor silently disables smile detection library-wide.
    /// `confidence(for:)` below writes a one-line stderr warning whenever
    /// `outerLips` is present but under this floor, specifically so that
    /// failure mode is distinguishable from "pose was unusable" on the
    /// first real run -- check the real count first thing.
    static let minOuterLipPoints = 6

    /// Returns a 0...1 smile-likelihood proxy for a single face, or `nil`
    /// when it cannot be determined — missing or too-sparse lip landmarks, or
    /// head pose that is missing or beyond the usable range. `nil` must never
    /// be conflated with a low or zero score: "we could not tell" and "not
    /// smiling" are different facts. An unreported (`nil`) yaw or pitch is
    /// treated the same as an excessive one — "cannot tell" either way, and a
    /// missing pose plausibly correlates with a poor detection generally
    /// (occlusion, extreme angle), so scoring it as if frontal would feed a
    /// wrong signal into culling.
    static func confidence(for face: FaceObservation) -> Double? {
        guard let points = face.outerLips else { return nil }
        guard points.count >= minOuterLipPoints else {
            // Distinguishes "Vision reported a lip contour, but with fewer
            // points than `minOuterLipPoints` assumes it always has" from
            // "pose was unusable" or "no lip contour at all" -- all three
            // collapse to the same `nil` return below, but only this one
            // means the unverified assumption in `minOuterLipPoints`'s doc
            // comment might be miscalibrated against Vision's real
            // per-region count. Grep stderr for this on the first real run.
            FileHandle.standardError.write(Data(
                "PhotobookEngine: outerLips has \(points.count) points, below minOuterLipPoints (\(minOuterLipPoints))\n".utf8
            ))
            return nil
        }
        guard let yaw = face.yaw, abs(yaw) <= maxYawRadians else { return nil }
        guard let pitch = face.pitch, abs(pitch) <= maxPitchRadians else { return nil }

        guard face.box.count == 4 else { return nil }
        let boxX = face.box[0], boxY = face.box[1], boxW = face.box[2], boxH = face.box[3]
        guard boxW > 0.0001, boxH > 0.0001 else { return nil }

        // Undo the image-space offset-and-scale so the lift/width ratio is
        // computed in face-box-relative space, where the brief's calibration
        // constants below are meaningful.
        let relPoints: [[Double]] = points.map { p in
            [(p[0] - boxX) / boxW, (p[1] - boxY) / boxH]
        }

        // Outer lip contour: leftmost and rightmost points are the corners,
        // and the vertical midpoint of the contour is the reference line.
        guard let left = relPoints.min(by: { $0[0] < $1[0] }),
              let right = relPoints.max(by: { $0[0] < $1[0] })
        else { return nil }

        let width = right[0] - left[0]
        guard width > 0.0001 else { return nil }

        let cornerY = (left[1] + right[1]) / 2.0
        let centreY = relPoints.reduce(0.0) { $0 + $1[1] } / Double(relPoints.count)

        // Top-left origin: corners *above* the contour centre means a smaller y.
        let lift = (centreY - cornerY) / width

        // Map roughly [-0.15, 0.35] of normalised lift onto 0...1.
        return ((lift + 0.15) / 0.5).clamped(0, 1)
    }

    /// Fraction of faces scoring above `threshold`, counted only over faces
    /// with a usable pose. `nil` when no face has usable pose — distinct from
    /// `0`, which would mean "everyone we could assess wasn't smiling".
    static func fraction(faces: [FaceObservation], threshold: Double) -> Double? {
        let usable = faces.compactMap { confidence(for: $0) }
        guard !usable.isEmpty else { return nil }
        let smiling = usable.filter { $0 > threshold }.count
        return Double(smiling) / Double(usable.count)
    }
}
