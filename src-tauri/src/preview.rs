//! The read-only preview payload: the assembled book, as the webview needs
//! it to draw the thing a human can actually judge.
//!
//! Nothing here is a second layout engine. Every number it emits is either
//! copied straight off the `Book` that `pace::assemble` produced and
//! `project.rs` persisted, or read from `geometry.rs`'s own constants. The
//! preview's job is to REVEAL what the engine chose -- a crop that cuts a
//! face, salient content sliding into the gutter, a page that prints blank --
//! so anything it recomputes is a chance for the screen and the print to
//! disagree.
//!
//! Two decisions follow from that:
//!
//! * **The guide constants travel on the wire.** `PreviewGeometry` carries
//!   `TRIM_U`, `SAFE_U`, `GUTTER_U` and friends from `geometry.rs` rather
//!   than letting TypeScript restate them. A guide drawn from a hand-copied
//!   constant is a guide that can drift out of step with the predicate the
//!   scorer actually enforced, and the whole point of drawing it is that it
//!   is the same line.
//! * **Every page is emitted, including the ones that hold nothing.** Ten of
//!   the 36 templates put all their slots on one page half (mostly because
//!   they carry text zones nothing renders yet), so a full printed page comes
//!   out blank white. A preview that quietly skipped those would be flattering
//!   the engine, and the user would meet them for the first time in print.

use crate::book::cull::Photo;
use crate::book::edit::{alternatives, opening_count};
use crate::book::pace::Book;
use crate::templates::Library;
use crate::export::output_filename;
use crate::geometry::{
    Rect, Side, GUTTER_U, PAGE_H_IN, PAGE_W_IN, SAFE_U, SAFE_V, TRIM_U, TRIM_V,
};
use serde::Serialize;

/// One photo of the set the book was assembled against, in the SAME ORDER as
/// that slice -- so `PreviewPlacement::photo_index` indexes straight into
/// `BookLayout::photos`, exactly as it indexes into `manifest`'s `photos`
/// argument.
///
/// Every analysed photo is present, not only the placed ones. Dropping the
/// unplaced ones would renumber every index and change which photo each
/// placement points at; it is also what makes "which photos were left out"
/// answerable at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPhoto {
    pub path: String,
    pub hash: String,
    /// ORIENTED pixel dimensions -- EXIF orientation is applied during the
    /// decode (`ExifReader` swaps the axes for orientations 5-8), which is
    /// the same frame `Placement::crop` is normalised against.
    pub width: u32,
    pub height: u32,
    /// The 400px contact-sheet JPEG the sidecar wrote, keyed by content hash,
    /// or `None` when writing it failed. A missing thumbnail is an empty slot
    /// in the preview, never a missing placement -- the layout is still real.
    pub thumbnail_path: Option<String>,
    /// The analysis's own quality percentile, what "Best first" sorts by.
    pub aesthetic_pct: u8,
    /// Capture time in Unix seconds, or `None` when the photo carries none.
    pub captured_at: Option<i64>,
}

/// One photo in one slot: where it lands on the page, and which part of it
/// prints. Mirrors `pace::Placement` field for field, but serialised
/// camelCase for the webview -- `Placement` itself serialises snake_case
/// into `book_json`, and changing that would break every saved project.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPlacement {
    pub photo_index: usize,
    /// Page-normalised destination rect.
    pub slot_rect: Rect,
    /// Crop window in the photo's own oriented, normalised frame. This is the
    /// single most important number on the screen: rendering the whole photo
    /// instead of this window hides exactly the defects the preview exists to
    /// show.
    pub crop: Rect,
    pub z: u32,
    /// The basename the exporter writes this placement under, without an
    /// extension -- so a user looking at something odd on screen can find the
    /// file it produced, and check it against `manifest.json`.
    ///
    /// Built with `export::output_filename`, the SAME function
    /// `book::manifest` and `export::build_items` use. Deriving it in the
    /// webview instead would be a second copy of the `p{page:02}-z{z}-{hash8}`
    /// rule, and a preview naming files the exporter does not write is worse
    /// than one naming none.
    ///
    /// `None` only when `photo_index` falls outside the photo list, which the
    /// engine never produces -- reported rather than papered over with a
    /// plausible-looking name.
    pub filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPage {
    pub number: u32,
    pub side: Side,
    pub template_id: String,
    /// True when this page prints with nothing on it.
    ///
    /// Two different causes, one appearance: a `BLANK_TEMPLATE_ID` page (no
    /// group was available), and a real template whose slots all sit on the
    /// OTHER half of the spread (the text-zone layouts). Both print white,
    /// so both are flagged -- `template_id` still says which happened.
    pub blank: bool,
    pub placements: Vec<PreviewPlacement>,
}

/// The engine's own print geometry, shipped so the preview draws the same
/// lines the scorer enforced rather than a hand-copied approximation.
///
/// Page-normalised, matching `Placement::slot_rect`. Derived here from
/// `geometry.rs` -- never restated as literals, which is what
/// `preview_geometry_is_read_from_the_engines_own_constants` pins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGeometry {
    pub page_w_in: f64,
    pub page_h_in: f64,
    /// Trim inset on the outer vertical edge. There is NO inset at the fold.
    pub trim_u: f64,
    /// Trim inset on the top and bottom edges.
    pub trim_v: f64,
    /// Pixajoy's 1/8" safe margin, inside the trim, on every edge except the
    /// fold -- where the gutter dead band already governs clearance.
    pub safe_u: f64,
    pub safe_v: f64,
    /// Width of the gutter dead strip, measured inward FROM the fold.
    pub gutter_u: f64,
}

/// The editable state of one opening, numbered as `toSpreads` draws them:
/// `0` is page 1, then each spread, then the last page. See `book::edit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewOpening {
    pub index: usize,
    pub locked: bool,
    /// Templates the user has rejected here, never offered again.
    pub rejected: Vec<String>,
    /// Templates that could replace the current one, in library order:
    /// what the "change template" menu offers. Empty when the opening holds
    /// nothing, or when every template for this many photos has been shown
    /// or rejected -- which is also when "regenerate" has nothing to do.
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookLayout {
    pub project_id: i64,
    pub seed: u64,
    pub page_count: usize,
    /// Placements across the whole book. The packer never places the same
    /// photo twice, so this is also the number of distinct photos used.
    pub placed_photos: usize,
    /// Copied from `Book::dropped` rather than recounted: that is the
    /// engine's own figure, covering every cause (culled, trimmed to
    /// capacity, left unplaced, overflowed a page half).
    pub dropped_photos: usize,
    pub geometry: PreviewGeometry,
    pub photos: Vec<PreviewPhoto>,
    pub pages: Vec<PreviewPage>,
    /// One entry per opening, in `toSpreads` order.
    pub openings: Vec<PreviewOpening>,
}

/// `geometry.rs`'s constants, as the webview receives them.
pub fn preview_geometry() -> PreviewGeometry {
    PreviewGeometry {
        page_w_in: PAGE_W_IN,
        page_h_in: PAGE_H_IN,
        trim_u: TRIM_U,
        trim_v: TRIM_V,
        safe_u: SAFE_U,
        safe_v: SAFE_V,
        gutter_u: GUTTER_U,
    }
}

/// One analysed photo as the preview needs it. `thumbnail_path` is not on
/// `Photo` -- it lives on the cached feature record the layout engine never
/// reads -- so the caller supplies it.
pub fn preview_photo(photo: &Photo, thumbnail_path: Option<String>) -> PreviewPhoto {
    PreviewPhoto {
        path: photo.path.clone(),
        hash: photo.hash.clone(),
        width: photo.width,
        height: photo.height,
        thumbnail_path,
        aesthetic_pct: photo.aesthetic_pct,
        captured_at: photo.captured_at,
    }
}

/// The whole preview payload for one saved book.
///
/// `photos` is the ORIGINAL slice `Placement::photo_index` indexes into --
/// the same contract `manifest::manifest` and `export::build_items` take, and
/// the reason `preview_agrees_with_the_manifest_about_which_photo_is_in_which_slot`
/// can check the two against each other.
pub fn book_layout(
    project_id: i64,
    book: &Book,
    photos: Vec<PreviewPhoto>,
    lib: &Library,
) -> BookLayout {
    let pages: Vec<PreviewPage> = book
        .pages
        .iter()
        .map(|page| PreviewPage {
            number: page.number,
            side: page.side,
            template_id: page.template_id.clone(),
            blank: page.placements.is_empty(),
            placements: page
                .placements
                .iter()
                .map(|p| PreviewPlacement {
                    photo_index: p.photo_index,
                    slot_rect: p.slot_rect,
                    crop: p.crop,
                    z: p.z,
                    filename: photos
                        .get(p.photo_index)
                        .map(|photo| output_filename(page.number, p.z, &photo.hash)),
                })
                .collect(),
        })
        .collect();

    let openings = (0..opening_count(book))
        .map(|index| {
            let controls = book.controls.get(&index);
            PreviewOpening {
                index,
                locked: controls.is_some_and(|c| c.locked),
                rejected: controls.map(|c| c.rejected.clone()).unwrap_or_default(),
                alternatives: alternatives(book, lib, index),
            }
        })
        .collect();

    BookLayout {
        project_id,
        seed: book.seed,
        page_count: pages.len(),
        placed_photos: pages.iter().map(|p| p.placements.len()).sum(),
        dropped_photos: book.dropped,
        geometry: preview_geometry(),
        photos,
        pages,
        openings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::PaletteColor;
    use crate::book::manifest::manifest;
    use crate::book::pace::{Page, Placement, BLANK_TEMPLATE_ID};
    use crate::geometry::{BLEED_IN, SAFE_MARGIN_IN};

    /// NEVER square, and never the same shape twice -- a square photo makes
    /// `height/width == 1` and hides the whole class of aspect bug.
    fn photo(path: &str, hash: &str, width: u32, height: u32) -> Photo {
        Photo {
            path: path.into(),
            hash: hash.into(),
            width,
            height,
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
        }
    }

    /// Every slot rect is DIFFERENT, and none of them is square or centred: a
    /// fixture where two slots share a rect cannot tell "each placement gets
    /// its own box" from "every placement gets the first one".
    fn placement(photo_index: usize, z: u32, slot_rect: Rect, crop: Rect) -> Placement {
        Placement { photo_index, slot_rect, crop, z }
    }

    /// Four pages, deliberately shaped like a real short book:
    ///
    /// * p1 is a RIGHT-hand single facing the inside front cover;
    /// * p2/p3 are a true spread whose template puts every slot on the LEFT
    ///   half -- p3 therefore prints blank while carrying a real template id
    ///   (the text-zone case, which is the common one);
    /// * p4 is a LEFT-hand single that got no group at all.
    ///
    /// Photo indices are deliberately OUT OF ORDER (3, then 1, then 0) so a
    /// consumer that walks placements positionally instead of reading
    /// `photo_index` produces different output.
    fn book() -> Book {
        Book {
            controls: Default::default(),
            seed: 424_242,
            dropped: 2,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Right,
                    template_id: "12-quad-right:right".into(),
                    placements: vec![placement(
                        3,
                        1,
                        Rect::new(0.0, 0.0, 0.72, 1.0),
                        Rect::new(0.3125, 0.0, 0.6875, 1.0),
                    )],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "03-hero-left-text-right".into(),
                    placements: vec![
                        placement(
                            1,
                            1,
                            Rect::new(0.06, 0.08, 0.5, 0.62),
                            Rect::new(0.0, 0.125, 1.0, 0.75),
                        ),
                        placement(
                            0,
                            2,
                            Rect::new(0.6, 0.31, 0.34, 0.45),
                            Rect::new(0.2, 0.1, 0.5, 0.4),
                        ),
                    ],
                },
                Page {
                    number: 3,
                    side: Side::Right,
                    template_id: "03-hero-left-text-right".into(),
                    placements: Vec::new(),
                },
                Page {
                    number: 4,
                    side: Side::Left,
                    template_id: BLANK_TEMPLATE_ID.into(),
                    placements: Vec::new(),
                },
            ],
        }
    }

    /// Scores and capture times deliberately in neither order, and one photo
    /// with no capture time, so the picker's sorts have something to do.
    fn photos() -> Vec<Photo> {
        let scored = |mut p: Photo, aesthetic_pct: u8, captured_at: Option<i64>| {
            p.aesthetic_pct = aesthetic_pct;
            p.captured_at = captured_at;
            p
        };
        vec![
            scored(photo("/a.jpg", "hash-a", 4032, 3024), 62, Some(1_700_000_300)),
            scored(photo("/b.jpg", "hash-b", 3024, 4032), 18, Some(1_700_000_100)),
            scored(photo("/c.jpg", "hash-c", 6000, 4000), 91, None),
            scored(photo("/d.jpg", "hash-d", 5472, 3648), 40, Some(1_700_000_400)),
            scored(photo("/e.jpg", "hash-e", 4000, 6000), 77, Some(1_700_000_200)),
        ]
    }

    fn preview_photos() -> Vec<PreviewPhoto> {
        photos()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                // One photo with no thumbnail, so the "missing thumbnail" path
                // is a real case in the fixture rather than a hypothetical.
                let thumb = if i == 2 {
                    None
                } else {
                    Some(format!("/thumbs/{}.jpg", p.hash))
                };
                preview_photo(p, thumb)
            })
            .collect()
    }

    // --- the pages the engine produced, all of them ----------------------

    /// The named regression: **a blank page is skipped instead of rendered
    /// blank.** Ten of the 36 templates leave a full page empty, so a preview
    /// that drops empty pages shows a shorter, tidier book than the one that
    /// prints.
    #[test]
    fn preview_keeps_every_page_including_the_ones_that_hold_nothing() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });

        assert_eq!(layout.pages.len(), 4, "every page of the book is present");
        assert_eq!(layout.page_count, 4);
        let numbers: Vec<u32> = layout.pages.iter().map(|p| p.number).collect();
        assert_eq!(numbers, vec![1, 2, 3, 4], "no gap in the page sequence");
    }

    /// A page prints blank whether it got no group at all OR its template put
    /// every slot on the other half. Both are flagged; `template_id` is what
    /// still tells them apart.
    #[test]
    fn preview_flags_a_page_blank_for_a_text_zone_template_as_well_as_for_no_template() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });
        let blank: Vec<(&str, bool)> =
            layout.pages.iter().map(|p| (p.template_id.as_str(), p.blank)).collect();

        assert_eq!(
            blank,
            vec![
                ("12-quad-right:right", false),
                ("03-hero-left-text-right", false),
                // A real template id, and it still prints white.
                ("03-hero-left-text-right", true),
                (BLANK_TEMPLATE_ID, true),
            ]
        );
    }

    /// Page 1 is a RIGHT-hand page and the last is a LEFT-hand page: neither
    /// is half of a spread. The preview must carry the engine's own `side`
    /// rather than letting the webview infer it from position alone.
    #[test]
    fn preview_carries_the_side_of_every_page_so_the_singles_are_not_halves_of_a_spread() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });
        let sides: Vec<Side> = layout.pages.iter().map(|p| p.side).collect();

        assert_eq!(sides, vec![Side::Right, Side::Left, Side::Right, Side::Left]);
    }

    // --- the placements, against the manifest ----------------------------

    /// The named regression: **the preview disagrees with the manifest about
    /// which photo is in which slot.** The manifest is what the exported
    /// files are named from, so if these two ever diverge the screen shows one
    /// book and the printer receives another.
    ///
    /// Both are built here from the SAME `Book` and the SAME photo slice and
    /// checked entry for entry. The fixture's `photo_index`es are out of
    /// order, so a preview that walked placements positionally -- the obvious
    /// wrong implementation -- fails on page 2's two slots.
    #[test]
    fn preview_agrees_with_the_manifest_about_which_photo_is_in_which_slot() {
        let book = book();
        let photos = photos();
        let manifest = manifest(&book, &photos, 7);
        let layout = book_layout(7, &book, preview_photos(), &Library { spreads: Vec::new() });

        assert_eq!(layout.pages.len(), manifest.pages.len());
        for (page, manifest_page) in layout.pages.iter().zip(&manifest.pages) {
            assert_eq!(page.number, manifest_page.number);
            assert_eq!(page.template_id, manifest_page.template_id);
            assert_eq!(
                page.placements.len(),
                manifest_page.photos.len(),
                "page {} slot count",
                page.number
            );
            for (placement, entry) in page.placements.iter().zip(&manifest_page.photos) {
                let shown = &layout.photos[placement.photo_index];
                assert_eq!(
                    shown.path, entry.source_path,
                    "page {} z{} shows a different file than the manifest names",
                    page.number, placement.z
                );
                assert_eq!(shown.hash, entry.hash, "page {} z{} hash", page.number, placement.z);
                assert_eq!(placement.z, entry.z);
                assert_eq!(
                    placement.crop, entry.crop,
                    "page {} z{} crops must be the same window",
                    page.number, placement.z
                );
                // The name on screen is the name on disk. Both come from
                // `export::output_filename`; if the preview ever derived its
                // own, this is where the two would part company.
                assert_eq!(
                    placement.filename.as_deref(),
                    Some(entry.filename.as_str()),
                    "page {} z{} must name the file the exporter writes",
                    page.number,
                    placement.z
                );
            }
        }
    }

    /// The crop reaches the webview UNCHANGED. It is normalised in the
    /// photo's own frame, and any rescaling here would silently show a
    /// different part of the picture than the exporter writes.
    #[test]
    fn preview_passes_the_engines_crop_window_through_untouched() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });
        let page = &layout.pages[1];

        assert_eq!(page.placements[0].crop, Rect::new(0.0, 0.125, 1.0, 0.75));
        assert_eq!(
            page.placements[1].crop,
            Rect::new(0.2, 0.1, 0.5, 0.4),
            "a crop that is neither centred nor the full frame"
        );
        assert_eq!(page.placements[0].slot_rect, Rect::new(0.06, 0.08, 0.5, 0.62));
        assert_eq!(
            page.placements[1].slot_rect,
            Rect::new(0.6, 0.31, 0.34, 0.45),
            "each placement keeps its OWN box, not the first slot's"
        );
    }

    // --- counts -----------------------------------------------------------

    /// `placed_photos` sums across EVERY page (a bug that only counts the
    /// first page's slots passes on a one-page fixture), and `dropped_photos`
    /// is the engine's own figure rather than a recount.
    #[test]
    fn preview_counts_placements_across_every_page_and_reports_the_engines_dropped_figure() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });

        assert_eq!(layout.placed_photos, 3);
        assert_eq!(layout.dropped_photos, 2);
        assert_eq!(layout.photos.len(), 5, "every analysed photo, not only the placed ones");
        assert_eq!(layout.seed, 424_242);
        assert_eq!(layout.project_id, 7);
    }

    /// `photo_index` indexes the WHOLE analysed slice, so the preview must
    /// carry the unplaced photos too -- otherwise every index shifts and the
    /// "which ones were left out" answer has nothing to draw from.
    #[test]
    fn preview_photos_keep_their_original_indices_so_unplaced_ones_are_identifiable() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });
        let placed: std::collections::BTreeSet<usize> =
            layout.pages.iter().flat_map(|p| p.placements.iter().map(|s| s.photo_index)).collect();
        let left_out: Vec<&str> = layout
            .photos
            .iter()
            .enumerate()
            .filter(|(i, _)| !placed.contains(i))
            .map(|(_, p)| p.path.as_str())
            .collect();

        assert_eq!(left_out, vec!["/c.jpg", "/e.jpg"]);
        assert_eq!(left_out.len(), layout.dropped_photos);
    }

    /// A `photo_index` outside the photo list names no file rather than a
    /// plausible-looking wrong one. The engine never produces such an index;
    /// if one ever appeared, a preview confidently naming `p01-z1-` (an empty
    /// hash) would send the user looking for a file that was never written.
    #[test]
    fn preview_names_no_file_for_a_placement_pointing_outside_the_photo_list() {
        let mut book = book();
        book.pages[0].placements[0].photo_index = 99;

        let layout = book_layout(7, &book, preview_photos(), &Library { spreads: Vec::new() });

        assert_eq!(layout.pages[0].placements[0].filename, None);
        // The surviving placements still name theirs.
        assert_eq!(
            layout.pages[1].placements[0].filename.as_deref(),
            Some("p02-z1-hash-b")
        );
    }

    #[test]
    fn preview_photo_carries_the_oriented_dimensions_and_the_thumbnail() {
        let photos = preview_photos();

        assert_eq!(photos[1].width, 3024, "a portrait photo keeps its oriented shape");
        assert_eq!(photos[1].height, 4032);
        assert_eq!(photos[1].thumbnail_path.as_deref(), Some("/thumbs/hash-b.jpg"));
        assert_eq!(photos[2].thumbnail_path, None, "a failed thumbnail write is not a lost photo");
    }

    /// The picker sorts by these, so they come off the analysed photo rather
    /// than being guessed in the webview.
    #[test]
    fn preview_photo_carries_the_aesthetic_score_and_the_capture_time() {
        let photos = preview_photos();

        assert_eq!(photos.iter().map(|p| p.aesthetic_pct).collect::<Vec<_>>(), vec![62, 18, 91, 40, 77]);
        assert_eq!(photos[1].captured_at, Some(1_700_000_100));
        assert_eq!(photos[2].captured_at, None);
    }

    // --- the guides -------------------------------------------------------

    /// The named regression: **a guide is drawn at the wrong place.** The
    /// preview's guides must be the engine's own constants, not literals
    /// retyped beside them -- a retyped constant is a guide that can drift
    /// away from the predicate the scorer enforced while still looking
    /// authoritative.
    ///
    /// Asserted two ways on purpose. Against `geometry::` catches a hand-typed
    /// literal here; against the INCH arithmetic catches the case where both
    /// this and `geometry.rs` were edited together to a wrong value.
    #[test]
    fn preview_geometry_is_read_from_the_engines_own_constants() {
        let g = preview_geometry();

        assert_eq!(g.page_w_in, PAGE_W_IN);
        assert_eq!(g.page_h_in, PAGE_H_IN);
        assert_eq!(g.trim_u, TRIM_U);
        assert_eq!(g.trim_v, TRIM_V);
        assert_eq!(g.safe_u, SAFE_U);
        assert_eq!(g.safe_v, SAFE_V);
        assert_eq!(g.gutter_u, GUTTER_U);

        assert!((g.trim_u - BLEED_IN / PAGE_W_IN).abs() < 1e-12, "trim_u was {}", g.trim_u);
        assert!((g.trim_v - BLEED_IN / PAGE_H_IN).abs() < 1e-12, "trim_v was {}", g.trim_v);
        assert!(
            (g.safe_u - SAFE_MARGIN_IN / PAGE_W_IN).abs() < 1e-12,
            "safe_u was {}",
            g.safe_u
        );
        assert!(
            (g.safe_v - SAFE_MARGIN_IN / PAGE_H_IN).abs() < 1e-12,
            "safe_v was {}",
            g.safe_v
        );
        assert!((g.gutter_u - BLEED_IN / PAGE_W_IN).abs() < 1e-12, "gutter_u was {}", g.gutter_u);
    }

    /// `trim_u` and `gutter_u` are numerically equal but conceptually
    /// unrelated (`geometry.rs` says so where it defines them), and `safe_u`
    /// must be a DIFFERENT number from both -- a preview that wired the safe
    /// margin to the trim inset would draw two guides on one line and look
    /// entirely plausible doing it.
    #[test]
    fn preview_geometry_keeps_the_safe_margin_distinct_from_the_trim_inset() {
        let g = preview_geometry();

        assert!(
            (g.safe_u - g.trim_u).abs() > 1e-6,
            "safe_u {} must not be the trim inset {}",
            g.safe_u,
            g.trim_u
        );
        assert!(g.safe_u < g.trim_u, "1/8\" is smaller than 5mm: {} vs {}", g.safe_u, g.trim_u);
    }

    // --- serialisation ----------------------------------------------------

    /// The webview reads these keys by name. Pinned here as well as in the
    /// wire fixture because `Side` and `Rect` are shared with the persisted
    /// `book_json`, where the spelling must NOT change.
    #[test]
    fn preview_serialises_camel_case_keys_and_lowercase_sides() {
        let layout = book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() });
        let value = serde_json::to_value(&layout).unwrap();

        assert_eq!(value["placedPhotos"], 3);
        assert_eq!(value["droppedPhotos"], 2);
        assert_eq!(value["pageCount"], 4);
        assert_eq!(value["pages"][0]["side"], "right");
        assert_eq!(value["pages"][3]["side"], "left");
        assert_eq!(value["pages"][0]["templateId"], "12-quad-right:right");
        assert_eq!(value["pages"][1]["placements"][1]["photoIndex"], 0);
        assert_eq!(value["pages"][1]["placements"][1]["slotRect"]["w"], 0.34);
        assert_eq!(value["pages"][1]["placements"][1]["crop"]["x"], 0.2);
        assert_eq!(value["photos"][0]["thumbnailPath"], "/thumbs/hash-a.jpg");
        assert_eq!(value["photos"][2]["thumbnailPath"], serde_json::Value::Null);
        assert!(value["geometry"]["gutterU"].is_number());
    }

    /// The Rust half of the wire pin -- see `tests/fixtures/wire/README.md`
    /// and the `wire fixtures` section of `commands.rs`. This one lives here
    /// rather than there so it can reuse the book fixture above; the contract
    /// is identical. `tests/preview.test.ts` feeds the SAME file through the
    /// real preview functions, so a rename on either side fails one suite
    /// against a fixture that did not move.
    #[test]
    fn book_layout_serialises_exactly_the_keys_the_preview_reads() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire/book-layout.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing wire fixture {path:?}: {e}"));
        let fixture: serde_json::Value =
            serde_json::from_str(&text).expect("wire fixture must be valid JSON");

        // Serialised to TEXT and parsed back, rather than `to_value`d
        // directly. `PreviewGeometry` is the first wire type in this project
        // to carry an irrational f64 (`0.197 / 11.197` and friends), and
        // serde_json's default float parser is the fast approximate one --
        // `float_roundtrip` is not enabled -- so reading the fixture back
        // lands up to one ULP from the constant that wrote it. Putting BOTH
        // sides through the same parse compares the JSON TEXT, which is what
        // this fixture exists to pin; a real drift is orders of magnitude
        // larger than one ULP and still fails.
        let serialised =
            serde_json::to_string(&book_layout(7, &book(), preview_photos(), &Library { spreads: Vec::new() })).unwrap();
        let value: serde_json::Value = serde_json::from_str(&serialised).unwrap();

        assert_eq!(value, fixture);
    }
}
