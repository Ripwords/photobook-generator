//! The agent's one write path: apply an edit, answer with the new view.
//!
//! Its error type is the other half of the privacy boundary. A refusal's
//! reason is `EditError`'s text, which is built from opening numbers,
//! template ids and fixed phrases. Everything else (the database, the cache,
//! the template folder) can name a path, so it crosses as `Failed` with no
//! text at all and is logged here instead.

use super::view::{agent_view, AgentView, SourcePhoto};
use crate::book::cull::Photo;
use crate::book::edit::{apply, BookEdit, EditError};
use crate::book::pace::Book;
use crate::templates::{Library, Weights};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentError {
    Refused { reason: String },
    Failed,
}

impl From<EditError> for AgentError {
    fn from(e: EditError) -> Self {
        Self::Refused {
            reason: e.to_string(),
        }
    }
}

impl AgentError {
    pub fn failed(detail: impl std::fmt::Display) -> Self {
        log::warn!("agent command failed: {detail}");
        Self::Failed
    }
}

/// Applies `edit` to `book` and returns the view of the result. A refused
/// edit leaves `book` as it was.
pub fn edit_and_view(
    book: &mut Book,
    edit: &BookEdit,
    lib: &Library,
    photos: &[SourcePhoto],
    weights: &Weights,
) -> Result<AgentView, AgentError> {
    let engine: Vec<Photo> = photos.iter().map(|p| p.photo.clone()).collect();
    apply(book, edit, lib, &engine, weights)?;
    Ok(agent_view(book, lib, photos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::view::tests::{frozen_library, small_book, small_photos};

    fn edit(book: &mut Book, edit: BookEdit) -> Result<AgentView, AgentError> {
        edit_and_view(
            book,
            &edit,
            &frozen_library(),
            &small_photos(),
            &Weights::default(),
        )
    }

    #[test]
    fn agent_edit_answers_with_the_view_of_the_edited_book() {
        let mut book = small_book();

        let view = edit(
            &mut book,
            BookEdit::SetLocked {
                opening: 1,
                locked: true,
            },
        )
        .expect("locking an opening is allowed");

        assert!(view.openings[1].locked, "the view shows the new lock");
        assert!(book.controls[&1].locked, "the book itself was edited");
        assert_eq!(view, agent_view(&book, &frozen_library(), &small_photos()));
    }

    #[test]
    fn agent_edit_refuses_with_edit_errors_text_and_leaves_the_book_alone() {
        let mut book = small_book();
        let before = book.clone();

        let refused = edit(&mut book, BookEdit::Regenerate { opening: 0 });

        assert_eq!(
            refused,
            Err(AgentError::Refused {
                reason: "this spread is locked; unlock it to change it".into()
            })
        );
        assert_eq!(book, before);
    }

    #[test]
    fn agent_error_serialises_a_refusal_with_its_reason_and_a_failure_with_nothing() {
        let refused = AgentError::from(EditError::NoSuchOpening(9));
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            serde_json::json!({"kind": "refused", "reason": "this book has no opening 9"})
        );
        assert_eq!(
            serde_json::to_value(AgentError::failed("/Users/alice/Library/pbg.sqlite")).unwrap(),
            serde_json::json!({"kind": "failed"})
        );
    }
}
