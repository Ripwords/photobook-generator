//! Deterministic crop-window selection.
//!
//! No search and no seed: given a photo and a target aspect there is exactly
//! one answer, which is what keeps golden files stable and makes
//! "regenerate" feel like browsing alternatives rather than rolling dice.

use crate::book::cull::Photo;
use crate::geometry::Rect;

/// Weight of a face relative to the generic saliency box when deciding where
/// to centre the crop. Faces are what a viewer looks at first; Vision's
/// attention saliency is a weaker signal and routinely lands on scenery.
const FACE_WEIGHT: f64 = 3.0;
const SALIENCY_WEIGHT: f64 = 1.0;

/// Returns the crop window in the photo's own normalised coordinates.
///
/// The window is the LARGEST rect of `target_aspect` that fits inside the
/// photo, positioned so its centre is as close as possible to the weighted
/// centroid of faces and salient content, clamped to stay in bounds. It
/// never upscales and never distorts.
pub fn choose_crop(photo: &Photo, target_aspect: f64) -> Rect {
    let photo_aspect = photo.aspect();

    // Largest rect of the target aspect that fits. Working in normalised
    // space, a target WIDER than the photo keeps full width; a NARROWER
    // target keeps full height.
    let (w, h) = if target_aspect >= photo_aspect {
        (1.0, photo_aspect / target_aspect)
    } else {
        (target_aspect / photo_aspect, 1.0)
    };

    let (cx, cy) = focus_centroid(photo);

    // Centre the window on the focus, then clamp. `max(0.0)` guards the
    // degenerate case where the window is the full extent on an axis.
    let x = (cx - w / 2.0).clamp(0.0, (1.0 - w).max(0.0));
    let y = (cy - h / 2.0).clamp(0.0, (1.0 - h).max(0.0));

    Rect::new(x, y, w, h)
}

/// Weighted centroid of everything worth keeping. Falls back to the frame
/// centre when the photo has neither faces nor a saliency box, which is the
/// correct neutral answer rather than a bias toward any corner.
fn focus_centroid(photo: &Photo) -> (f64, f64) {
    let mut total = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;

    for face in &photo.faces {
        // Area-weighted so a large foreground face outranks a small one in
        // the background, and quality-weighted so a blurred face pulls less.
        let quality = face.capture_quality.unwrap_or(0.5).clamp(0.0, 1.0);
        let weight = FACE_WEIGHT * face.box_.area().max(1e-6) * (0.5 + quality);
        total += weight;
        sx += weight * (face.box_.x + face.box_.w / 2.0);
        sy += weight * (face.box_.y + face.box_.h / 2.0);
    }

    if let Some(s) = photo.saliency_box {
        let weight = SALIENCY_WEIGHT * s.area().max(1e-6);
        total += weight;
        sx += weight * (s.x + s.w / 2.0);
        sy += weight * (s.y + s.h / 2.0);
    }

    if total <= 0.0 {
        return (0.5, 0.5);
    }
    (sx / total, sy / total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, PaletteColor};

    /// NEVER square: a square photo makes width/height == 1 and hides the
    /// entire class of aspect bug (a real Phase 1 failure).
    fn landscape_photo() -> Photo {
        Photo {
            path: "/p.jpg".into(),
            hash: "h".into(),
            width: 4000,
            height: 3000, // 4:3
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::<PaletteColor>::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
        }
    }

    #[test]
    fn crop_to_a_wider_target_keeps_full_width_and_trims_height() {
        let p = landscape_photo(); // 1.333
        let c = choose_crop(&p, 2.0);
        assert!((c.w - 1.0).abs() < 1e-9, "full width kept, was {}", c.w);
        // 4000 wide at 2:1 needs 2000 tall of 3000 -> 0.6667 normalised.
        assert!((c.h - 2.0 / 3.0).abs() < 1e-6, "h was {}", c.h);
    }

    #[test]
    fn crop_to_a_narrower_target_keeps_full_height_and_trims_width() {
        let p = landscape_photo();
        let c = choose_crop(&p, 1.0);
        assert!((c.h - 1.0).abs() < 1e-9, "full height kept, was {}", c.h);
        // 3000 tall at 1:1 needs 3000 wide of 4000 -> 0.75 normalised.
        assert!((c.w - 0.75).abs() < 1e-6, "w was {}", c.w);
    }

    #[test]
    fn crop_centres_when_there_is_no_saliency_or_face() {
        let p = landscape_photo();
        let c = choose_crop(&p, 1.0);
        assert!((c.x - 0.125).abs() < 1e-6, "x was {}", c.x);
    }

    #[test]
    fn crop_follows_the_saliency_box_off_centre() {
        let mut p = landscape_photo();
        // Salient content hard against the LEFT of the frame.
        p.saliency_box = Some(Rect::new(0.02, 0.3, 0.20, 0.4));
        let c = choose_crop(&p, 1.0);
        assert!(c.x < 0.125, "crop should move left, x was {}", c.x);
        assert!(c.x >= 0.0, "crop must stay inside the photo, x was {}", c.x);
    }

    /// The saliency box here is deliberately LARGER (area 0.126) than the
    /// face's raw area*(quality-bonus) term (0.063) so that at equal
    /// weighting (FACE_WEIGHT == SALIENCY_WEIGHT) the saliency box already
    /// wins and pulls the crop left of centre. Only `FACE_WEIGHT`'s 3x
    /// multiplier flips that outcome and pulls the crop right. A smaller
    /// saliency box (as an earlier version of this fixture used) lets the
    /// face's area/quality terms dominate on their own, so the test would
    /// keep passing even with FACE_WEIGHT reduced to parity -- exactly the
    /// kind of decorative test this project's CLAUDE.md warns about.
    #[test]
    fn crop_prefers_a_face_over_generic_saliency() {
        let mut p = landscape_photo();
        p.saliency_box = Some(Rect::new(0.02, 0.3, 0.28, 0.45)); // left, large
        p.faces = vec![Face { box_: Rect::new(0.80, 0.3, 0.15, 0.3), capture_quality: Some(0.9) }];
        let c = choose_crop(&p, 1.0);
        assert!(c.x > 0.125, "the face should pull the crop right, x was {}", c.x);
    }

    #[test]
    fn crop_never_leaves_the_photo_bounds() {
        let mut p = landscape_photo();
        p.faces = vec![Face { box_: Rect::new(0.97, 0.9, 0.03, 0.1), capture_quality: Some(0.5) }];
        let c = choose_crop(&p, 1.0);
        assert!(c.x >= -1e-9 && c.right() <= 1.0 + 1e-9, "x={} right={}", c.x, c.right());
        assert!(c.y >= -1e-9 && c.bottom() <= 1.0 + 1e-9);
    }

    #[test]
    fn crop_is_deterministic_across_repeated_calls() {
        let mut p = landscape_photo();
        p.saliency_box = Some(Rect::new(0.3, 0.2, 0.4, 0.5));
        p.faces = vec![Face { box_: Rect::new(0.5, 0.4, 0.1, 0.2), capture_quality: Some(0.6) }];
        let a = choose_crop(&p, 1.6);
        let b = choose_crop(&p, 1.6);
        assert_eq!(a, b);
    }

    // The ratio check alone is not enough: swapping which branch handles
    // the wide/narrow case still yields the right RATIO (the formula is
    // algebraically self-consistent either way -- e.g. target 0.7 under a
    // swapped branch produces w=1.0, h=1.905, and 4000/(1.905*3000) is
    // still 0.7), while silently upscaling one axis past the photo's own
    // extent. So this test also pins the never-upscale bound (w, h <= 1.0),
    // which the ratio alone cannot catch.
    #[test]
    fn crop_matches_the_target_aspect_in_real_pixels() {
        let p = landscape_photo();
        for target in [0.7, 1.0, 1.6, 2.4] {
            let c = choose_crop(&p, target);
            let real = (c.w * p.width as f64) / (c.h * p.height as f64);
            assert!((real - target).abs() < 1e-6, "target {target} produced {real}");
            assert!(c.w <= 1.0 + 1e-9 && c.h <= 1.0 + 1e-9,
                "target {target} upscaled an axis: w={} h={}", c.w, c.h);
        }
    }
}
