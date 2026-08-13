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

use crate::book::pace::Book;
use serde::{Deserialize, Serialize};

/// A saved book: everything needed to reopen, regenerate or re-export it
/// without re-running analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub source_folder: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub book: Book,
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
    pub page_count: i64,
    pub photo_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::{Rect, Side};

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
    fn book_counts_of_an_empty_book_is_zero_and_zero() {
        let book = Book { seed: 0, dropped: 0, pages: vec![] };
        assert_eq!(book_counts(&book), (0, 0));
    }
}
