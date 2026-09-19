//! A project makes a `Book` durable: reopenable, regenerable and
//! re-exportable without re-analysing the source folder. Phase 3 renders a
//! project; Phase 4 edits one. This module only has to capture the data
//! those phases need.
//!
//! ## Storage decision: metadata columns plus one `book_json` column
//!
//! `projects` stores the fields a project *list* needs to display or sort
//! by -- name, source folder, page count, photo count, created/updated
//! timestamps -- as real columns, and the whole `Book` (every page, every
//! placement, the seed, the dropped count) as one serialised `book_json`
//! TEXT column. It does NOT use normalised `project_pages` /
//! `project_placements` tables.
//!
//! Why: `Book`, `Page`, `Placement` and `Rect` already round-trip through
//! serde (see `book::pace`), so persisting one is `serde_json::to_string`
//! and loading it back is `serde_json::from_str` -- not a hand-written row
//! mapper for three nested types. Listing projects only ever needs the
//! denormalised columns above; it never touches a placement. The one
//! consumer that would benefit from updating a single placement without
//! rewriting the whole book is Phase 4 (editing), which is two phases away
//! and unspecced -- normalising now means designing a schema against
//! guessed requirements instead of Phase 4's actual ones. SQLite makes
//! adding `project_pages` / `project_placements` tables later cheap, so if
//! Phase 4 does need per-placement updates, that migration can be scoped
//! against what Phase 4 actually turns out to require.

use crate::book::cull::Overrides;
use crate::book::pace::Book;
use serde::{Deserialize, Serialize};

/// A saved book: everything needed to reopen, regenerate or re-export it
/// without re-running analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    /// The first of `source_folders`, kept as its own column for the list's
    /// label and for rows saved before a book could draw from several.
    pub source_folder: String,
    /// Every folder the book was analysed from, in the order the user picked
    /// them. Re-analysing exactly this list is what "edit the selection" on a
    /// reopened project does.
    pub source_folders: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub book: Book,
    /// Content hashes of the photo set the book is defined over, in the
    /// SAME ORDER as the slice `pace::assemble` was given -- so
    /// `Placement::photo_index` indexes straight into this list (spec 5.4a:
    /// "The culled photo set, by content hash").
    ///
    /// This is what makes a project genuinely durable. Without it, exporting
    /// a saved book required the webview to hand back the same photo array
    /// it happened to still be holding, in the same order: the app could not
    /// export a project after a restart, and a re-analysis that produced a
    /// same-length-but-different set would have exported the old book's
    /// crops against the new photos, with no error anywhere. Every hash
    /// resolves back to a full feature record through the `features` cache,
    /// which is keyed by exactly this hash.
    ///
    /// **Every photo, not only the placed ones.** `Placement::photo_index`
    /// indexes the whole slice `assemble` received (see `pace.rs`), so a
    /// list holding only the survivors would renumber every index and change
    /// which photo each placement points at. Storing the full ordered list
    /// keeps `Placement` -- and the goldens built on it -- untouched.
    ///
    /// A hash may legitimately repeat: two byte-identical files in one
    /// folder hash the same, and both are real placements of what is, in
    /// pixels, the same photo.
    pub photo_hashes: Vec<String>,
    /// The user's own include/exclude decisions, keyed by content hash --
    /// what they asked for that differs from what the engine would have
    /// chosen on its own.
    ///
    /// Persisted with the project rather than derived, because it CANNOT be
    /// derived: the decisions are made on the contact sheet, before a project
    /// exists, and nothing about the saved book records why a photo is in it.
    /// A project that reopened without them would quietly revert every
    /// decision to `Auto` and look entirely correct while doing it.
    ///
    /// Unordered, unlike `photo_hashes`: this is a map from hash to state and
    /// nothing indexes into it positionally.
    pub overrides: Overrides,
    pub exports: Vec<ExportRecord>,
}

/// One export of a project: what was written, where, and when.
///
/// `at` is set by the database, not the caller: `Db::record_export` always
/// stamps `exported_at` with SQLite's `unixepoch()` (matching the
/// convention `db.rs` already uses for `created_at`/`updated_at`), so
/// whatever value `at` holds when passed into `record_export` is ignored.
/// A record read back via `Db::load_project` carries the real persisted
/// timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRecord {
    pub at: i64,
    pub output_dir: String,
    pub format: String,
    pub file_count: usize,
}

/// The columns a project list needs to display or sort by, without paying
/// for `book_json` deserialisation.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub source_folder: String,
    pub source_folders: Vec<String>,
    pub page_count: i64,
    pub photo_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub favourite: bool,
}

/// Page count and photo count, denormalised from a `Book` into the
/// `projects` columns so listing never needs to parse `book_json`. Photo
/// count is the number of placements across the whole book, summed over
/// every page -- the packer never places the same photo twice (see
/// `book::pack`), so this also equals the number of distinct photos used.
pub(crate) fn book_counts(book: &Book) -> (i64, i64) {
    let page_count = book.pages.len() as i64;
    let photo_count: i64 = book.pages.iter().map(|p| p.placements.len() as i64).sum();
    (page_count, photo_count)
}

/// The photos a library cover shows: the first `limit` distinct photos in
/// reading order, page by page and lowest `z` first within a page, as
/// indices into the book's photo slice. A photo placed twice counts once.
pub(crate) fn cover_photo_indices(book: &Book, limit: usize) -> Vec<usize> {
    let mut picked: Vec<usize> = Vec::with_capacity(limit);
    for page in &book.pages {
        let mut placements: Vec<_> = page.placements.iter().collect();
        placements.sort_by_key(|p| p.z);
        for placement in placements {
            if picked.len() == limit {
                return picked;
            }
            if !picked.contains(&placement.photo_index) {
                picked.push(placement.photo_index);
            }
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::{Rect, Side};
    use crate::print_spec::pixajoy_spec;

    fn empty_book() -> Book {
        Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 0,
            dropped: 0,
            pages: vec![],
        }
    }

    fn placement(photo_index: usize, z: u32) -> Placement {
        Placement {
            photo_index,
            slot_rect: Rect::new(0.0, 0.0, 0.1, 0.1),
            crop: Rect::new(0.0, 0.0, 1.0, 1.0),
            z,
        }
    }

    #[test]
    fn book_counts_sums_placements_across_every_page_not_just_the_first() {
        let book = Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Right,
                    template_id: "a".into(),
                    placements: vec![placement(0, 1)],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "b".into(),
                    placements: vec![placement(1, 1), placement(2, 2)],
                },
            ],
        };

        let (pages, photos) = book_counts(&book);

        assert_eq!(pages, 2);
        assert_eq!(photos, 3);
    }

    #[test]
    fn cover_photos_are_the_first_distinct_photos_in_reading_order() {
        let page = |number, placements| Page {
            number,
            side: Side::Right,
            template_id: "t".into(),
            placements,
        };
        let book = Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![
                // Stacked out of order on purpose: z, not vector order, is reading order.
                page(1, vec![placement(7, 2), placement(3, 1)]),
                page(2, vec![placement(3, 1), placement(5, 2)]),
                page(3, vec![placement(9, 1), placement(1, 2)]),
            ],
        };

        assert_eq!(cover_photo_indices(&book, 4), vec![3, 7, 5, 9]);
        assert_eq!(cover_photo_indices(&book, 2), vec![3, 7]);
        assert_eq!(
            cover_photo_indices(&empty_book(), 4),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn book_counts_of_an_empty_book_is_zero_and_zero() {
        assert_eq!(book_counts(&empty_book()), (0, 0));
    }
}
