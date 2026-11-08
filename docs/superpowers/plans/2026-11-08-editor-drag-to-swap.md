# Editor: drag to swap (UX phase 1)

Research behind this: the editor UX audit and the survey of other photobook and layout editors
(Mixbook, Shutterfly, iPhoto, Canva, Affinity, Lightroom; NN/g, Apple HIG and Atlassian
drag-and-drop guidance), summarised in the proposal the user approved on 2026-11-08.

## Problem

A user reported that swapping, moving and replacing photos "still doesn't feel intuitive". The
cause is the drag gesture. In every editor surveyed, dragging a photo moves the photo. Here it pans
the crop, and swapping takes a click on one photo and then a click on another, with no drag, no drop
preview and no animation. Clicks on a locked spread do nothing and say nothing. Errors render as a
banner at the top of the desk, possibly scrolled out of view.

## Decisions (user, 2026-11-08)

- Dragging a photo moves it: photo onto photo swaps, anywhere in the book. Cropping moves to
  double-click.
- A page that loses a photo re-lays out with one fewer (phase 3; not this plan).
- Custom layouts get a per-layout "Use in auto layout" toggle (phase 4; not this plan).
- Build this phase first.

## Scope

Frontend only. No Rust change: `swapPhotos` already exists, refuses locked spreads and broken
constraints, and re-crops each photo for its new slot with the subject-aware `choose_crop`. That
replaces the hand crop; a carried-over crop would rarely fit a slot of a different shape, so this is
intended, and the manual says so.

In **Arrange photos** mode (renamed from "Crop and swap"):

| Gesture | Result |
|---|---|
| Drag a photo onto another photo, any page | Swap. While hovering, the target shows a **Swap** badge and the source slot shows the target photo, so the result is visible before release. |
| Drag and release anywhere else, or press Esc mid-drag | Nothing changes; the ghost slides back to where it came from. |
| Drag onto a photo on a locked spread | That target shows **Locked** and is not a drop target. |
| Click a photo | Selects it. The bar at the bottom offers Swap with…, Crop… and Choose any photo…. |
| Double-click a photo | Opens the crop dialog. |
| Right-click a photo | Context menu: Swap with…, Crop…, Choose any photo…. |
| **Swap with…** (bar or menu), then click a photo | Swap. The keyboard and single-pointer route for the drag (WCAG 2.5.7). Esc cancels. |
| Press, click or drag on a locked spread | Toast: "This spread is locked", with an **Unlock** action. |

After a successful edit, the changed slots flash for 700 ms, and a toast names the change with an
**Undo** action, using the history label Rust already produces. An edit error also shows as a toast
by the book, as well as in the existing banner.

**Move and resize boxes** mode does not change.

Out of scope: dragging from the photo tray, adding or removing photos (phase 2), moving a photo to
another page's empty slot (phase 3), and the layouts tab (phase 4).

## Design

The drag is built on pointer events, not HTML5 drag and drop. WKWebView's HTML5 drag is unreliable
inside Tauri, which takes OS file drops, and a custom ghost needs pointer coordinates anyway.

- `app/types/photoDrag.ts` (pure, unit tested):
  - `PhotoDragState` holds the source ref, its start point, the current point and the hovered
    target, or none.
  - `dropTargetAt(el)` reads the `data-placement` and `data-locked` attributes from the element
    under the pointer.
  - `dropOutcome(source, target)` returns `swap` with a `swapPhotos` edit, `none` (same slot or no
    target) or `locked`.
  - `autoScrollSpeed(pointerY, viewport)` scrolls the desk near its top and bottom edges.
  - `swapStepFor(armed, clicked)` replaces `nextSwapStep`. A plain click only selects; a swap needs
    the "Swap with…" step to be armed first.
- `app/composables/usePhotoDrag.ts` holds one drag for the whole book. BookPreview provides it and
  each BookPreviewPage injects it. It runs the ghost, hit-testing with `document.elementFromPoint`,
  auto-scrolling and Esc.
- `BookPreviewPage.vue`: in arrange mode a press past `DRAG_THRESHOLD_PX` starts a photo drag
  instead of a crop pan, and inline panning goes away. Each slot gets `data-placement` and
  `data-locked`. Swap and Locked badges, the source preview and the post-edit flash are drawn here.
- `BookPreview.vue`: the floating ghost, the context menu, the "Swap with…" arming and the renamed
  mode button with a corrected tooltip.
- `BookEditor.vue` / `useBook.ts`: after `editBook`, a toast with the undo label and an Undo action.
  Errors also go to a toast.

## Tests (TDD, mutation-checked)

`tests/photoDrag.test.ts`:
- `dropOutcome`: a swap across pages; the same slot gives none; a locked target gives locked;
  no target gives none.
- `dropTargetAt`: parses a real element tree built with happy-dom, including a child `<img>` under
  the pointer, which must resolve to its slot.
- `autoScrollSpeed`: zero in the middle band, rising towards each edge, and signed correctly.
- `swapStepFor`: a click without arming selects only; armed plus click on another photo swaps; armed
  plus the same photo cancels.

Mutation checks: break each function (return `swap` for the same slot, ignore `data-locked`, flip
the scroll sign, let a plain click swap) and confirm a test fails. Paste the evidence in the commit.

## Verification

`bun run test`, `lint`, `typecheck` and `check:build`. Then drive `bun run ui:mock` with Playwright
and take screenshots: mid-drag with the Swap badge and the source preview, a drop onto a locked
target, the context menu, the toast, the flash, and a double-click opening the crop dialog. Check
the edges: a drag near the bottom auto-scrolls, and a drop off the book snaps back.

## Manual

Update the "Editing the book" section in `docs/manual.md`: swap, crop, the mode name and tooltip,
the locked notice and the toast. Fix the stale "more than two chapters" claim. Leave the Settings
section alone (another session owns it).

## Coordination

The other session owns the `convertFileSrc` → `photoSrc()` lines in `BookPreviewPage.vue:315`,
`BookPreviewCover.vue:130` and `BookPreview.vue:242,604`. Don't edit those lines, and merge
`feat/story` before committing.
