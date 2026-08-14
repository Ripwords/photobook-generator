//! Builds the sidecar's `.export` payload from an assembled `Book`.
//!
//! This module owns the mapping from a `Placement` (page-normalised rects,
//! photo index) to an `ExportItem` (source path, crop window, output
//! basename) -- the wire shape itself lives in `protocol.rs`, matched
//! field-for-field against Swift's `ExportItem`/`ExportRequest`.

use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::protocol::ExportItem;

/// How many hex characters of a photo's content hash go into its output
/// filename. Enough to make a collision between two DIFFERENT source photos
/// astronomically unlikely, while keeping filenames short; uniqueness within
/// one export does not depend on this at all -- `output_filename` is already
/// unique by page number and z order alone (see its doc comment).
const HASH_PREFIX_LEN: usize = 8;

fn hash_prefix(hash: &str) -> &str {
    &hash[..hash.len().min(HASH_PREFIX_LEN)]
}

/// The output basename for one placement, WITHOUT an extension -- the
/// sidecar appends the one that matches the format it chooses from the
/// source file itself (see `ExportItem`'s doc comment in `protocol.rs`).
///
/// Unique across a whole book by construction: `page_number` is unique per
/// page (`pace::assemble` numbers pages 1..=N once, consecutively) and `z`
/// is unique within a page (`pace::place` assigns it 1-based and distinct),
/// so the pair is unique across every placement in the book. The hash
/// prefix is not load-bearing for uniqueness -- it exists so a human
/// scanning the output directory can tell which source photo a file came
/// from -- but see
/// `build_items_produces_distinct_filenames_across_a_book_with_many_pages_and_placements`
/// below, which pins the actual guarantee with a test rather than resting on
/// this paragraph.
///
/// The page number is zero-padded to two digits so that filenames SORT
/// lexicographically in page order: unpadded, page 9 (`"p9-..."`) would sort
/// after page 10 (`"p10-..."`) because `'9' > '1'` byte-for-byte. Two digits
/// is never truncated by a real page number -- Pixajoy's largest published
/// SKU is 40 pages (see the design spec's SKU table).
pub(crate) fn output_filename(page_number: u32, z: u32, hash: &str) -> String {
    format!("p{:02}-z{}-{}", page_number, z, hash_prefix(hash))
}

/// Predicts the container the sidecar's `Exporter.outputFormat` will choose
/// for a source file, from its extension alone.
///
/// This is informational-only, used by `book::manifest::manifest` to record
/// an expected format before the sidecar has actually run. It cannot be
/// exact: Swift's real rule sniffs the file's ACTUAL container via ImageIO
/// (a `.jpg`-named file that is secretly a renamed PNG would decode as PNG
/// there), and only falls back to the extension when ImageIO cannot name the
/// container. Rust has no ImageIO, so this always takes that fallback path --
/// which mirrors the Swift sidecar's own degradation, not a divorced guess.
/// The authority for what actually got written is always the `ExportRecord`
/// the sidecar returns, never this manifest field.
///
/// Mirrors `Exporter.outputFormat`'s rule: lossy source (JPEG, HEIC/HEIF)
/// exports as JPEG; everything else (PNG, TIFF, RAW, unrecognised) exports
/// as PNG, since PNG never adds a generation of loss the source did not
/// already have.
pub(crate) fn predicted_format(source_path: &str) -> &'static str {
    let ext = std::path::Path::new(source_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "heic" | "heif" => "jpg",
        _ => "png",
    }
}

/// Builds one `ExportItem` per placement across the whole book, in page
/// order and z order within a page -- the same order `pace::place` already
/// lays placements in, so no separate sort is needed here. `photos` is the
/// ORIGINAL slice `Placement::photo_index` indexes into (see `pace.rs`'s doc
/// comment on `photo_index`), not the culled subset.
pub fn build_items(book: &Book, photos: &[Photo]) -> Vec<ExportItem> {
    book.pages
        .iter()
        .flat_map(|page| {
            page.placements.iter().map(move |placement| {
                let photo = &photos[placement.photo_index];
                ExportItem {
                    source_path: photo.path.clone(),
                    filename: output_filename(page.number, placement.z, &photo.hash),
                    crop_x: placement.crop.x,
                    crop_y: placement.crop.y,
                    crop_w: placement.crop.w,
                    crop_h: placement.crop.h,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::{Rect, Side};
    use std::collections::HashSet;

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
        }
    }

    fn placement(photo_index: usize, z: u32, crop: Rect) -> Placement {
        Placement { photo_index, slot_rect: Rect::new(0.0, 0.0, 0.4, 0.4), crop, z }
    }

    /// A page whose placements point at exactly the given `indices`, one
    /// placement per index, z-ordered 1-based in the order given -- the
    /// caller controls which photo indices are reused, so callers that need
    /// distinct photos per placement pass distinct indices.
    fn page_with_placements(number: u32, side: Side, indices: &[usize]) -> Page {
        Page {
            number,
            side,
            template_id: "t".into(),
            placements: indices
                .iter()
                .enumerate()
                .map(|(i, &photo_index)| {
                    placement(photo_index, i as u32 + 1, Rect::new(0.1, 0.1, 0.5, 0.5))
                })
                .collect(),
        }
    }

    // --- output_filename: no extension, unique, sortable ------------------

    #[test]
    fn output_filename_has_no_extension() {
        let name = output_filename(4, 1, "abcd1234ef");
        assert!(!name.contains('.'), "filename must carry no extension: {name}");
    }

    #[test]
    fn output_filename_zero_pads_the_page_number_to_two_digits() {
        assert_eq!(output_filename(4, 1, "abcd1234"), "p04-z1-abcd1234");
        assert_eq!(output_filename(12, 3, "abcd1234"), "p12-z3-abcd1234");
    }

    #[test]
    fn output_filename_truncates_the_hash_to_eight_characters() {
        let name = output_filename(1, 1, "abcdef0123456789");
        assert_eq!(name, "p01-z1-abcdef01");
    }

    #[test]
    fn output_filename_tolerates_a_hash_shorter_than_the_prefix_length() {
        let name = output_filename(1, 1, "ab");
        assert_eq!(name, "p01-z1-ab");
    }

    // --- build_items: shape and ordering -----------------------------------

    #[test]
    fn build_items_produces_one_item_per_placement_in_book_order() {
        let photos = vec![photo("/a.jpg", "haaa1111"), photo("/b.jpg", "hbbb2222")];
        let book = Book {
            seed: 1,
            dropped: 0,
            pages: vec![page_with_placements(1, Side::Right, &[0, 1])],
        };
        let items = build_items(&book, &photos);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].source_path, "/a.jpg");
        assert_eq!(items[0].filename, "p01-z1-haaa1111");
        assert_eq!(items[1].source_path, "/b.jpg");
        assert_eq!(items[1].filename, "p01-z2-hbbb2222");
    }

    #[test]
    fn build_items_copies_the_placement_crop_unconverted() {
        let photos = vec![photo("/a.jpg", "haaa1111")];
        let crop = Rect::new(0.05, 0.1, 0.6, 0.7);
        let book = Book {
            seed: 1,
            dropped: 0,
            pages: vec![Page {
                number: 1,
                side: Side::Right,
                template_id: "t".into(),
                placements: vec![placement(0, 1, crop)],
            }],
        };
        let items = build_items(&book, &photos);
        assert_eq!(items[0].crop_x, crop.x);
        assert_eq!(items[0].crop_y, crop.y);
        assert_eq!(items[0].crop_w, crop.w);
        assert_eq!(items[0].crop_h, crop.h);
    }

    #[test]
    fn build_items_skips_pages_with_no_placements() {
        let photos = vec![photo("/a.jpg", "haaa1111")];
        let book = Book {
            seed: 1,
            dropped: 0,
            pages: vec![
                Page { number: 1, side: Side::Right, template_id: "blank".into(), placements: Vec::new() },
                page_with_placements(2, Side::Left, &[0]),
            ],
        };
        let items = build_items(&book, &photos);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].filename, "p02-z1-haaa1111");
    }

    // --- the regression this task specifically has to pin ------------------

    /// The contract carried from Task 3's review: filenames come from
    /// `page.number` and `placement.z`, unique by construction (unique page
    /// numbers, distinct z within a page) -- but that guarantee is exactly
    /// the kind that quietly stops holding if the naming scheme ever
    /// changes. A book with MANY pages and MULTIPLE placements per page, so
    /// both axes of the "unique by construction" claim are actually
    /// exercised (a single-placement-per-page fixture couldn't tell a z
    /// collision from a page collision).
    #[test]
    fn build_items_produces_distinct_filenames_across_a_book_with_many_pages_and_placements() {
        // Every photo on a page shares the SAME hash prefix (`"dupe"`, cycled
        // over just 3 distinct hashes across the whole book). If uniqueness
        // is genuinely coming from page number and z -- not incidentally
        // from the hash varying per photo -- collapsing the hash pool this
        // hard must not produce a single collision. A fixture that instead
        // gives every photo its OWN hash cannot tell "page+z is unique" from
        // "the hash happened to be unique": dropping z from the filename
        // scheme would still pass such a fixture, because the hash alone
        // would still disambiguate every file.
        let mut photos = Vec::new();
        let mut pages = Vec::new();
        let mut next_index = 0usize;
        for page_number in 1..=20u32 {
            let placements_on_page = 1 + (page_number as usize % 4); // 1..=4
            let mut indices = Vec::new();
            for _ in 0..placements_on_page {
                let hash = format!("hash{:04}", next_index % 3);
                photos.push(photo(&format!("/p{next_index}.jpg"), &hash));
                indices.push(next_index);
                next_index += 1;
            }
            pages.push(page_with_placements(
                page_number,
                if page_number % 2 == 1 { Side::Right } else { Side::Left },
                &indices,
            ));
        }
        let book = Book { seed: 1, dropped: 0, pages };

        let items = build_items(&book, &photos);
        assert!(items.len() > 40, "fixture must actually exercise many placements: {}", items.len());
        // Sanity: the hash pool really did collapse to 3 distinct values, so
        // the uniqueness below cannot be riding on the hash alone.
        let distinct_hashes: HashSet<&str> = photos.iter().map(|p| p.hash.as_str()).collect();
        assert_eq!(distinct_hashes.len(), 3, "fixture must collapse hashes, not vary them per photo");

        let names: Vec<&str> = items.iter().map(|i| i.filename.as_str()).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "every filename in one export request must be distinct: {names:?}"
        );
    }

    /// Sorting the emitted filenames lexicographically must reproduce the
    /// book's own page-then-z traversal order. Pins the zero-padding on the
    /// page number: without it, `"p9-..."` sorts AFTER `"p10-..."` because
    /// `'9' > '1'` byte-for-byte, which is exactly backwards.
    #[test]
    fn build_items_filenames_sort_lexicographically_into_page_then_z_order() {
        let mut photos = Vec::new();
        let mut pages = Vec::new();
        let mut next_index = 0usize;
        for page_number in 1..=15u32 {
            let mut indices = Vec::new();
            for _ in 0..2 {
                photos.push(photo(&format!("/p{next_index}.jpg"), &format!("hash{next_index:04}")));
                indices.push(next_index);
                next_index += 1;
            }
            pages.push(page_with_placements(page_number, Side::Right, &indices));
        }
        let book = Book { seed: 1, dropped: 0, pages };

        let items = build_items(&book, &photos);
        let traversal_order: Vec<&str> = items.iter().map(|i| i.filename.as_str()).collect();

        let mut sorted_order = traversal_order.clone();
        sorted_order.sort_unstable();

        assert_eq!(
            sorted_order, traversal_order,
            "lexicographic sort must reproduce page-then-z traversal order"
        );
    }

    #[test]
    fn predicted_format_treats_jpeg_and_heic_as_lossy() {
        assert_eq!(predicted_format("/a/photo.JPG"), "jpg");
        assert_eq!(predicted_format("/a/photo.jpeg"), "jpg");
        assert_eq!(predicted_format("/a/photo.heic"), "jpg");
        assert_eq!(predicted_format("/a/photo.HEIF"), "jpg");
    }

    #[test]
    fn predicted_format_treats_everything_else_as_lossless() {
        assert_eq!(predicted_format("/a/photo.png"), "png");
        assert_eq!(predicted_format("/a/photo.tiff"), "png");
        assert_eq!(predicted_format("/a/photo.arw"), "png");
        assert_eq!(predicted_format("/a/photo"), "png");
    }
}
