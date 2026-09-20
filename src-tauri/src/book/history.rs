//! Undo and redo: what each edit is called, and what the controls say.
//!
//! The timeline itself is snapshots of the whole `Book`, held in
//! `book_history` beside the project (see `Db::commit_book`). Nothing here
//! touches the database; this is the naming, which is what the user reads.
//!
//! ## Why snapshots, and why not in the webview
//!
//! `book::edit::apply` is not invertible. A regenerate throws away the
//! template it replaced, a swap that trips a hard constraint leaves the book
//! untouched, and `Shuffle` rewrites every unlocked opening at once. So the
//! only honest "before" is the whole book, and the only place it exists is
//! Rust: `BookLayout`, the one thing the webview is ever given, omits
//! `Book::controls` entirely, so a layout the webview kept could not restore
//! what was locked or which templates were rejected.
//!
//! Snapshots are affordable here. A 40-page book measures around 17 KB of
//! JSON, so `CAP` of them is under a megabyte per project.

use crate::book::edit::{BookEdit, PlacementRef};
use crate::book::pace::Book;
use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

/// Which way along the timeline to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Undo,
    Redo,
}

/// What the Undo and Redo controls offer, or `None` where the timeline ends.
/// The strings are `edit_label`s, so the button reads "Undo resize".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatus {
    /// The edit stepping back would reverse.
    pub undo: Option<String>,
    /// The edit stepping forward would re-apply.
    pub redo: Option<String>,
}

/// What to call this edit on the Undo control, lower case so it reads
/// "Undo resize" under a capitalising label.
///
/// `before` is the book as it stands, so this must be asked BEFORE the edit
/// is applied. Only `SetSlot` reads it, and only to tell the two layout-mode
/// gestures apart: both emit one `SetSlot`, and the difference between them
/// is the size the slot had, which the edit does not carry.
///
/// Deliberately no catch-all arm: a new `BookEdit` variant must fail to
/// compile here rather than quietly join the pile under one vague word.
pub fn edit_label(edit: &BookEdit, before: &Book) -> &'static str {
    match edit {
        BookEdit::Regenerate { .. } => "regenerate",
        BookEdit::RejectTemplate { .. } => "reject layout",
        BookEdit::SetTemplate { .. } => "layout change",
        // The two read as opposites on the button, because undoing a lock
        // and undoing an unlock are opposite acts.
        BookEdit::SetLocked { locked: true, .. } => "lock",
        BookEdit::SetLocked { locked: false, .. } => "unlock",
        BookEdit::Shuffle => "shuffle",
        BookEdit::SwapPhotos { .. } => "swap",
        BookEdit::SetCrop { .. } => "crop",
        // A box the user only dragged keeps its size exactly: `slotMoved`
        // translates and returns the width and height it was given. So a
        // slot the book still has at this size was moved, and anything else
        // -- including a corner drag, which moves the box as well as
        // reshaping it -- is a resize. A slot the book does not have cannot
        // happen (the edit has not been applied, so it is about to fail) and
        // keeps the old wording.
        BookEdit::SetSlot { placement, rect } => {
            match slot_rect(before, *placement) {
                Some(was) if same_size(was, *rect) => "move",
                _ => "resize",
            }
        }
        BookEdit::ReplacePhoto { .. } => "photo replacement",
        BookEdit::SetPrintSpec { .. } => "print size change",
        BookEdit::SetCoverPhoto { photo: Some(_), .. } => "cover photo",
        BookEdit::SetCoverPhoto { photo: None, .. } => "cover photo removal",
        BookEdit::SetCoverCrop { .. } => "cover crop",
        BookEdit::SetSpineColour { .. } => "spine colour",
    }
}

/// The slot `at` names, or `None` if the book has no such placement.
fn slot_rect(book: &Book, at: PlacementRef) -> Option<Rect> {
    book.pages
        .iter()
        .find(|page| page.number == at.page)?
        .placements
        .iter()
        .find(|placement| placement.z == at.z)
        .map(|placement| placement.slot_rect)
}

/// Whether two rects are the same size. Tight on purpose: the webview sends
/// back the very `f64`s it was given for a move, and a difference too small
/// to see is not a resize worth naming.
fn same_size(a: Rect, b: Rect) -> bool {
    (a.w - b.w).abs() < SIZE_EPS && (a.h - b.h).abs() < SIZE_EPS
}

const SIZE_EPS: f64 = 1e-9;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cover::Rgb;
    use crate::book::edit::PlacementRef;
    use crate::book::pace::{Book, Page, Placement};
    use crate::geometry::Side;
    use crate::geometry::{CoverSide, Rect};
    use crate::print_spec::PrintSpec;

    /// A book holding one slot, so a `SetSlot` has something to be compared
    /// against. `edit_label` reads nothing else.
    fn book_with_slot(rect: Rect) -> Book {
        Book {
            pages: vec![Page {
                number: 1,
                side: Side::Right,
                template_id: "t".into(),
                placements: vec![Placement { photo_index: 0, slot_rect: rect, crop: rect, z: 0 }],
            }],
            seed: 1,
            dropped: 0,
            controls: Default::default(),
            spec: PrintSpec::pixajoy(),
            options: Default::default(),
            cover: Default::default(),
        }
    }

    const SLOT: Rect = Rect { x: 0.1, y: 0.2, w: 0.4, h: 0.3 };

    /// Every variant, constructed once, so the exhaustive `match` in
    /// `edit_label` is exercised rather than merely compiled.
    fn every_edit() -> Vec<BookEdit> {
        let at = PlacementRef { page: 1, z: 0 };
        vec![
            BookEdit::Regenerate { opening: 0 },
            BookEdit::RejectTemplate { opening: 0 },
            BookEdit::SetTemplate { opening: 0, template_id: "t".into() },
            BookEdit::SetLocked { opening: 0, locked: true },
            BookEdit::SetLocked { opening: 0, locked: false },
            BookEdit::Shuffle,
            BookEdit::SwapPhotos { a: at, b: PlacementRef { page: 2, z: 0 } },
            BookEdit::SetCrop { placement: at, x: 0.0, y: 0.0, w: 1.0 },
            BookEdit::SetSlot { placement: at, rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 } },
            BookEdit::ReplacePhoto { placement: at, photo: 3 },
            BookEdit::SetPrintSpec { spec: PrintSpec::pixajoy() },
            BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(1) },
            BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: None },
            BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.0, y: 0.0, w: 1.0 },
            BookEdit::SetSpineColour { rgb: Rgb { r: 1, g: 2, b: 3 } },
        ]
    }

    #[test]
    fn edit_label_names_every_edit_with_something_readable() {
        for edit in every_edit() {
            let label = edit_label(&edit, &book_with_slot(SLOT));
            assert!(!label.is_empty(), "{edit:?} has no label");
            assert_eq!(label, label.to_lowercase(), "{edit:?} is not lower case");
        }
    }

    /// Two edits sharing a label make the control lie about what it would
    /// undo, so no two of them may.
    #[test]
    fn edit_label_tells_the_edits_apart() {
        let book = book_with_slot(SLOT);
        let mut seen: Vec<&str> = every_edit().iter().map(|e| edit_label(e, &book)).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two edits share a label: {seen:?}");
    }

    #[test]
    fn edit_label_says_lock_and_unlock_the_way_round_the_edit_asks() {
        let book = book_with_slot(SLOT);
        assert_eq!(edit_label(&BookEdit::SetLocked { opening: 2, locked: true }, &book), "lock");
        assert_eq!(edit_label(&BookEdit::SetLocked { opening: 2, locked: false }, &book), "unlock");
    }

    /// Clearing a cover is not setting one, and "Undo cover photo" after
    /// clearing one reads as though the photo were put back.
    #[test]
    fn edit_label_distinguishes_setting_a_cover_photo_from_clearing_one() {
        let set = BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: Some(4) };
        let cleared = BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: None };
        let book = book_with_slot(SLOT);
        assert_ne!(edit_label(&set, &book), edit_label(&cleared, &book));
    }

    /// Both gestures in layout mode emit one `SetSlot`, so the label came out
    /// "resize" for a box that was only moved: the user dragged a box across
    /// the page and the Undo tooltip offered to undo a resize. The edit
    /// carries the new rect and the book still has the old one, which is the
    /// only place the difference exists.
    #[test]
    fn edit_label_calls_a_slot_that_only_changed_place_a_move() {
        let book = book_with_slot(SLOT);
        let moved = BookEdit::SetSlot {
            placement: PlacementRef { page: 1, z: 0 },
            rect: Rect { x: 0.5, y: 0.6, w: SLOT.w, h: SLOT.h },
        };

        assert_eq!(edit_label(&moved, &book), "move");
    }

    #[test]
    fn edit_label_still_calls_a_slot_that_changed_size_a_resize() {
        let book = book_with_slot(SLOT);
        let at = PlacementRef { page: 1, z: 0 };
        let wider = BookEdit::SetSlot { placement: at, rect: Rect { w: SLOT.w + 0.05, ..SLOT } };
        let taller = BookEdit::SetSlot { placement: at, rect: Rect { h: SLOT.h + 0.05, ..SLOT } };

        assert_eq!(edit_label(&wider, &book), "resize");
        assert_eq!(edit_label(&taller, &book), "resize");
    }

    /// A resize that also shifts the box -- which is every corner drag except
    /// the south-east one -- is still a resize, not a move.
    #[test]
    fn edit_label_calls_a_corner_drag_a_resize_even_though_it_moved_the_box() {
        let book = book_with_slot(SLOT);
        let nw = BookEdit::SetSlot {
            placement: PlacementRef { page: 1, z: 0 },
            rect: Rect { x: SLOT.x - 0.05, y: SLOT.y - 0.05, w: SLOT.w + 0.05, h: SLOT.h + 0.05 },
        };

        assert_eq!(edit_label(&nw, &book), "resize");
    }

    /// The label is read before the edit is applied, so the slot is always
    /// there. If it somehow is not, the old wording is the safe answer.
    #[test]
    fn edit_label_falls_back_to_resize_for_a_slot_the_book_does_not_have() {
        let book = book_with_slot(SLOT);
        let nowhere = BookEdit::SetSlot {
            placement: PlacementRef { page: 9, z: 0 },
            rect: SLOT,
        };

        assert_eq!(edit_label(&nowhere, &book), "resize");
    }
}
