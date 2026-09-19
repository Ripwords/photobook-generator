//! The export manifest: a durable record of exactly what was placed where,
//! independent of whether the sidecar export that reads it has run yet.
//!
//! Built from the same `Book`/`Photo` inputs `export::build_items` consumes,
//! and using the SAME filename builder, so a manifest entry and the
//! `ExportItem` the sidecar actually receives always name the same file --
//! there is no second place this pairing could drift out of step.

use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::export::{output_filename, predicted_format};
use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

/// One exported photo's record within a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestPhoto {
    pub source_path: String,
    pub hash: String,
    /// The page this photo sits on -- carried directly on the photo record
    /// (as well as being implied by nesting under `ManifestPage`) so a
    /// consumer that flattens the manifest never has to reconstruct it from
    /// position.
    pub page_number: u32,
    pub z: u32,
    /// Crop window in the photo's own normalised coordinates -- the same
    /// space `Placement::crop` and `ExportItem`'s `crop_*` fields use.
    pub crop: Rect,
    /// Where the photo lands on the printed PAGE, in real inches
    /// (the book's own page size). Deliberately NOT the raw normalised
    /// `Placement::slot_rect` -- a manifest recording normalised units would
    /// silently misreport where the photo actually prints, since normalised
    /// coordinates on the page canvas are not directly comparable to any
    /// physical measurement Pixajoy or the user would recognise.
    pub dest_rect_in: Rect,
    /// The output basename the sidecar will write this photo under, without
    /// an extension -- identical to the `filename` on the matching
    /// `ExportItem`.
    pub filename: String,
    /// Predicted output container (`"jpg"` or `"png"`); see
    /// `export::predicted_format` for why this is a prediction, not a fact,
    /// until the sidecar's own `ExportRecord` confirms it.
    pub format: String,
}

/// One page of the book, always present even when it holds nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestPage {
    pub number: u32,
    pub template_id: String,
    /// Empty for a blank page or a page whose group failed to lay out --
    /// NEVER omitted. Dropping empty pages from this list would make the
    /// manifest's page count stop matching `Book::pages.len()`, and the SKU
    /// fixes the page count, so a manifest that silently drops a page cannot
    /// be reconciled against what was actually ordered.
    pub photos: Vec<ManifestPhoto>,
}

/// The whole book's export manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub project_id: i64,
    pub seed: u64,
    /// Book length. Always equals `pages.len()`; kept as its own field
    /// (rather than making a reader count the list) because it is the
    /// number a consumer reconciles against the SKU.
    pub page_count: usize,
    pub pages: Vec<ManifestPage>,
}

/// Builds the manifest for `book`. `photos` is the ORIGINAL slice
/// `Placement::photo_index` indexes into, matching `export::build_items`'s
/// own contract.
pub fn manifest(book: &Book, photos: &[Photo], project_id: i64) -> Manifest {
    // The book's own geometry, never a default: a manifest is the hand-off to
    // the printer, so naming the wrong page size here is the one bug nothing
    // downstream can catch.
    let spec = &book.spec;
    let pages = book
        .pages
        .iter()
        .map(|page| ManifestPage {
            number: page.number,
            template_id: page.template_id.clone(),
            photos: page
                .placements
                .iter()
                .map(|placement| {
                    let photo = &photos[placement.photo_index];
                    ManifestPhoto {
                        source_path: photo.path.clone(),
                        hash: photo.hash.clone(),
                        page_number: page.number,
                        z: placement.z,
                        crop: placement.crop,
                        dest_rect_in: Rect::new(
                            placement.slot_rect.x * spec.page_w_in(),
                            placement.slot_rect.y * spec.page_h_in(),
                            placement.slot_rect.w * spec.page_w_in(),
                            placement.slot_rect.h * spec.page_h_in(),
                        ),
                        filename: output_filename(page.number, placement.z, &photo.hash),
                        format: predicted_format(&photo.path).to_string(),
                    }
                })
                .collect(),
        })
        .collect();

    Manifest { project_id, seed: book.seed, page_count: book.pages.len(), pages }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::Side;
    use crate::export::build_items;
    use crate::print_spec::{odd_spec, pixajoy_spec};

    fn photo(path: &str, hash: &str) -> Photo {
        Photo {
            path: path.into(),
            hash: hash.into(),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
        }
    }

    fn placement(photo_index: usize, z: u32, slot_rect: Rect, crop: Rect) -> Placement {
        Placement { photo_index, slot_rect, crop, z }
    }

    #[test]
    fn manifest_carries_the_project_id_seed_and_page_count() {
        let photos = vec![photo("/a.jpg", "haaa1111")];
        let book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            controls: Default::default(),
            seed: 4242,
            dropped: 0,
            pages: vec![Page {
                number: 1,
                side: Side::Right,
                template_id: "t".into(),
                placements: vec![placement(
                    0,
                    1,
                    Rect::new(0.0, 0.0, 0.4, 0.4),
                    Rect::new(0.0, 0.0, 1.0, 1.0),
                )],
            }],
        };
        let m = manifest(&book, &photos, 77);
        assert_eq!(m.project_id, 77);
        assert_eq!(m.seed, 4242);
        assert_eq!(m.page_count, 1);
        assert_eq!(m.page_count, m.pages.len());
    }

    /// Regression: a page with zero placements must still appear in the
    /// manifest, with an empty photo list -- not be filtered out. Otherwise
    /// `page_count` (or a reader counting `pages.len()`) stops reconciling
    /// against the SKU's fixed page count.
    #[test]
    fn manifest_records_a_page_with_zero_placements_as_an_empty_entry() {
        let photos = vec![photo("/a.jpg", "haaa1111")];
        let book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![
                Page { number: 1, side: Side::Right, template_id: "blank".into(), placements: Vec::new() },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "t".into(),
                    placements: vec![placement(
                        0,
                        1,
                        Rect::new(0.0, 0.0, 0.4, 0.4),
                        Rect::new(0.0, 0.0, 1.0, 1.0),
                    )],
                },
            ],
        };
        let m = manifest(&book, &photos, 1);
        assert_eq!(m.page_count, 2, "book has 2 pages, manifest must report 2");
        assert_eq!(m.pages.len(), 2, "the blank page must not be dropped from the list");
        assert_eq!(m.pages[0].number, 1);
        assert!(m.pages[0].photos.is_empty(), "the blank page has no photos, but is still present");
        assert_eq!(m.pages[1].photos.len(), 1);
    }

    /// R11. The destination rect must be converted to real inches on the
    /// BOOK'S OWN page canvas, never left as the raw normalised `slot_rect`
    /// and never converted against Pixajoy's page.
    ///
    /// Runs under `odd_spec` (8.0 x 10.0), not the default: under Pixajoy's
    /// numbers a hardcoded `11.197` is indistinguishable from reading
    /// `spec.page_w_in()`. It is also PORTRAIT, so transposing the two axes
    /// is visible here too.
    ///
    /// `slot_rect`'s components are all NOT 0 or 1, so a bug that forgets the
    /// multiplication cannot coincidentally still pass.
    #[test]
    fn manifest_dest_rect_is_in_the_books_own_page_inches() {
        let photos = vec![photo("/a.jpg", "haaa1111")];
        let slot_rect = Rect::new(0.1, 0.2, 0.3, 0.4);
        let spec = odd_spec();
        let book = Book {
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            spec,
            cover: Default::default(),
            pages: vec![Page {
                number: 1,
                side: Side::Right,
                template_id: "t".into(),
                placements: vec![placement(0, 1, slot_rect, Rect::new(0.0, 0.0, 1.0, 1.0))],
            }],
        };
        let m = manifest(&book, &photos, 1);
        let dest = m.pages[0].photos[0].dest_rect_in;

        assert!((dest.x - slot_rect.x * 8.0).abs() < 1e-9, "x was {}", dest.x);
        assert!((dest.y - slot_rect.y * 10.0).abs() < 1e-9, "y was {}", dest.y);
        assert!((dest.w - slot_rect.w * 8.0).abs() < 1e-9, "w was {}", dest.w);
        assert!((dest.h - slot_rect.h * 10.0).abs() < 1e-9, "h was {}", dest.h);

        // The same book under Pixajoy's page must give different numbers, or
        // this fixture cannot tell "reads the spec" from "hardcodes 11.197".
        let pixajoy = Book { spec: pixajoy_spec(), ..book };
        let other = manifest(&pixajoy, &photos, 1).pages[0].photos[0].dest_rect_in;
        assert!((other.w - dest.w).abs() > 0.1, "the two specs must be distinguishable");

        // Sanity: the page is not 1" square, so a bug that skips the
        // conversion (returns the raw normalised rect) is distinguishable
        // from a correct implementation on this fixture.
        assert!(
            (dest.w - slot_rect.w).abs() > 1.0,
            "fixture must distinguish inches from normalised units"
        );
    }

    #[test]
    fn manifest_records_source_path_hash_crop_and_z_per_photo() {
        let photos = vec![photo("/photos/a.jpg", "deadbeef99")];
        let crop = Rect::new(0.05, 0.1, 0.6, 0.7);
        let book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![Page {
                number: 3,
                side: Side::Right,
                template_id: "t".into(),
                placements: vec![placement(0, 2, Rect::new(0.0, 0.0, 0.4, 0.4), crop)],
            }],
        };
        let m = manifest(&book, &photos, 1);
        let p = &m.pages[0].photos[0];
        assert_eq!(p.source_path, "/photos/a.jpg");
        assert_eq!(p.hash, "deadbeef99");
        assert_eq!(p.page_number, 3);
        assert_eq!(p.z, 2);
        assert_eq!(p.crop, crop);
        assert_eq!(p.format, "jpg");
    }

    /// The manifest's filenames must be IDENTICAL to what `build_items`
    /// sends to the sidecar for the same book -- otherwise a consumer
    /// correlating an `ExportRecord`'s path back to the manifest (e.g. to
    /// show "photo X printed at file Y") would look up the wrong entry.
    #[test]
    fn manifest_filenames_match_build_items_filenames_for_the_same_book() {
        let photos = vec![
            photo("/a.jpg", "haaa1111"),
            photo("/b.jpg", "hbbb2222"),
            photo("/c.jpg", "hccc3333"),
        ];
        let book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            controls: Default::default(),
            seed: 9,
            dropped: 0,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Right,
                    template_id: "t".into(),
                    placements: vec![
                        placement(0, 1, Rect::new(0.0, 0.0, 0.4, 0.4), Rect::new(0.0, 0.0, 1.0, 1.0)),
                        placement(1, 2, Rect::new(0.5, 0.0, 0.4, 0.4), Rect::new(0.0, 0.0, 1.0, 1.0)),
                    ],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "t".into(),
                    placements: vec![placement(
                        2,
                        1,
                        Rect::new(0.0, 0.0, 0.4, 0.4),
                        Rect::new(0.0, 0.0, 1.0, 1.0),
                    )],
                },
            ],
        };

        let items = build_items(&book, &photos);
        let m = manifest(&book, &photos, 1);
        let manifest_filenames: Vec<&str> =
            m.pages.iter().flat_map(|p| p.photos.iter().map(|ph| ph.filename.as_str())).collect();
        let item_filenames: Vec<&str> = items.iter().map(|i| i.filename.as_str()).collect();

        assert_eq!(manifest_filenames, item_filenames);
    }
}
