# User-made layouts (UX phase 4): the backend

This note covers the Rust store, validator, engine merge, commands and wire types. The editor UI
that draws and manages layouts is a separate piece of work and is not built yet.

## Decisions (user, 2026-11-08)

- Custom layouts sit on top of the built-in templates. Each one is either a single page or a
  spread of normalised photo boxes.
- The library is global. It lives in the app's SQLite database, and every book sees the same
  list.
- Each layout has a **Use in auto layout** flag. It is off by default.
  - Off: generate, Regenerate, Reject, Shuffle and Add photo never pick the layout. **Choose a
    layout** still lists it for an opening with the same photo count (and side, for a single
    page). `setTemplate` with its id works.
  - On: the layout is scored exactly like a built-in.
- A layout that breaks a print rule for a book is refused for that book only, with a reason.
  The menu shows it dimmed.
- The print rules are the 5 mm bleed and the 10 mm gutter dead zone (5 mm each side of the
  fold on Pixajoy). A box may reach the paper edge, but it may never span the fold.

## Shape

| Piece | Where |
|---|---|
| Validator, store, conversion | `src-tauri/src/custom_layouts.rs` |
| Merge into the engine | `Library::with_custom` and the `auto_*` / `menu_*` queries in `src-tauri/src/templates.rs` |
| Refused layouts and names in the preview | `PreviewOpening.refused`, `BookLayout.layoutNames`, `BookLayout.sketches` in `src-tauri/src/preview.rs` |
| Commands | the `user layouts` section of `src-tauri/src/commands.rs` |
| TypeScript mirrors | `app/types/customLayouts.ts`, plus `refused`, `layoutNames` and `layoutLabel` in `app/types/preview.ts` |
| Wire pin | `tests/fixtures/wire/custom-layouts.json` (`custom_layouts_serialise_exactly_...` and `tests/customLayouts.test.ts`) |
| Browser harness | `dev/tauri-mock/layouts.ts` |

### Store

Each layout is one row of `custom_layouts (id, name, shape_json, auto, created_at, updated_at)`,
created by `Db::migrate`. `shape_json` is the `LayoutShape` that serde writes:
`{"kind":"spread","boxes":[...]}` or `{"kind":"page","side":"right","boxes":[...]}`. Each box
is `{rect:{x,y,w,h}, role:"hero"|"support"}`. A row whose shape no longer parses is skipped
by `list`, not treated as an error.

**Ids** are `user-` followed by 32 hex digits from `/dev/urandom`. They never contain `:`,
because `<id>:left` names a built-in page half. `Library::load` refuses a built-in template
whose id starts with `user-` or contains `:` (`TemplateError::ReservedId`), so the two id
spaces cannot collide.

Names are trimmed and must be 1 to 60 characters. Duplicating a layout appends " copy",
truncating the original name to fit, and sets auto off.

### Validator

A save is refused only by the **structural** rules:

| Kind | Rule |
|---|---|
| `noBoxes` | No boxes. |
| `tooManyBoxes` | More than 8 boxes. `best_spread` tries n! photo orders. |
| `notFinite` | A rect that is not a finite number. |
| `offCanvas` | A rect that leaves the canvas. |
| `tooSmall` | A box under 5 % of the page either way (`MIN_SLOT`). |
| `crossesFold` | A spread box that crosses x = 0.5. Ending on the fold is fine. |
| `overlap` | Two boxes on one page overlapping by more than `OVERLAP_EPS` both ways. |

The **print rules** are evaluated per book, from `book.spec`, and refuse the layout for that
book only:

| Kind | Rule |
|---|---|
| `edgeInBleed` | An outer edge stops strictly between the paper edge and the trim line. |
| `edgeInGutter` | The fold-side edge stops strictly inside the gutter strip. |
| `mostlyHidden` | Less than 5 % of the box is left inside the trim and clear of the gutter. |

An edge exactly at the paper edge, the trim line, the fold or the gutter line passes. Every
built-in template passes every rule (`custom_layouts_accepts_every_built_in_template`). Rust
writes every message, and the webview shows `message` as it arrives.

The TypeScript editor's text-gap rule (`MIN_SLOT_GAP_IN`) is deliberately not ported. It is a
taste rule, not a print rule.

### Engine merge

Every book command rebuilds the library from the store (`commands::book_library`):
`Library::load(templates).with_custom(&custom_layouts::list(db), &book.spec)`. As a result,
creating, editing, toggling or deleting a layout takes effect on the next command, with no
restart and no cache. `edit_book` merges again when the edit changes the print size.

`with_custom` converts each layout through `to_template` into the engine's own
`SpreadTemplate` or `PageLayout`, so it is scored like a built-in. A layout that fails
`check` at the book's size goes on `Library.refused` with its problems instead.

Each candidate query states which question it answers:

- `auto_spreads_with` / `auto_pages`: built-ins plus auto-on custom layouts. Used by
  generate (`pace`), Regenerate, Reject, Shuffle and Add photo.
- `menu_spreads_with` / `menu_pages`: built-ins plus every admitted custom layout. Used by
  the layout menu (`PreviewOpening.alternatives`) and by `setTemplate`.
- Capacity (`recommend_pages`, `page_half_pool`) counts built-ins only, so a custom layout
  never changes how many photos a book is planned for.

### Deleting a layout

Deleting a layout rewrites no book. Pages laid out with it keep their rects, crops and
template id. The id stops resolving, so the preview carries no sketch or name for it, and the
menu no longer offers it. Crop, Shuffle and Regenerate on that opening still work
(`custom_layouts_engine_deleting_a_layout_leaves_books_working`).

## Commands

Every store command rejects with a structured `CustomLayoutError`. The webview turns it into
one line with `customLayoutErrorText`.

```ts
invoke<CustomLayout[]>("list_custom_layouts")
invoke<CustomLayout>("create_custom_layout", { name: string, shape: LayoutShape })
invoke<CustomLayout>("update_custom_layout", { id: string, shape: LayoutShape })
invoke<CustomLayout>("rename_custom_layout", { id: string, name: string })
invoke<CustomLayout>("duplicate_custom_layout", { id: string })
invoke<void>("delete_custom_layout", { id: string })
invoke<CustomLayout>("set_custom_layout_auto", { id: string, auto: boolean })
invoke<Problem[]>("check_custom_layout", { shape: LayoutShape, spec?: PrintSpec })  // never rejects
```

Without `spec`, `check_custom_layout` returns only the structural problems. With `spec`, it
returns those plus the print problems at that size. An editor can call it on every change.

For one opening, the UI reads usable and refused layouts from `book_layout` (and from every
edit's answer):

- Usable layouts are in `opening.alternatives`, built-in and custom ids mixed, auto flag
  ignored.
- Refused layouts are in `opening.refused`: `{ templateId, problems }` for every custom layout
  with this photo count (and side) that this book's size cannot print.
- `layout.sketches[id]` draws either kind, and `layoutLabel(layout, id)` names either kind.

## Known limits

- Custom layouts compete only at the group sizes that built-in capacity produces. A 7-box
  layout is never chosen automatically if no built-in holds 7.
- Custom layouts have no title text zone.
- A custom spread is not cut into halves for page 1 or the last page. Use a page layout.
- `canRelayout` (`app/types/preview.ts`) reads the menu alternatives. If the only remaining
  alternatives are auto-off custom layouts, Regenerate is enabled but answers `noAlternative`.
- No command copies a built-in's shape as a starting point yet. `custom_layouts::shape_of`
  exists for one.
- The manual has no entry yet, because there is no control a user can reach. The UI change
  that adds the Layouts screen must add the entry.
