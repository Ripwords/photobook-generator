# Phase 1: agent view in Rust

[Overview](overview.md)

## Goal

One function decides everything the model may know about a book. Every read tool and every
write tool's result goes through it.

## Changes

- New `src-tauri/src/agent/view.rs` (and `agent/mod.rs`). A pure function builds an
  `AgentView` from a `Book` and its resolved `Photo`s. Percentiles are recomputed within the
  book's own photos, per the design's "rank as percentiles within this book".
- New command `agent_view(projectId)` in `commands.rs`, registered in `lib.rs`. It loads the
  project, resolves its photos through `resolve_photos`, and returns the view.
- A wire fixture `tests/fixtures/wire/agent-view.json`, pinned from both sides like
  `book-layout.json`.

## Data structures

- `AgentView { pageCount, placedPhotos, droppedPhotos, openings: AgentOpening[], photos: AgentPhoto[] }`
- `AgentOpening { index, pages, templateId, locked, alternatives, slots: { page, z, photo }[] }`
- `AgentPhoto { id, day, event, faces, faceArea, tags (at most 3), aestheticPct, sharpnessPct, placed }`
- `id` is the `photoIndex` already used by `BookLayout`, so a tool call can name a photo.
  `day` is days since the book's earliest dated photo, `null` when undated.

## Verification

- A leak test serialises a view built from a fixture that holds a path, a hash, GPS, an
  absolute capture time, face boxes and a saliency box. It asserts none of those values
  appear anywhere in the JSON (value search, not key search). Mutation: add `path` to
  `AgentPhoto` and paste the failure.
- `day` is relative. Test two photos 36 hours apart come out as days 0 and 1, and an
  undated photo as `null`.
- Tags are capped at 3 and keep Vision's order.
- The opening numbering matches `edit::opening_pages` for a 20-page book.
- `bun run test:rust`, and the TS half of the fixture in `bun run test`.
