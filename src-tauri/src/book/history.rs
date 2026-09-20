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

use crate::book::edit::BookEdit;
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
/// Deliberately no catch-all arm: a new `BookEdit` variant must fail to
/// compile here rather than quietly join the pile under one vague word.
pub fn edit_label(edit: &BookEdit) -> &'static str {
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
        BookEdit::SetSlot { .. } => "resize",
        BookEdit::ReplacePhoto { .. } => "photo replacement",
        BookEdit::SetPrintSpec { .. } => "print size change",
        BookEdit::SetCoverPhoto { photo: Some(_), .. } => "cover photo",
        BookEdit::SetCoverPhoto { photo: None, .. } => "cover photo removal",
        BookEdit::SetCoverCrop { .. } => "cover crop",
        BookEdit::SetSpineColour { .. } => "spine colour",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cover::Rgb;
    use crate::book::edit::PlacementRef;
    use crate::geometry::{CoverSide, Rect};
    use crate::print_spec::PrintSpec;

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
            let label = edit_label(&edit);
            assert!(!label.is_empty(), "{edit:?} has no label");
            assert_eq!(label, label.to_lowercase(), "{edit:?} is not lower case");
        }
    }

    /// Two edits sharing a label make the control lie about what it would
    /// undo, so no two of them may.
    #[test]
    fn edit_label_tells_the_edits_apart() {
        let mut seen: Vec<&str> = every_edit().iter().map(edit_label).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two edits share a label: {seen:?}");
    }

    #[test]
    fn edit_label_says_lock_and_unlock_the_way_round_the_edit_asks() {
        assert_eq!(edit_label(&BookEdit::SetLocked { opening: 2, locked: true }), "lock");
        assert_eq!(edit_label(&BookEdit::SetLocked { opening: 2, locked: false }), "unlock");
    }

    /// Clearing a cover is not setting one, and "Undo cover photo" after
    /// clearing one reads as though the photo were put back.
    #[test]
    fn edit_label_distinguishes_setting_a_cover_photo_from_clearing_one() {
        let set = BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: Some(4) };
        let cleared = BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: None };
        assert_ne!(edit_label(&set), edit_label(&cleared));
    }
}
