//! The case-bound cover: a front photo, a back photo and a plain-colour
//! spine. No text and no single photo wrapping round the spine; the user
//! chose separate front and back photos.
//!
//! A cover panel is one trim page plus the wrap on its three outer edges
//! (`PrintSpec::cover_panel_w_in`), and a cover crop is cut to that panel's
//! aspect. The panel-normalised crop is the whole panel, so a face's place on
//! the finished board is its place in the crop, with no slot in between.

use crate::book::crop::choose_crop;
use crate::book::cull::{PaletteColor, Photo};
use crate::book::score::Rejection;
use crate::geometry::{CoverSide, Rect};
use crate::print_spec::PrintSpec;
use serde::{Deserialize, Serialize};

/// A plain sRGB colour, on the wire and on disk as `#rrggbb`.
///
/// One spelling everywhere -- the stored book, the edit the webview sends,
/// the preview and the manifest -- so there is no second form to convert
/// between, and a malformed colour is refused where it enters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// White: the spine of a cover with no photo to take a colour from, the
/// colour of the unprinted board.
impl Default for Rgb {
    fn default() -> Self {
        Self { r: 255, g: 255, b: 255 }
    }
}

impl Rgb {
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// The sidecar's palette channels are 0-1 averages of 8-bit samples.
    fn from_palette(c: &PaletteColor) -> Self {
        let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self { r: channel(c.r), g: channel(c.g), b: channel(c.b) }
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let digits = s
            .strip_prefix('#')
            .filter(|d| d.len() == 6 && d.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| format!("{s:?} is not a #rrggbb colour"))?;
        let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).expect("checked hex");
        Ok(Self { r: channel(0), g: channel(2), b: channel(4) })
    }
}

impl From<Rgb> for String {
    fn from(c: Rgb) -> Self {
        c.hex()
    }
}

/// One cover photo: a `Placement` without a slot, because the slot is the
/// whole panel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverPhoto {
    /// Indexes the analysed photos, like `Placement::photo_index`.
    pub photo_index: usize,
    /// In the photo's own normalised frame, at the panel's aspect.
    pub crop: Rect,
}

/// `None` on a side means that side prints as plain board: there was no
/// photo that could go there, or the user cleared it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Cover {
    pub front: Option<CoverPhoto>,
    pub back: Option<CoverPhoto>,
    pub spine: Rgb,
}

impl Cover {
    pub fn side(&self, side: CoverSide) -> Option<&CoverPhoto> {
        match side {
            CoverSide::Front => self.front.as_ref(),
            CoverSide::Back => self.back.as_ref(),
        }
    }

    pub fn side_mut(&mut self, side: CoverSide) -> &mut Option<CoverPhoto> {
        match side {
            CoverSide::Front => &mut self.front,
            CoverSide::Back => &mut self.back,
        }
    }
}

/// The photo's resolution over the panel's full printed width.
pub fn effective_dpi(spec: &PrintSpec, photo: &Photo, crop: &Rect) -> f64 {
    photo.width as f64 * crop.w / spec.cover_panel_w_in()
}

/// The cover's hard constraints, the same three the interior enforces:
/// resolution, a face cut by the crop, and a face outside the visible rect.
///
/// A face in the wrap folds under the board, so the visible rect already
/// excludes the wrap, and that case is reported as `FaceInSafeMargin` --
/// the face is outside the area that shows, which is what that refusal
/// means on a page too.
pub fn rejects(spec: &PrintSpec, photo: &Photo, crop: &Rect, side: CoverSide) -> Option<Rejection> {
    if effective_dpi(spec, photo, crop) < spec.min_dpi() {
        return Some(Rejection::TooLowResolution);
    }
    let visible = spec.cover_visible_rect(side);
    for face in &photo.faces {
        if !crop.contains(&face.box_) {
            // Wholly outside is simply not in the picture; only a partial
            // overlap is a half-face.
            if face.box_.intersect(crop).is_some() {
                return Some(Rejection::FaceClipped);
            }
            continue;
        }
        let on_panel = Rect::new(
            (face.box_.x - crop.x) / crop.w,
            (face.box_.y - crop.y) / crop.h,
            face.box_.w / crop.w,
            face.box_.h / crop.h,
        );
        if !visible.contains(&on_panel) {
            return Some(Rejection::FaceInSafeMargin);
        }
    }
    None
}

/// The automatic crop for a side, or why it cannot go there.
pub fn crop_for(spec: &PrintSpec, photo: &Photo, side: CoverSide) -> Result<Rect, Rejection> {
    let crop = choose_crop(photo, spec.cover_aspect());
    match rejects(spec, photo, &crop, side) {
        Some(reason) => Err(reason),
        None => Ok(crop),
    }
}

/// The cover `assemble` starts a book with.
///
/// `candidates` index `photos`, and are the photos that survived culling.
/// Front: the best-looking candidate whose automatic crop passes the
/// cover's constraints, ties broken by sharpness and then path so the choice
/// is a total order. Back: the next such photo from a different event, so
/// the two covers are not two frames of one moment; failing that, any other.
/// Spine: the front's dominant palette colour.
pub fn choose(spec: &PrintSpec, photos: &[Photo], candidates: &[usize]) -> Cover {
    let mut ranked: Vec<usize> = candidates.to_vec();
    ranked.sort_by(|&a, &b| {
        let (pa, pb) = (&photos[a], &photos[b]);
        pb.aesthetic_pct
            .cmp(&pa.aesthetic_pct)
            .then(pb.sharpness_pct.cmp(&pa.sharpness_pct))
            .then(pa.path.cmp(&pb.path))
    });
    ranked.dedup();

    let fit = |i: usize, side| {
        crop_for(spec, &photos[i], side).ok().map(|crop| CoverPhoto { photo_index: i, crop })
    };
    let front = ranked.iter().find_map(|&i| fit(i, CoverSide::Front));
    let back = front.and_then(|f| {
        let event = photos[f.photo_index].event_cluster;
        let others = || ranked.iter().filter(move |&&i| i != f.photo_index);
        others()
            .filter(|&&i| photos[i].event_cluster != event)
            .find_map(|&i| fit(i, CoverSide::Back))
            .or_else(|| others().find_map(|&i| fit(i, CoverSide::Back)))
    });
    let spine = front
        .and_then(|f| {
            photos[f.photo_index]
                .palette
                .iter()
                .max_by(|a, b| a.weight.total_cmp(&b.weight))
        })
        .map(Rgb::from_palette)
        .unwrap_or_default();
    Cover { front, back, spine }
}

/// Re-cuts both cover crops for a new panel shape, keeping the photos.
///
/// Mirrors `reprint`: a size change never refuses, so a photo that no longer
/// fits keeps its place and pre-flight reports it.
pub fn recut(cover: &mut Cover, photos: &[Photo], spec: &PrintSpec) {
    for side in [CoverSide::Front, CoverSide::Back] {
        if let Some(c) = cover.side_mut(side) {
            c.crop = choose_crop(&photos[c.photo_index], spec.cover_aspect());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::Face;
    use crate::print_spec::{odd_spec, pixajoy_spec};

    /// Landscape 4:3, never square: at Pixajoy's 1.175 cover aspect the crop
    /// is the full height and 88% of the width, so the wrap is in the crop.
    fn photo(i: usize, aesthetic: u8) -> Photo {
        Photo {
            path: format!("/c/p{i:02}.jpg"),
            hash: format!("h{i:02}"),
            width: 4000,
            height: 3000,
            is_utility: false,
            aesthetic_pct: aesthetic,
            sharpness_pct: 50,
            near_dup_cluster: i as u32,
            event_cluster: i as u32,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0,
            clipped_high: 0.0,
            feature_print: None,
        }
    }

    fn face(x: f64, y: f64, w: f64, h: f64) -> Face {
        Face { box_: Rect::new(x, y, w, h), capture_quality: Some(0.5) }
    }

    fn all(photos: &[Photo]) -> Vec<usize> {
        (0..photos.len()).collect()
    }

    #[test]
    fn rgb_is_hex_on_the_wire_and_refuses_anything_else() {
        let c: Rgb = serde_json::from_str(r##""#1A2b3c""##).unwrap();
        assert_eq!(c, Rgb { r: 0x1a, g: 0x2b, b: 0x3c });
        assert_eq!(serde_json::to_string(&c).unwrap(), r##""#1a2b3c""##);
        for bad in [r#""1a2b3c""#, r##""#1a2b3""##, r##""#1a2b3cd""##, r##""#gg0000""##, r#""""#] {
            assert!(serde_json::from_str::<Rgb>(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn rgb_from_palette_rounds_each_channel_from_the_unit_range() {
        let c = Rgb::from_palette(&PaletteColor { r: 1.0, g: 0.5, b: 0.2, weight: 1.0 });
        assert_eq!(c.hex(), "#ff8033");
    }

    /// A book saved before the cover existed has no `cover` key; it loads
    /// empty with a white spine, and an empty cover round-trips.
    #[test]
    fn an_empty_cover_round_trips() {
        let c = Cover::default();
        let json = serde_json::to_value(c).unwrap();
        assert_eq!(json, serde_json::json!({"front": null, "back": null, "spine": "#ffffff"}));
        assert_eq!(serde_json::from_value::<Cover>(json).unwrap(), c);
    }

    #[test]
    fn a_cover_photo_serialises_camel_case() {
        let c = CoverPhoto { photo_index: 3, crop: Rect::new(0.0, 0.0, 0.5, 1.0) };
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            serde_json::json!({"photoIndex": 3, "crop": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 1.0}})
        );
    }

    /// The face sits at y 0.02-0.07 of a full-height crop: inside the photo,
    /// inside the crop, and inside Pixajoy's 0.75" top wrap (the visible rect
    /// starts at 0.875/10 = 0.0875 of the panel). Moved to the middle, the
    /// same face passes, so the refusal is the wrap and nothing else.
    #[test]
    fn a_face_in_the_wrap_is_refused_and_the_same_face_lower_is_not() {
        let spec = pixajoy_spec();
        let mut p = photo(0, 90);
        p.faces = vec![face(0.45, 0.02, 0.08, 0.05)];
        let crop = choose_crop(&p, spec.cover_aspect());
        assert!((crop.h - 1.0).abs() < 1e-9 && crop.w < 0.9, "sanity: full-height crop {crop:?}");
        assert!(crop.contains(&p.faces[0].box_), "sanity: the face is in the crop");
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), Some(Rejection::FaceInSafeMargin));

        p.faces = vec![face(0.45, 0.40, 0.08, 0.05)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), None);
    }

    /// The outer-edge wrap is on opposite sides of the two panels. A face
    /// hard against the crop's right edge is in the front's wrap but in the
    /// back's visible area, and the mirror case holds for the left edge.
    #[test]
    fn the_wrap_is_on_the_right_of_the_front_and_the_left_of_the_back() {
        let spec = odd_spec();
        let mut p = photo(0, 90);
        p.width = 3000;
        p.height = 4000;
        let crop = choose_crop(&p, spec.cover_aspect());
        let fw = 0.02 * crop.w;
        p.faces = vec![face(crop.right() - 2.0 * fw, 0.5, fw, 0.02)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), Some(Rejection::FaceInSafeMargin));
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Back), None);

        p.faces = vec![face(crop.x + fw, 0.5, fw, 0.02)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Back), Some(Rejection::FaceInSafeMargin));
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), None);
    }

    /// A face's place on the board is its place in the CROP. Both faces sit
    /// well inside the photo's middle but hard against the edge of an offset
    /// crop window, so reading the photo coordinate instead passes them.
    #[test]
    fn a_face_is_placed_on_the_panel_through_the_crop_window() {
        let spec = pixajoy_spec();
        let mut p = photo(0, 90);
        p.width = 3000;
        p.height = 4000;
        let crop = Rect::new(0.0, 0.3, 1.0, p.aspect() / spec.cover_aspect());
        p.faces = vec![face(0.45, 0.31, 0.08, 0.03)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), Some(Rejection::FaceInSafeMargin));

        let mut q = photo(1, 90);
        let crop = Rect::new(0.05, 0.0, spec.cover_aspect() / q.aspect(), 1.0);
        q.faces = vec![face(crop.right() - 0.04, 0.45, 0.02, 0.05)];
        assert_eq!(rejects(&spec, &q, &crop, CoverSide::Front), Some(Rejection::FaceInSafeMargin));
    }

    #[test]
    fn a_face_half_out_of_the_crop_is_clipped_and_one_wholly_out_is_ignored() {
        let spec = pixajoy_spec();
        let mut p = photo(0, 90);
        let crop = choose_crop(&p, spec.cover_aspect());
        p.faces = vec![face(crop.right() - 0.02, 0.4, 0.05, 0.05)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), Some(Rejection::FaceClipped));
        p.faces = vec![face(crop.right() + 0.01, 0.4, 0.02, 0.05)];
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), None);
    }

    /// DPI is over the PANEL's width, not the page's: 2600 px at 0.88 of
    /// the width is ~195 DPI over 11.75" (refused) and ~208 over 11" (would
    /// pass), so measuring against the page lets this photo through.
    #[test]
    fn resolution_is_measured_over_the_panel_width() {
        let spec = pixajoy_spec();
        let mut p = photo(0, 90);
        p.width = 2600;
        p.height = 1950;
        let crop = choose_crop(&p, spec.cover_aspect());
        let over_page = p.width as f64 * crop.w / (spec.page_w_in() - spec.bleed_in());
        assert!(over_page >= spec.min_dpi(), "sanity: passes over the trim, {over_page}");
        assert_eq!(rejects(&spec, &p, &crop, CoverSide::Front), Some(Rejection::TooLowResolution));
    }

    #[test]
    fn the_front_is_the_best_looking_photo_that_fits() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(0, 60), photo(1, 95), photo(2, 80), photo(3, 99)];
        photos[3].faces = vec![face(0.45, 0.02, 0.08, 0.05)];
        let cover = choose(&spec, &photos, &all(&photos));
        let front = cover.front.expect("a photo fits");
        assert_eq!(front.photo_index, 1, "99 has a face in the wrap; 95 is next");
        assert_eq!(front.crop, choose_crop(&photos[1], spec.cover_aspect()));
    }

    #[test]
    fn a_tie_goes_to_the_sharper_photo_then_the_earlier_path() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(2, 90), photo(1, 90), photo(0, 90)];
        photos[1].sharpness_pct = 70;
        assert_eq!(choose(&spec, &photos, &all(&photos)).front.unwrap().photo_index, 1);

        photos[1].sharpness_pct = 50;
        let front = choose(&spec, &photos, &all(&photos)).front.unwrap();
        assert_eq!(photos[front.photo_index].path, "/c/p00.jpg");
    }

    /// Only candidates are considered, and indices are into `photos`, so a
    /// better photo that was culled never reaches the cover.
    #[test]
    fn only_candidates_reach_the_cover() {
        let spec = pixajoy_spec();
        let photos = vec![photo(0, 99), photo(1, 50), photo(2, 70)];
        let cover = choose(&spec, &photos, &[1, 2]);
        assert_eq!(cover.front.unwrap().photo_index, 2);
        assert_eq!(cover.back.unwrap().photo_index, 1);
    }

    #[test]
    fn the_back_comes_from_a_different_event_when_one_fits() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(0, 99), photo(1, 95), photo(2, 60)];
        photos[1].event_cluster = photos[0].event_cluster;
        let cover = choose(&spec, &photos, &all(&photos));
        assert_eq!(cover.front.unwrap().photo_index, 0);
        assert_eq!(cover.back.unwrap().photo_index, 2, "95 is the same event as the front");
        assert_eq!(cover.back.unwrap().crop, choose_crop(&photos[2], spec.cover_aspect()));
    }

    #[test]
    fn the_back_falls_back_to_the_same_event_rather_than_nothing() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(0, 99), photo(1, 95), photo(2, 60)];
        for p in &mut photos {
            p.event_cluster = 7;
        }
        let cover = choose(&spec, &photos, &all(&photos));
        assert_eq!(cover.back.unwrap().photo_index, 1);
    }

    /// The back is checked against the BACK panel's wrap: a photo whose face
    /// is in the back's outer wrap but clear on the front is skipped there.
    /// Portrait under a portrait panel, so the crop is the full width and
    /// cannot slide sideways to centre the face out of the wrap.
    #[test]
    fn the_back_is_checked_against_its_own_side() {
        let spec = odd_spec();
        let mut photos = vec![photo(0, 99), photo(1, 95), photo(2, 60)];
        photos[1].width = 3000;
        photos[1].height = 4000;
        photos[1].faces = vec![face(0.02, 0.5, 0.02, 0.02)];
        let crop = choose_crop(&photos[1], spec.cover_aspect());
        assert!((crop.w - 1.0).abs() < 1e-9, "sanity: full-width crop {crop:?}");
        assert!(rejects(&spec, &photos[1], &crop, CoverSide::Front).is_none(), "sanity");
        assert!(rejects(&spec, &photos[1], &crop, CoverSide::Back).is_some(), "sanity");
        let cover = choose(&spec, &photos, &all(&photos));
        assert_eq!(cover.back.unwrap().photo_index, 2);
    }

    #[test]
    fn with_nothing_that_fits_both_sides_are_empty_and_the_spine_is_white() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(0, 99)];
        photos[0].width = 400;
        photos[0].height = 300;
        let cover = choose(&spec, &photos, &all(&photos));
        assert_eq!(cover, Cover::default());
        assert_eq!(choose(&spec, &[], &[]), Cover::default());
    }

    #[test]
    fn a_single_photo_fills_the_front_and_leaves_the_back_empty() {
        let spec = pixajoy_spec();
        let photos = vec![photo(0, 99)];
        let cover = choose(&spec, &photos, &all(&photos));
        assert_eq!(cover.front.unwrap().photo_index, 0);
        assert_eq!(cover.back, None, "the front is never repeated on the back");
    }

    /// The heaviest colour, not the first: the fixture lists it second.
    #[test]
    fn the_spine_is_the_fronts_heaviest_palette_colour() {
        let spec = pixajoy_spec();
        let mut photos = vec![photo(0, 99), photo(1, 50)];
        photos[0].palette = vec![
            PaletteColor { r: 0.0, g: 0.0, b: 0.0, weight: 0.3 },
            PaletteColor { r: 0.2, g: 0.4, b: 0.6, weight: 0.5 },
            PaletteColor { r: 1.0, g: 1.0, b: 1.0, weight: 0.2 },
        ];
        photos[1].palette = vec![PaletteColor { r: 1.0, g: 0.0, b: 0.0, weight: 1.0 }];
        assert_eq!(choose(&spec, &photos, &all(&photos)).spine.hex(), "#336699");
    }

    #[test]
    fn recut_keeps_the_photos_and_cuts_to_the_new_panel() {
        let photos = vec![photo(0, 99), photo(1, 50)];
        let mut cover = choose(&pixajoy_spec(), &photos, &all(&photos));
        let before = cover;
        recut(&mut cover, &photos, &odd_spec());
        for (after, orig) in [(cover.front, before.front), (cover.back, before.back)] {
            let (after, orig) = (after.unwrap(), orig.unwrap());
            assert_eq!(after.photo_index, orig.photo_index);
            assert_eq!(after.crop, choose_crop(&photos[after.photo_index], odd_spec().cover_aspect()));
            assert_ne!(after.crop, orig.crop, "portrait panel must change a landscape crop");
        }
        assert_eq!(cover.spine, before.spine);
    }
}
