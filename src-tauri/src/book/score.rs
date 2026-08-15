//! Scoring a template plus a photo-to-slot assignment.
//!
//! Hard constraints REJECT a candidate; soft terms weight it. The split is
//! deliberate: a face cut in half is not a slightly worse layout, it is a
//! ruined photo, and no weighting scheme should ever be able to outvote it.

use crate::book::crop::choose_crop;
use crate::book::cull::Photo;
use crate::geometry::{
    clear_of_gutter, gutter_overlap_area, in_safe_margin, Rect, Side, PAGE_H_IN, PAGE_W_IN,
};
use crate::templates::{Role, Slot, SpreadTemplate, Weights};

/// Pixajoy's published minimum: below this, print is visibly soft and no
/// downstream step can fix it. Their recommended target is 300 DPI, which
/// `resolution_headroom` below treats as a WARNING band, not a second floor.
pub const MIN_DPI: f64 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    FaceClipped,
    FaceInGutter,
    FaceInSafeMargin,
    TooLowResolution,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub template_id: String,
    /// `assignment[i]` is the index into the photo slice for slot `i`, in
    /// left-page-then-right-page slot order.
    pub assignment: Vec<usize>,
    pub score: f64,
}

/// A slot's real-world aspect ratio on the PAGE canvas.
///
/// The trap: `rect` is normalised to the page (11.197" x 8.894" = 1.259:1),
/// so `w / h` is NOT the ratio `aspect_pref` is expressed in. Comparing
/// against the normalised ratio mis-scores every slot.
pub fn slot_aspect(slot: &Slot) -> f64 {
    slot.rect.aspect_in(PAGE_W_IN, PAGE_H_IN)
}

/// Pixels per inch the photo actually resolves at, once cropped, when placed
/// in this slot.
pub fn effective_dpi(photo: &Photo, crop: &Rect, slot: &Slot) -> f64 {
    let cropped_px = photo.width as f64 * crop.w;
    let slot_in = slot.rect.w * PAGE_W_IN;
    if slot_in <= 0.0 {
        return 0.0;
    }
    cropped_px / slot_in
}

/// Maps a face box from photo coordinates into the slot's page coordinates,
/// given the crop window. Returns `None` when the crop window is degenerate.
fn face_in_page(face: &Rect, crop: &Rect, slot: &Slot) -> Option<Rect> {
    if crop.w <= 0.0 || crop.h <= 0.0 {
        return None;
    }
    let u = (face.x - crop.x) / crop.w;
    let v = (face.y - crop.y) / crop.h;
    let uw = face.w / crop.w;
    let vh = face.h / crop.h;
    Some(Rect::new(
        slot.rect.x + u * slot.rect.w,
        slot.rect.y + v * slot.rect.h,
        uw * slot.rect.w,
        vh * slot.rect.h,
    ))
}

/// The three hard constraints. Returns the first violation, or `None` when
/// the placement is acceptable.
pub fn rejects(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> Option<Rejection> {
    if effective_dpi(photo, crop, slot) < MIN_DPI {
        return Some(Rejection::TooLowResolution);
    }

    for face in &photo.faces {
        // Clipped: the crop window does not fully contain the face box.
        let contained = face.box_.x >= crop.x - 1e-9
            && face.box_.y >= crop.y - 1e-9
            && face.box_.right() <= crop.right() + 1e-9
            && face.box_.bottom() <= crop.bottom() + 1e-9;
        if !contained {
            // A face wholly outside the crop is not "clipped" -- it is
            // simply not in the picture, which is fine. Only a PARTIAL
            // overlap is a half-face.
            if face.box_.intersect(crop).is_some() {
                return Some(Rejection::FaceClipped);
            }
            continue;
        }

        if let Some(page_rect) = face_in_page(&face.box_, crop, slot) {
            if !clear_of_gutter(&page_rect, side) {
                return Some(Rejection::FaceInGutter);
            }
            if !in_safe_margin(&page_rect, side) {
                return Some(Rejection::FaceInSafeMargin);
            }
        }
    }

    None
}

/// How well a photo's aspect matches a slot, in [0,1]. 1.0 when the photo
/// needs no crop at all; falls off with the fraction of the frame discarded.
fn aspect_fit(photo: &Photo, slot: &Slot) -> f64 {
    let target = slot_aspect(slot);
    let actual = photo.aspect();
    let ratio = if target > actual { actual / target } else { target / actual };
    ratio.clamp(0.0, 1.0)
}

/// Fraction of the saliency box surviving the crop, in [0,1]. A photo with
/// no saliency box scores neutrally rather than zero -- absence of a signal
/// is not evidence of a bad crop.
fn saliency_retention(photo: &Photo, crop: &Rect) -> f64 {
    match photo.saliency_box {
        None => 0.5,
        Some(s) => {
            let area = s.area();
            if area <= 0.0 {
                return 0.5;
            }
            s.intersect(crop).map_or(0.0, |i| i.area()) / area
        }
    }
}

/// Fraction of total face area surviving the crop. Neutral when faceless.
fn face_area_retention(photo: &Photo, crop: &Rect) -> f64 {
    let total: f64 = photo.faces.iter().map(|f| f.box_.area()).sum();
    if total <= 0.0 {
        return 0.5;
    }
    let kept: f64 = photo
        .faces
        .iter()
        .map(|f| f.box_.intersect(crop).map_or(0.0, |i| i.area()))
        .sum();
    kept / total
}

/// Vision's own capture-quality score for the best face in the photo:
/// blur, exposure and pose, in [0,1].
///
/// This is the closest thing the engine has to an EXPRESSION signal, and
/// the distance is worth stating. Vision has no expression classifier at
/// any macOS version, and the geometric smile proxy was measured against
/// real faces with a 100% false-negative rate, so it is computed and
/// deliberately unused. Capture quality does not know a smile from a
/// grimace -- it knows a sharp, well-exposed, front-facing face from a
/// blurred or turned one, which is a different thing that happens to
/// correlate with the complaint.
///
/// Neutral at 0.5 when there is no face or Vision returned no score: a
/// faceless photo is not a bad photo. Same convention as
/// `saliency_retention` and `face_area_retention`.
fn face_quality(photo: &Photo) -> f64 {
    photo.capture_quality.map_or(0.5, |q| q.clamp(0.0, 1.0))
}

/// Penalty for generic salient content falling in the gutter dead band, in
/// [0,1] where 1.0 is "none of it is in the crease".
///
/// The spec's split, and the reason this is not a rejection: a FACE in the
/// dead strip is a hard rejection, but generic salient content there is
/// only a penalty -- otherwise no slot could ever run flush to the fold and
/// half the library would be unusable.
///
/// Only the part of the saliency box that SURVIVES the crop is measured.
/// Unlike a face, a saliency box is never a hard constraint, so it can be
/// legitimately partial, and judging the whole declared box would penalise
/// a crop that already removed the offending part. Pre-flight's own
/// gutter-saliency WARN makes the same choice.
///
/// Neutral at 1.0 with no saliency box: absence of a signal is not evidence
/// of a bad placement.
fn gutter_saliency(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> f64 {
    let Some(visible) = photo.saliency_box.and_then(|s| s.intersect(crop)) else {
        return 1.0;
    };
    let Some(mapped) = face_in_page(&visible, crop, slot) else {
        return 1.0;
    };
    let area = mapped.area();
    if area <= 0.0 {
        return 1.0;
    }
    // Graded, not binary: half a saliency box in the crease is half as bad
    // as all of it. `clear_of_gutter` answers yes/no, which is the right
    // shape for a rejection and the wrong one for a penalty.
    let in_band = gutter_overlap_area(&mapped, side);
    (1.0 - (in_band / area)).clamp(0.0, 1.0)
}

/// Rewards the highest-aesthetic photo landing in a `hero` slot.
fn hero_match(photo: &Photo, slot: &Slot, best_aesthetic: u8) -> f64 {
    match slot.role {
        Role::Hero => {
            if best_aesthetic == 0 {
                0.5
            } else {
                photo.aesthetic_pct as f64 / best_aesthetic as f64
            }
        }
        Role::Support => 0.5,
    }
}

/// Headroom above the hard floor, saturating at the 300 DPI target.
fn resolution_headroom(photo: &Photo, crop: &Rect, slot: &Slot) -> f64 {
    let dpi = effective_dpi(photo, crop, slot);
    ((dpi - MIN_DPI) / (300.0 - MIN_DPI)).clamp(0.0, 1.0)
}

/// Rewards a spread whose photos share a coherent dominant hue: a spread
/// whose photos scatter across the hue circle reads as noisy.
///
/// This is a CRUDE hue proxy, not a perceptual one. Phase 1's palette is
/// plain sRGB in [0,1] -- `Metrics.palette` in the sidecar averages 8-bit
/// channels and divides by 255, it is not Oklab -- so `atan2(b, r)` ignores
/// `g` entirely, maps every neutral grey from near-black to near-white onto
/// the same angle, and, because `r` and `b` are both non-negative, spans
/// only a quarter turn. Two photos therefore cannot score below
/// `cos(pi/4) ~= 0.707`, so the term's whole reachable range is about 0.29
/// wide, not 1.0.
///
/// Hence the deliberately low default weight: it is the fuzziest term in
/// the scorer, it is uncalibrated, and zeroing its weight must leave a
/// usable book. Recalibrating it in a real hue space is deferred.
fn palette_harmony(photos: &[&Photo]) -> f64 {
    let hues: Vec<f64> = photos
        .iter()
        .filter_map(|p| p.palette.first())
        .map(|c| c.b.atan2(c.r))
        .collect();
    if hues.len() < 2 {
        return 0.5;
    }
    // Circular variance: 1.0 when all hues agree, 0.0 when uniformly spread.
    let (sx, sy) = hues.iter().fold((0.0, 0.0), |(x, y), h| (x + h.cos(), y + h.sin()));
    let n = hues.len() as f64;
    ((sx / n).hypot(sy / n)).clamp(0.0, 1.0)
}

/// How unlike each other the photos on one spread are, in [0,1].
///
/// The complaint this answers: "there are too many images that are similar
/// in some pages". Near-duplicate clustering is perceptual-hash based, so it
/// only collapses near-IDENTICAL frames; two photos of the same moment from
/// slightly different angles are not near-duplicates and both survive.
///
/// Three components, each normalised to [0,1] and combined with EQUAL
/// weight. Equal weighting is a starting point, not a claim -- the three are
/// not commensurable and no measurement exists to weight them against each
/// other. Sub-weights are a tuning question; if one component turns out to
/// dominate, the fix is to measure it, not to guess a ratio.
///
/// `phash` would be the most direct signal and is deliberately absent: it is
/// stripped before the webview because JavaScript loses precision above
/// 2^53, and re-widening the wire is a larger change than this term
/// justifies.
///
/// Neutral at 0.5 for fewer than two photos: one photo on a spread is not
/// "undiverse", the question does not apply.
fn spread_diversity(photos: &[&Photo]) -> f64 {
    if photos.len() < 2 {
        return 0.5;
    }
    let mut total = 0.0;
    let mut pairs = 0.0;
    for i in 0..photos.len() {
        for j in (i + 1)..photos.len() {
            total += pair_distance(photos[i], photos[j]);
            pairs += 1.0;
        }
    }
    if pairs == 0.0 {
        return 0.5;
    }
    (total / pairs).clamp(0.0, 1.0)
}

/// Mean of the three pairwise distances, each in [0,1].
fn pair_distance(a: &Photo, b: &Photo) -> f64 {
    (scene_tag_distance(a, b) + palette_distance(a, b) + capture_gap_distance(a, b)) / 3.0
}

/// Jaccard distance over scene tags. Two photos with no tags at all are
/// neither similar nor different on this axis, so they score neutrally
/// rather than identical -- an empty-vs-empty comparison is an absent
/// signal, not agreement.
fn scene_tag_distance(a: &Photo, b: &Photo) -> f64 {
    use std::collections::BTreeSet;
    let sa: BTreeSet<&str> = a.scene_tags.iter().map(String::as_str).collect();
    let sb: BTreeSet<&str> = b.scene_tags.iter().map(String::as_str).collect();
    if sa.is_empty() && sb.is_empty() {
        return 0.5;
    }
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 {
        return 0.5;
    }
    1.0 - (sa.intersection(&sb).count() as f64 / union)
}

/// Euclidean distance between the dominant colours, normalised by the
/// longest possible distance in the unit RGB cube (sqrt(3)).
///
/// Plain sRGB, matching what `Metrics.palette` actually produces -- the
/// same crude space `palette_harmony` reads, and for the same reason: this
/// is not a perceptual distance and does not claim to be.
fn palette_distance(a: &Photo, b: &Photo) -> f64 {
    let (Some(ca), Some(cb)) = (a.palette.first(), b.palette.first()) else {
        return 0.5;
    };
    let d = ((ca.r - cb.r).powi(2) + (ca.g - cb.g).powi(2) + (ca.b - cb.b).powi(2)).sqrt();
    (d / 3.0_f64.sqrt()).clamp(0.0, 1.0)
}

/// Capture-time gap, saturating at one hour: photos minutes apart are the
/// same moment, photos hours apart are not. Beyond an hour the axis carries
/// no more information, and event clustering has already separated the
/// chapters.
fn capture_gap_distance(a: &Photo, b: &Photo) -> f64 {
    const SATURATE_SECONDS: f64 = 3600.0;
    let (Some(ta), Some(tb)) = (a.captured_at, b.captured_at) else {
        return 0.5;
    };
    let gap = (ta - tb).abs() as f64;
    (gap / SATURATE_SECONDS).clamp(0.0, 1.0)
}

/// How much room the template gives its hero slot, scaled by how much this
/// group has a standout photo to put there. In [0,1].
///
/// The complaint: "some interesting features should be highlighted rather
/// than mixed with other images". `hero_match` already rewards the best
/// photo landing in a hero slot, but only WITHIN a group that has already
/// been formed -- `pack` knows nothing about merit, so a standout can be
/// dealt into a six-up and get a sixth of a spread.
///
/// This biases which template a formed group gets. It does NOT change how
/// groups are formed; letting merit influence group size is a real change to
/// `pack`'s contract and is deliberately deferred until this has been
/// measured against a real book.
///
/// Neutral at 0.5 when the group is flat, so the term is silent rather than
/// pushing toward big slots generally: it exists to give a standout room,
/// not to prefer dominant templates as a matter of taste.
fn hero_prominence(t: &SpreadTemplate, photos: &[&Photo]) -> f64 {
    if photos.len() < 2 {
        return 0.5;
    }
    let mut pcts: Vec<f64> = photos.iter().map(|p| p.aesthetic_pct as f64).collect();
    pcts.sort_by(|a, b| b.total_cmp(a));
    let rest = &pcts[1..];
    let rest_mean = rest.iter().sum::<f64>() / rest.len() as f64;
    // How far the best photo clears the others, as a fraction of the
    // percentile scale. Zero when the group is flat.
    let standout = ((pcts[0] - rest_mean) / 100.0).clamp(0.0, 1.0);
    if standout == 0.0 {
        return 0.5;
    }

    let slots: Vec<&Slot> =
        t.left.slots.iter().chain(t.right.slots.iter()).collect();
    let total: f64 = slots.iter().map(|s| s.rect.area()).sum();
    if total <= 0.0 {
        return 0.5;
    }
    let hero_area: f64 = slots
        .iter()
        .filter(|s| s.role == Role::Hero)
        .map(|s| s.rect.area())
        .sum();
    // An even split of n slots gives each 1/n; anything above that is
    // genuine dominance. Normalised so a single full-spread hero reads 1.0.
    let even_share = 1.0 / slots.len() as f64;
    let dominance = ((hero_area / total - even_share) / (1.0 - even_share)).clamp(0.0, 1.0);

    (0.5 + 0.5 * standout * dominance).clamp(0.0, 1.0)
}

/// Penalises repeating the immediately preceding template.
fn variety(template_id: &str, previous: Option<&str>) -> f64 {
    match previous {
        Some(prev) if prev == template_id => 0.0,
        _ => 1.0,
    }
}

fn ordered_slots(t: &SpreadTemplate) -> Vec<(&Slot, Side)> {
    t.left
        .slots
        .iter()
        .map(|s| (s, Side::Left))
        .chain(t.right.slots.iter().map(|s| (s, Side::Right)))
        .collect()
}

/// Scores one candidate. Returns `None` when any hard constraint rejects it.
pub fn score_spread(
    t: &SpreadTemplate,
    photos: &[&Photo],
    assignment: &[usize],
    previous: Option<&str>,
    w: &Weights,
) -> Option<f64> {
    let slots = ordered_slots(t);
    if slots.len() != assignment.len() || assignment.len() != photos.len() {
        return None;
    }

    let best_aesthetic = photos.iter().map(|p| p.aesthetic_pct).max().unwrap_or(0);
    let mut total = 0.0;

    for (i, (slot, side)) in slots.iter().enumerate() {
        let photo = photos[assignment[i]];
        let crop = choose_crop(photo, slot_aspect(slot));
        if rejects(photo, &crop, slot, *side).is_some() {
            return None;
        }
        total += w.aspect_fit * aspect_fit(photo, slot)
            + w.saliency_retention * saliency_retention(photo, &crop)
            + w.face_area_retention * face_area_retention(photo, &crop)
            + w.face_quality * face_quality(photo)
            + w.hero_match * hero_match(photo, slot, best_aesthetic)
            + w.resolution_headroom * resolution_headroom(photo, &crop, slot)
            + w.gutter_saliency * gutter_saliency(photo, &crop, slot, *side);
    }

    // Spread-global terms, added once rather than per slot.
    total += w.palette_harmony * palette_harmony(photos);
    total += w.spread_diversity * spread_diversity(photos);
    total += w.hero_prominence * hero_prominence(t, photos);
    total += w.variety * variety(&t.id, previous);

    Some(total)
}

/// Enumerates every eligible template and every assignment, returning the
/// best. Brute force: at most 6! = 720 assignments over box arithmetic with
/// no pixels touched.
///
/// Exact ties are broken by the SEED, the same way `pace::best_single` does
/// it. Breaking them on template id instead was deterministic but seed-blind,
/// which left "regenerate this spread advances the seed" unimplementable for
/// every middle spread: advancing the seed changed nothing.
///
/// Exact equality is the definition of the tie the seed exists to break;
/// anything looser would let the seed override a real preference.
pub fn best_spread<'a>(
    templates: &[&'a SpreadTemplate],
    photos: &[&Photo],
    previous: Option<&str>,
    w: &Weights,
    seed: u64,
) -> Option<(&'a SpreadTemplate, Vec<usize>, f64)> {
    let mut best = f64::NEG_INFINITY;
    let mut tied: Vec<(&SpreadTemplate, Vec<usize>)> = Vec::new();

    for t in templates {
        if t.photo_count() != photos.len() {
            continue;
        }
        for assignment in permutations(photos.len()) {
            let Some(score) = score_spread(t, photos, &assignment, previous, w) else {
                continue;
            };
            if score > best {
                best = score;
                tied.clear();
                tied.push((t, assignment));
            } else if score == best {
                tied.push((t, assignment));
            }
        }
    }

    if tied.is_empty() {
        return None;
    }
    // Sorted by template id before indexing, so the candidate ORDER is a
    // total order over the inputs rather than enumeration order -- the seed
    // picks among candidates, it does not depend on how they were found.
    tied.sort_by(|(a, ai), (b, bi)| a.id.cmp(&b.id).then(ai.cmp(bi)));
    let pick = crate::book::pace::tie_break(seed, tied.len());
    let (template, assignment) = tied.swap_remove(pick);
    Some((template, assignment, best))
}

/// All permutations of `0..n`, in lexicographic order so enumeration is
/// deterministic.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut used = vec![false; n];
    let mut buf = Vec::with_capacity(n);
    fn go(n: usize, used: &mut Vec<bool>, buf: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if buf.len() == n {
            out.push(buf.clone());
            return;
        }
        for i in 0..n {
            if used[i] {
                continue;
            }
            used[i] = true;
            buf.push(i);
            go(n, used, buf, out);
            buf.pop();
            used[i] = false;
        }
    }
    go(n, &mut used, &mut buf, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, PaletteColor};
    use crate::geometry::BleedEdge;
    use crate::templates::{Density, EdgeTreatment, Energy, PageLayout};

    fn photo(w: u32, h: u32) -> Photo {
        Photo {
            path: "/p.jpg".into(), hash: "h".into(), width: w, height: h,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
            scene_tags: Vec::new(), captured_at: None,
        }
    }

    /// A slot whose normalised ratio and inch ratio are DIFFERENT numbers.
    /// A fixture where they coincide passes under the aspect_pref trap.
    fn wide_slot() -> Slot {
        Slot {
            // On an 11.197 x 8.894 page: 0.5 x 0.5 normalised is
            // 5.5985" x 4.447" = 1.259 real, not 1.0.
            rect: Rect::new(0.2, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        }
    }

    /// A slot exactly 6.0" wide. `6.0 / PAGE_W_IN` round-trips back through
    /// `* PAGE_W_IN` to exactly 6.0 in IEEE doubles, so `effective_dpi` can
    /// return exactly 200.0 for an integer pixel count -- the only way to
    /// test the floor's inclusivity at all.
    fn six_inch_slot() -> Slot {
        Slot {
            rect: Rect::new(0.1, 0.2, 6.0 / PAGE_W_IN, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.6),
        }
    }

    #[test]
    fn score_slot_aspect_uses_page_inches_not_the_normalised_ratio() {
        let s = wide_slot();
        let normalised = s.rect.w / s.rect.h;
        let real = slot_aspect(&s);
        assert!((normalised - 1.0).abs() < 1e-9, "sanity: normalised is 1:1");
        assert!((real - 1.259).abs() < 0.002, "real was {real}");
    }

    #[test]
    fn score_effective_dpi_accounts_for_the_crop() {
        let p = photo(4000, 3000);
        let s = wide_slot(); // 5.5985" wide
        let full = Rect::new(0.0, 0.0, 1.0, 1.0);
        let half = Rect::new(0.25, 0.0, 0.5, 1.0);
        let d_full = effective_dpi(&p, &full, &s);
        let d_half = effective_dpi(&p, &half, &s);
        assert!((d_full - 4000.0 / 5.5985).abs() < 1.0, "was {d_full}");
        assert!((d_half - d_full / 2.0).abs() < 1.0, "cropping halves the DPI");
    }

    #[test]
    fn score_rejects_a_photo_below_the_dpi_floor() {
        let p = photo(400, 300);
        let s = wide_slot();
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::TooLowResolution)
        ));
    }

    /// The floor pinned on BOTH sides, on a slot that can express it exactly.
    ///
    /// `wide_slot` cannot: 5.5985" needs 1119.7 px for 200 DPI, which rounds
    /// to 1120 px = 200.0357 DPI -- never on the boundary it was named for,
    /// so `< MIN_DPI` could be flipped to `<= MIN_DPI` with the whole suite
    /// still green. A slot exactly 6.0" wide has an integer answer: 1200 px
    /// is exactly 200.0 DPI (verified in the first assertion, since only
    /// exact IEEE equality distinguishes the two comparisons), and one pixel
    /// less is 199.83.
    #[test]
    fn score_dpi_floor_admits_exactly_min_dpi_and_rejects_one_pixel_below() {
        let s = six_inch_slot();
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);

        let at = photo(1200, 900);
        assert_eq!(
            effective_dpi(&at, &crop, &s),
            MIN_DPI,
            "the fixture must land ON the floor, not merely near it"
        );
        assert!(rejects(&at, &crop, &s, Side::Left).is_none(), "the floor itself must pass");

        let below = photo(1199, 899);
        assert!(matches!(
            rejects(&below, &crop, &s, Side::Left),
            Some(Rejection::TooLowResolution)
        ));
    }

    /// The face must STRADDLE the crop edge. The brief's fixture put it at
    /// x 0.85..0.97 against a crop ending at 0.70 -- wholly outside, which
    /// is explicitly not a clip, so that fixture asserted the opposite of
    /// the rule it was written to guard. Here the box spans 0.65..0.77 and
    /// the crop edge at 0.70 cuts it in half.
    #[test]
    fn score_rejects_a_crop_that_clips_a_face() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.65, 0.4, 0.12, 0.2), capture_quality: Some(0.8) }];
        let crop = Rect::new(0.0, 0.0, 0.7, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &wide_slot(), Side::Left),
            Some(Rejection::FaceClipped)
        ));
    }

    /// The other half of the same rule: a face the crop misses entirely is
    /// simply not in the picture, which is fine. Rejecting it would forbid
    /// every crop that tightens onto a subject with bystanders in frame.
    #[test]
    fn score_does_not_reject_a_face_wholly_outside_the_crop() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.85, 0.4, 0.12, 0.2), capture_quality: Some(0.8) }];
        let crop = Rect::new(0.0, 0.0, 0.7, 1.0);
        assert!(rejects(&p, &crop, &wide_slot(), Side::Left).is_none());
    }

    #[test]
    fn score_accepts_a_crop_that_fully_contains_the_face() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.30, 0.4, 0.12, 0.2), capture_quality: Some(0.8) }];
        let crop = Rect::new(0.0, 0.0, 0.7, 1.0);
        assert!(rejects(&p, &crop, &wide_slot(), Side::Left).is_none());
    }

    #[test]
    fn score_rejects_a_face_landing_in_the_gutter_strip() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.90, 0.4, 0.08, 0.2), capture_quality: Some(0.8) }];
        // A slot running flush to the fold on a LEFT page.
        let s = Slot {
            rect: Rect::new(0.5, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::FaceInGutter)
        ));
    }

    /// The safe-margin hard constraint. The face sits INSIDE the trim
    /// rectangle -- `in_trim` on the mapped rect is asserted true as a
    /// sanity check -- but inside the additional 1/8" Pixajoy buffer beyond
    /// it, so only `in_safe_margin` catches it. This is what distinguishes
    /// the new check from the pre-existing trim/gutter ones: a fixture that
    /// merely sat outside trim entirely would pass under a scorer with no
    /// safe-margin check at all, since nothing else in `rejects` looks at
    /// `in_trim`.
    #[test]
    fn score_rejects_a_face_inside_trim_but_inside_the_safe_margin_band() {
        use crate::geometry::in_trim;

        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.044, 0.4, 0.02, 0.02), capture_quality: Some(0.8) }];
        // Flush to the outer-left edge, far from the fold, so gutter cannot
        // fire and only the new outer-edge safe-margin inset is in play.
        let s = Slot {
            rect: Rect::new(0.0, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: vec![BleedEdge::Left, BleedEdge::Top, BleedEdge::Bottom],
            aspect_pref: (1.0, 1.4),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);

        // Sanity: prove the fixture is inside trim before asserting it is
        // rejected on safe-margin grounds specifically.
        let mapped = Rect::new(0.022, 0.4, 0.01, 0.01);
        assert!(in_trim(&mapped, Side::Left), "fixture must sit inside trim");

        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::FaceInSafeMargin)
        ));
    }

    /// The counterpart: a face comfortably clear of the outer edge, top and
    /// bottom by more than 1/8" must not be rejected.
    #[test]
    fn score_accepts_a_face_comfortably_inside_the_safe_margin() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.3, 0.3, 0.1, 0.1), capture_quality: Some(0.8) }];
        let s = Slot {
            rect: Rect::new(0.0, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.0, 1.4),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(rejects(&p, &crop, &s, Side::Left).is_none());
    }

    /// Order matters when a face violates BOTH constraints at once. Placed
    /// near the fold AND near the top edge, the face fails `clear_of_gutter`
    /// (fold proximity) and would ALSO fail the new safe-margin check (top
    /// proximity) if reached. The pre-existing gutter rejection must win --
    /// `rejects` checks it first, so a genuinely ambiguous face reports the
    /// established diagnosis rather than being silently reclassified by the
    /// new check. Mirrors the trim-vs-safe-margin ordering pre-flight now
    /// enforces between its own two findings.
    #[test]
    fn score_gutter_rejection_takes_priority_over_safe_margin_when_both_would_fire() {
        let mut p = photo(4000, 3000);
        // Near the fold (x) AND near the top edge (y) on a left page slot
        // that is flush to both.
        p.faces = vec![Face { box_: Rect::new(0.90, 0.02, 0.08, 0.05), capture_quality: Some(0.8) }];
        let s = Slot {
            rect: Rect::new(0.5, 0.0, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::FaceInGutter)
        ));
    }

    /// Generic salient content in the dead strip is only a PENALTY, never a
    /// rejection -- otherwise no slot could ever run to the fold, which is
    /// the layout the user explicitly asked to keep available.
    #[test]
    fn score_does_not_reject_generic_saliency_in_the_gutter_strip() {
        let mut p = photo(4000, 3000);
        p.saliency_box = Some(Rect::new(0.90, 0.4, 0.08, 0.2));
        let s = Slot {
            rect: Rect::new(0.5, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(rejects(&p, &crop, &s, Side::Left).is_none());
    }

    /// The pixel dimensions are load-bearing, not incidental. The brief's
    /// fixture used 4000x2000 and 2000x4000: the tall photo in the wide hero
    /// slot then resolves at 223 DPI, so `resolution_headroom` ALSO ranks
    /// the matched assignment above the swapped one, and the test kept
    /// passing with `aspect_fit`'s weight zeroed -- verified, not assumed.
    /// At 6000x3000 and 3000x6000 every photo/slot pairing clears 300 DPI
    /// and that term clamps to 1.0 everywhere, so `aspect_fit` is the only
    /// term that differs between the two assignments.
    #[test]
    fn score_prefers_the_assignment_matching_slot_aspects() {
        let lib = fixture_library();
        let t = &lib[0]; // hero slot is wide, support slot is narrow
        let wide = photo(6000, 3000);
        let tall = photo(3000, 6000);
        let photos = vec![&wide, &tall];

        let matched = score_spread(t, &photos, &[0, 1], None, &Weights::default());
        let swapped = score_spread(t, &photos, &[1, 0], None, &Weights::default());
        assert!(matched.unwrap() > swapped.unwrap(), "aspect fit must drive the choice");
    }

    #[test]
    fn score_puts_the_highest_aesthetic_photo_in_the_hero_slot() {
        let lib = fixture_library();
        let t = &lib[0];
        let mut a = photo(3000, 2000);
        a.aesthetic_pct = 95;
        let mut b = photo(3000, 2000);
        b.aesthetic_pct = 10;
        let photos = vec![&a, &b];
        let hero_first = score_spread(t, &photos, &[0, 1], None, &Weights::default()).unwrap();
        let hero_last = score_spread(t, &photos, &[1, 0], None, &Weights::default()).unwrap();
        assert!(hero_first > hero_last);
    }

    #[test]
    fn score_penalises_reusing_the_previous_template() {
        let lib = fixture_library();
        let t = &lib[0];
        let p = photo(3000, 2000);
        let photos = vec![&p, &p];
        let fresh = score_spread(t, &photos, &[0, 1], None, &Weights::default()).unwrap();
        let repeat =
            score_spread(t, &photos, &[0, 1], Some(&t.id), &Weights::default()).unwrap();
        assert!(fresh > repeat, "variety must penalise an immediate repeat");
    }

    /// Three photos that share every signal must score LOWER than three that
    /// share none. The fixture varies all three components at once, so the
    /// assertion holds regardless of how they are weighted against each other.
    #[test]
    fn score_spread_diversity_separates_a_varied_spread_from_a_samey_one() {
        let mut same: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
        for p in &mut same {
            p.scene_tags = vec!["beach".into(), "sunset".into()];
            p.captured_at = Some(1_700_000_000);
            p.palette = vec![PaletteColor { r: 0.9, g: 0.4, b: 0.1, weight: 1.0 }];
        }

        let mut varied: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
        varied[0].scene_tags = vec!["beach".into()];
        varied[1].scene_tags = vec!["forest".into()];
        varied[2].scene_tags = vec!["city".into()];
        varied[0].captured_at = Some(1_700_000_000);
        varied[1].captured_at = Some(1_700_050_000);
        varied[2].captured_at = Some(1_700_100_000);
        varied[0].palette = vec![PaletteColor { r: 0.9, g: 0.1, b: 0.1, weight: 1.0 }];
        varied[1].palette = vec![PaletteColor { r: 0.1, g: 0.9, b: 0.1, weight: 1.0 }];
        varied[2].palette = vec![PaletteColor { r: 0.1, g: 0.1, b: 0.9, weight: 1.0 }];

        let same_refs: Vec<&Photo> = same.iter().collect();
        let varied_refs: Vec<&Photo> = varied.iter().collect();

        let s = spread_diversity(&same_refs);
        let v = spread_diversity(&varied_refs);
        assert!(v > s, "a varied spread must outscore a samey one: {v} vs {s}");
        assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&v));
    }

    /// Fewer than two photos, or no signal at all, is neutral rather than zero
    /// -- a single photo on a spread is not "undiverse", the question simply
    /// does not apply.
    #[test]
    fn score_spread_diversity_is_neutral_without_enough_to_compare() {
        let one = photo(4000, 3000);
        assert_eq!(spread_diversity(&[&one]), 0.5);
        assert_eq!(spread_diversity(&[]), 0.5);
    }

    /// The term must reach the spread total. Shipped weight is 0.0, so a test
    /// against the shipped weights would pass with the body deleted.
    #[test]
    fn score_spread_diversity_changes_the_spread_total_when_weighted() {
        let t = two_slot_template();
        let mut a = photo(4000, 3000);
        let mut b = photo(4000, 3000);
        a.scene_tags = vec!["beach".into()];
        b.scene_tags = vec!["beach".into()];
        a.captured_at = Some(1_700_000_000);
        b.captured_at = Some(1_700_000_000);

        let mut c = photo(4000, 3000);
        let mut d = photo(4000, 3000);
        c.scene_tags = vec!["beach".into()];
        d.scene_tags = vec!["city".into()];
        c.captured_at = Some(1_700_000_000);
        d.captured_at = Some(1_700_100_000);

        let w = Weights { spread_diversity: 1.0, ..Weights::default() };
        let samey = score_spread(&t, &[&a, &b], &[0, 1], None, &w).expect("scores");
        let varied = score_spread(&t, &[&c, &d], &[0, 1], None, &w).expect("scores");
        assert!(varied > samey, "spread_diversity never reached the total");

        let inert = Weights::default();
        assert_eq!(
            score_spread(&t, &[&a, &b], &[0, 1], None, &inert),
            score_spread(&t, &[&c, &d], &[0, 1], None, &inert),
            "the term must be inert at its shipped weight of 0.0"
        );
    }

    /// When a group holds a standout photo, a template whose hero slot dominates
    /// the spread should beat one whose slots are all the same size. When the
    /// group is flat, the two should be indistinguishable -- the term is about
    /// giving a STANDOUT room, not about preferring big slots generally.
    #[test]
    fn score_hero_prominence_prefers_a_dominant_hero_only_for_a_standout_group() {
        let dominant = hero_dominant_template();
        let even = even_three_up_template();

        let mut standout: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
        standout[0].aesthetic_pct = 99;
        standout[1].aesthetic_pct = 40;
        standout[2].aesthetic_pct = 38;

        let flat: Vec<Photo> = (0..3)
            .map(|_| {
                let mut p = photo(4000, 3000);
                p.aesthetic_pct = 50;
                p
            })
            .collect();

        let standout_refs: Vec<&Photo> = standout.iter().collect();
        let flat_refs: Vec<&Photo> = flat.iter().collect();

        assert!(
            hero_prominence(&dominant, &standout_refs) > hero_prominence(&even, &standout_refs),
            "a standout photo should pull toward a dominant hero slot"
        );
        assert_eq!(
            hero_prominence(&dominant, &flat_refs),
            hero_prominence(&even, &flat_refs),
            "with no standout, the term must not prefer either template"
        );
    }

    /// The term must reach the total; shipped weight is 0.0.
    #[test]
    fn score_hero_prominence_changes_the_spread_total_when_weighted() {
        let dominant = hero_dominant_template();
        let even = even_three_up_template();
        let mut standout: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
        standout[0].aesthetic_pct = 99;
        standout[1].aesthetic_pct = 40;
        standout[2].aesthetic_pct = 38;
        let refs: Vec<&Photo> = standout.iter().collect();

        let w = Weights { hero_prominence: 1.0, ..Weights::default() };
        let d = score_spread(&dominant, &refs, &[0, 1, 2], None, &w).expect("scores");
        let e = score_spread(&even, &refs, &[0, 1, 2], None, &w).expect("scores");
        assert!(d > e, "hero_prominence never reached the total: {d} vs {e}");

        // Inert at the shipped weight -- but hero_match still separates these
        // two templates, so compare the DELTA rather than asserting equality.
        let inert = Weights::default();
        let id = score_spread(&dominant, &refs, &[0, 1, 2], None, &inert).expect("scores");
        let ie = score_spread(&even, &refs, &[0, 1, 2], None, &inert).expect("scores");
        assert!(
            (d - e) > (id - ie),
            "weighting the term must widen the gap it is responsible for"
        );
    }

    // --- the four soft terms the brief left with no behavioural test.
    //
    // Each pairs two candidates that differ in exactly ONE term, so zeroing
    // that term's weight collapses the two scores to the same number and the
    // strict `>` fails. Anything that merely *correlates* with the term
    // would keep these passing under the mutation, which is how the brief's
    // `aspect_fit` fixture slipped through.

    /// `saliency_retention`. Both saliency boxes are centred on (0.5, 0.5),
    /// so `choose_crop` -- which positions on the focus centroid -- returns
    /// the IDENTICAL window for both photos. That is what makes the pair a
    /// clean isolation: every other term reads the same crop, the same
    /// pixels and the same aspect. Only the box's extent differs, and the
    /// tall one cannot survive a crop band 0.463 high.
    #[test]
    fn score_rewards_a_crop_that_keeps_the_salient_region() {
        let t = single_hero_template();
        let target = slot_aspect(&t.left.slots[0]);

        let mut kept = photo(4000, 3000);
        kept.saliency_box = Some(Rect::new(0.4, 0.4, 0.2, 0.2));
        let mut cut = photo(4000, 3000);
        cut.saliency_box = Some(Rect::new(0.4, 0.0, 0.2, 1.0));

        assert_eq!(
            choose_crop(&kept, target),
            choose_crop(&cut, target),
            "the fixture only isolates saliency_retention if the crops match"
        );

        let a = score_spread(&t, &[&kept], &[0], None, &Weights::default()).unwrap();
        let b = score_spread(&t, &[&cut], &[0], None, &Weights::default()).unwrap();
        assert!(a > b, "keeping the salient region must score higher: {a} vs {b}");
    }

    /// `face_area_retention`. A face PARTIALLY out of the crop is a hard
    /// rejection, so the only way this term can legitimately vary is a face
    /// the crop misses entirely -- it contributes to the denominator and
    /// nothing to the numerator. The second face is deliberately tiny and
    /// low-quality so it barely shifts the crop centroid and stays wholly
    /// clear of the band; if it crept into the band the candidate would be
    /// rejected outright and the test would fail loudly rather than
    /// silently degrade. Crop WIDTH is 1.0 for both, so
    /// `resolution_headroom` cannot move; there is no saliency box, so that
    /// term sits at its neutral 0.5 for both.
    #[test]
    fn score_rewards_a_crop_that_keeps_the_faces() {
        let t = single_hero_template();

        let mut kept = photo(4000, 3000);
        kept.faces =
            vec![Face { box_: Rect::new(0.4, 0.45, 0.1, 0.1), capture_quality: Some(0.8) }];

        let mut lost = photo(4000, 3000);
        lost.faces = vec![
            Face { box_: Rect::new(0.4, 0.45, 0.1, 0.1), capture_quality: Some(0.8) },
            Face { box_: Rect::new(0.4, 0.95, 0.04, 0.04), capture_quality: Some(0.1) },
        ];

        let a = score_spread(&t, &[&kept], &[0], None, &Weights::default()).unwrap();
        let b = score_spread(&t, &[&lost], &[0], None, &Weights::default()).unwrap();
        assert!(a > b, "dropping a face out of frame must cost: {a} vs {b}");
    }

    /// `resolution_headroom`. Same aspect, same everything, different pixel
    /// counts: 6000 px clears the 300 DPI target in this slot and saturates
    /// the term, 2000 px resolves at 223 DPI -- above the hard floor, so it
    /// is not rejected, merely worse.
    #[test]
    fn score_prefers_the_photo_with_more_pixels_for_the_same_slot() {
        let t = single_hero_template();
        let big = photo(6000, 4500);
        let small = photo(2000, 1500);
        assert_eq!(big.aspect(), small.aspect(), "aspect_fit must not move");

        let a = score_spread(&t, &[&big], &[0], None, &Weights::default()).unwrap();
        let b = score_spread(&t, &[&small], &[0], None, &Weights::default()).unwrap();
        assert!(a > b, "resolution headroom must break the tie: {a} vs {b}");
    }

    /// `palette_harmony` -- spread-global, added once rather than per slot,
    /// so it needs two photos to say anything at all. Identical geometry and
    /// identical aesthetics in both candidates; only the dominant colours
    /// move, from agreeing exactly to a quarter-turn apart on the hue
    /// circle.
    #[test]
    fn score_prefers_a_spread_whose_photos_share_a_dominant_hue() {
        let lib = fixture_library();
        let t = &lib[0];

        let warm = PaletteColor { r: 1.0, g: 0.5, b: 0.0, weight: 1.0 };
        let cool = PaletteColor { r: 0.0, g: 0.5, b: 1.0, weight: 1.0 };

        let mut a_wide = photo(6000, 3000);
        a_wide.palette = vec![warm.clone()];
        let mut a_tall = photo(3000, 6000);
        a_tall.palette = vec![warm.clone()];

        let mut b_wide = photo(6000, 3000);
        b_wide.palette = vec![warm];
        let mut b_tall = photo(3000, 6000);
        b_tall.palette = vec![cool];

        let coherent =
            score_spread(t, &[&a_wide, &a_tall], &[0, 1], None, &Weights::default()).unwrap();
        let clashing =
            score_spread(t, &[&b_wide, &b_tall], &[0, 1], None, &Weights::default()).unwrap();
        assert!(coherent > clashing, "a coherent palette must score higher: {coherent} vs {clashing}");
    }

    /// A faceless photo is not a bad photo, so an absent capture quality scores
    /// NEUTRALLY rather than zero -- the same convention saliency_retention and
    /// face_area_retention already use for a missing signal.
    #[test]
    fn score_face_quality_is_neutral_without_a_face_and_ordered_with_one() {
        let mut none = photo(4000, 3000);
        none.capture_quality = None;
        assert_eq!(face_quality(&none), 0.5);

        let mut poor = photo(4000, 3000);
        poor.capture_quality = Some(0.1);
        let mut good = photo(4000, 3000);
        good.capture_quality = Some(0.9);

        assert!(
            face_quality(&good) > face_quality(&poor),
            "a well-captured face must outscore a poorly-captured one"
        );
        assert!((0.0..=1.0).contains(&face_quality(&good)));
        assert!((0.0..=1.0).contains(&face_quality(&poor)));
    }

    /// The term must actually reach the total. Weight 0.0 ships, so a test
    /// using the shipped weights would pass with the function body deleted --
    /// this one gives it a non-zero weight explicitly.
    #[test]
    fn score_face_quality_changes_the_spread_total_when_weighted() {
        let t = one_slot_template();
        let mut poor = photo(4000, 3000);
        poor.capture_quality = Some(0.1);
        let mut good = photo(4000, 3000);
        good.capture_quality = Some(0.9);

        let w = Weights { face_quality: 1.0, ..Weights::default() };
        let poor_score = score_spread(&t, &[&poor], &[0], None, &w).expect("scores");
        let good_score = score_spread(&t, &[&good], &[0], None, &w).expect("scores");
        assert!(
            good_score > poor_score,
            "face_quality never reached the total: {good_score} vs {poor_score}"
        );

        // And at the SHIPPED weight it changes nothing, which is what "inert"
        // means and what keeps the golden stable.
        let inert = Weights::default();
        assert_eq!(
            score_spread(&t, &[&poor], &[0], None, &inert),
            score_spread(&t, &[&good], &[0], None, &inert),
            "the term must be inert at its shipped weight of 0.0"
        );
    }

    /// Generic salient content in the gutter is a PENALTY, never a rejection --
    /// otherwise no slot could ever run flush to the fold. A saliency box
    /// straddling the dead band must score below one clear of it, and both must
    /// still produce a score at all.
    ///
    /// The box coordinates (1/16, 3/16, 13/16 ...) are deliberately exact
    /// binary fractions, not the rounder 0.05/0.75 an author would reach for
    /// first. `Rect::intersect` recomputes width as `right() - x`, and with
    /// an inexact x (0.05 or 0.75 are not exact in f64) that recomputation
    /// can land 1 ULP away from the original width depending on x's
    /// magnitude -- verified by instrumenting `saliency_retention` on the
    /// 0.05/0.75 fixture, which produced 1.0 exactly for one candidate and
    /// 0.9999999999999998 for the other. That 1-ULP drift is invisible here
    /// (this test only asserts `<`) but is exactly what breaks the
    /// companion `assert_eq!` in the weighted test below, and it has
    /// nothing to do with `gutter_saliency` -- it is a pre-existing property
    /// of `saliency_retention` (weight 0.8, non-zero by default) that this
    /// fixture must not accidentally exercise.
    #[test]
    fn score_gutter_saliency_penalises_without_rejecting() {
        let slot = full_left_page_slot();
        let mut clear = photo(4000, 3000);
        // Well away from the fold, in the photo's own normalised coordinates.
        clear.saliency_box = Some(Rect::new(0.0625, 0.30, 0.1875, 0.30));

        let mut in_gutter = photo(4000, 3000);
        // Flush to the photo's own right edge, which maps into the dead band.
        in_gutter.saliency_box = Some(Rect::new(0.8125, 0.30, 0.1875, 0.30));

        let crop_clear = choose_crop(&clear, slot_aspect(&slot));
        let crop_gutter = choose_crop(&in_gutter, slot_aspect(&slot));

        // Prove the mapped rects land where the term needs them to: one
        // clearing the dead band entirely, one genuinely straddling it. See
        // the report for these numbers.
        let mapped_clear = face_in_page(
            &clear.saliency_box.unwrap().intersect(&crop_clear).unwrap(),
            &crop_clear,
            &slot,
        )
        .unwrap();
        let mapped_gutter = face_in_page(
            &in_gutter.saliency_box.unwrap().intersect(&crop_gutter).unwrap(),
            &crop_gutter,
            &slot,
        )
        .unwrap();
        let band_start = 1.0 - crate::geometry::GUTTER_U;
        eprintln!(
            "mapped_clear = {mapped_clear:?} (right={}), mapped_gutter = {mapped_gutter:?} (right={}), band_start={band_start}",
            mapped_clear.right(),
            mapped_gutter.right()
        );
        assert!(
            mapped_clear.right() < band_start,
            "clear fixture must not reach the dead band: right={}, band_start={band_start}",
            mapped_clear.right()
        );
        assert!(
            mapped_gutter.x < band_start && mapped_gutter.right() > band_start,
            "gutter fixture must straddle the dead band: x={}, right={}, band_start={band_start}",
            mapped_gutter.x,
            mapped_gutter.right()
        );

        let g_clear = gutter_saliency(&clear, &crop_clear, &slot, Side::Left);
        let g_gutter = gutter_saliency(&in_gutter, &crop_gutter, &slot, Side::Left);

        assert!(
            g_gutter < g_clear,
            "saliency in the gutter must score lower: {g_gutter} vs {g_clear}"
        );
        assert!((0.0..=1.0).contains(&g_gutter) && (0.0..=1.0).contains(&g_clear));

        // And it must remain a penalty, not a rejection.
        assert!(
            rejects(&in_gutter, &crop_gutter, &slot, Side::Left).is_none(),
            "generic saliency in the gutter must not reject the candidate"
        );
    }

    /// A photo with no saliency box scores neutrally: absence of a signal is
    /// not evidence of a bad placement, the same rule saliency_retention uses.
    #[test]
    fn score_gutter_saliency_is_neutral_without_a_saliency_box() {
        let slot = full_left_page_slot();
        let mut p = photo(4000, 3000);
        p.saliency_box = None;
        let crop = choose_crop(&p, slot_aspect(&slot));
        assert_eq!(gutter_saliency(&p, &crop, &slot, Side::Left), 1.0);
    }

    /// The term must reach the total; shipped weight is 0.0.
    ///
    /// Same exact-binary-fraction box coordinates as
    /// `score_gutter_saliency_penalises_without_rejecting`, for the same
    /// reason: with the inert weight, EVERY other term must be bit-identical
    /// between the two candidates so the `assert_eq!` below isolates
    /// `gutter_saliency` (weight 0.0, contributes exactly 0.0 regardless of
    /// its own value) rather than tripping on 1-ULP noise from
    /// `saliency_retention` (weight 0.8) recomputing width via subtraction.
    #[test]
    fn score_gutter_saliency_changes_the_spread_total_when_weighted() {
        let t = one_slot_template();
        let mut clear = photo(4000, 3000);
        clear.saliency_box = Some(Rect::new(0.0625, 0.30, 0.1875, 0.30));
        let mut in_gutter = photo(4000, 3000);
        in_gutter.saliency_box = Some(Rect::new(0.8125, 0.30, 0.1875, 0.30));

        let w = Weights { gutter_saliency: 1.0, ..Weights::default() };
        let a = score_spread(&t, &[&clear], &[0], None, &w).expect("scores");
        let b = score_spread(&t, &[&in_gutter], &[0], None, &w).expect("scores");
        assert!(a > b, "gutter_saliency never reached the total: {a} vs {b}");

        let inert = Weights::default();
        let ia = score_spread(&t, &[&clear], &[0], None, &inert).expect("scores");
        let ib = score_spread(&t, &[&in_gutter], &[0], None, &inert).expect("scores");
        assert_eq!(ia, ib, "the term must be inert at its shipped weight of 0.0");
    }

    #[test]
    fn score_best_spread_returns_none_when_every_candidate_is_rejected() {
        let lib = fixture_library();
        let tiny = photo(60, 40);
        let refs: Vec<&SpreadTemplate> = lib.iter().collect();
        let photos = vec![&tiny, &tiny];
        assert!(best_spread(&refs, &photos, None, &Weights::default(), 0).is_none());
    }

    /// The tie-break has to be REACHED to be tested. Two templates with
    /// identical geometry score identically on the same photos, so only the
    /// tie-break rule can decide. Asserting BOTH presentation orders is what
    /// makes this load-bearing: "keep the first" passes one order, "keep the
    /// last" passes the other, and only a tie-break independent of
    /// enumeration order passes both.
    ///
    /// The winner is no longer pinned to the lexicographically smallest id --
    /// `best_spread` now sorts tied candidates by id and hands the SEED the
    /// choice, the same way `pace::best_single` already does, so that
    /// "regenerate this spread" has something to advance. What must still
    /// hold is that presentation order never decides it: the same seed must
    /// pick the same winner regardless of which order the candidates arrive
    /// in.
    #[test]
    fn score_best_spread_breaks_ties_on_the_seed_not_iteration_order() {
        let mut early = fixture_library().remove(0);
        early.id = "aaa-first".into();
        let mut late = fixture_library().remove(0);
        late.id = "zzz-last".into();

        let wide = photo(6000, 3000);
        let tall = photo(3000, 6000);
        let photos = vec![&wide, &tall];

        let forward: Vec<&SpreadTemplate> = vec![&early, &late];
        let backward: Vec<&SpreadTemplate> = vec![&late, &early];
        let (a, a_assign, a_score) =
            best_spread(&forward, &photos, None, &Weights::default(), 7).unwrap();
        let (b, _, b_score) =
            best_spread(&backward, &photos, None, &Weights::default(), 7).unwrap();

        assert_eq!(a_score, b_score, "the fixture must actually reach a tie");
        assert_eq!(a.id, b.id, "iteration order must not decide the winner; the seed does");
        assert_eq!(a_assign, vec![0, 1], "and the winner is still the best assignment");
    }

    /// A slot on a LEFT page, flush to the fold (`x + w == 1.0` in page
    /// coordinates) and non-square. Flush to the fold so per-slot terms that
    /// care about position relative to the gutter dead band can exercise it;
    /// non-square so an area- or aspect-dependent term cannot hide behind
    /// `w == h`.
    fn full_left_page_slot() -> Slot {
        Slot {
            rect: Rect::new(0.7, 0.375, 0.3, 0.25),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.6),
        }
    }

    /// A 1-photo spread template built around `full_left_page_slot`, so a
    /// per-slot term can be varied without a second slot's terms moving
    /// alongside it. A legal template: `photo_count()` is 1 and
    /// `ordered_slots` handles an empty page.
    fn one_slot_template() -> SpreadTemplate {
        SpreadTemplate {
            id: "fx-one-slot".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![full_left_page_slot()],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: Vec::new(),
            },
            density: Density::Sparse,
            energy: Energy::Calm,
        }
    }

    /// One slot on the left page and none on the right, so a per-slot term
    /// can be varied without a second slot's terms moving alongside it. A
    /// legal template: `photo_count()` is 1 and `ordered_slots` handles an
    /// empty page.
    fn single_hero_template() -> SpreadTemplate {
        SpreadTemplate {
            id: "fx-single-hero".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.1, 0.2, 0.8, 0.35),
                    role: Role::Hero,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.5, 3.0),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: Vec::new(),
            },
            density: Density::Sparse,
            energy: Energy::Calm,
        }
    }

    /// Two photos, two slots, ASYMMETRIC on purpose -- a symmetric fixture
    /// cannot detect an assignment that is silently reversed.
    fn fixture_library() -> Vec<SpreadTemplate> {
        vec![SpreadTemplate {
            id: "fx-hero-support".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.1, 0.2, 0.8, 0.35), // wide
                    role: Role::Hero,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.5, 3.0),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.3, 0.1, 0.3, 0.75), // tall
                    role: Role::Support,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (0.4, 0.6),
                }],
            },
            density: Density::Medium,
            energy: Energy::Calm,
        }]
    }

    /// Two photos, one slot per page half, both non-square and both
    /// resolving comfortably above the 200 DPI floor for a 4000x3000 photo.
    /// Used to isolate `spread_diversity`, a spread-global term, from
    /// per-slot terms without pulling in the asymmetric hero/support shape
    /// `fixture_library` uses for other purposes.
    fn two_slot_template() -> SpreadTemplate {
        SpreadTemplate {
            id: "fx-two-slot".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.1, 0.2, 0.7, 0.35),
                    role: Role::Hero,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.2, 2.6),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.15, 0.15, 0.7, 0.35),
                    role: Role::Support,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.2, 2.6),
                }],
            },
            density: Density::Medium,
            energy: Energy::Calm,
        }
    }

    /// Three photos: one large Hero slot on the left page, two small Support
    /// slots on the right. The hero clearly dominates the spread's area.
    /// Non-square throughout, and every slot resolves well above 200 DPI for
    /// a 4000x3000 photo.
    fn hero_dominant_template() -> SpreadTemplate {
        SpreadTemplate {
            id: "fx-hero-dominant".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.1, 0.15, 0.8, 0.6),
                    role: Role::Hero,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (1.5, 1.8),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![
                    Slot {
                        rect: Rect::new(0.1, 0.05, 0.35, 0.4),
                        role: Role::Support,
                        bleed: Vec::<BleedEdge>::new(),
                        aspect_pref: (1.0, 1.3),
                    },
                    Slot {
                        rect: Rect::new(0.1, 0.5, 0.35, 0.4),
                        role: Role::Support,
                        bleed: Vec::<BleedEdge>::new(),
                        aspect_pref: (1.0, 1.3),
                    },
                ],
            },
            density: Density::Dense,
            energy: Energy::Lively,
        }
    }

    /// Three photos, three equal-area Support slots -- the flat counterpart
    /// to `hero_dominant_template`. Same slot dimensions repeated across
    /// both pages so every slot's area is identical by construction. Non-
    /// square throughout, and every slot resolves well above 200 DPI for a
    /// 4000x3000 photo.
    fn even_three_up_template() -> SpreadTemplate {
        SpreadTemplate {
            id: "fx-even-three-up".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.15, 0.3, 0.7, 0.35),
                    role: Role::Support,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.4, 2.6),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![
                    Slot {
                        rect: Rect::new(0.15, 0.05, 0.7, 0.35),
                        role: Role::Support,
                        bleed: Vec::<BleedEdge>::new(),
                        aspect_pref: (2.4, 2.6),
                    },
                    Slot {
                        rect: Rect::new(0.15, 0.55, 0.7, 0.35),
                        role: Role::Support,
                        bleed: Vec::<BleedEdge>::new(),
                        aspect_pref: (2.4, 2.6),
                    },
                ],
            },
            density: Density::Dense,
            energy: Energy::Lively,
        }
    }
}
