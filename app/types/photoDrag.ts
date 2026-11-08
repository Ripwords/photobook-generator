/**
 * Dragging a photo onto another photo to swap them, as pure functions. The
 * composable (`usePhotoDrag`) owns the pointer and the DOM; everything it
 * decides is decided here, so it can be tested without a webview.
 *
 * Which swaps are ALLOWED is not decided here either: Rust refuses one that
 * would cut a face, put one in the gutter or print too small, and returns the
 * untouched book with the reason. The one thing settled before sending is a
 * locked target, because the drag has to show it as refused while hovering.
 */
import { photoFor, type BookEdit, type BookLayout, type PlacementRef, type PreviewPhoto } from "~/types/preview";

/** The `data-placement` value on a photo slot: `"page:z"`. */
export function placementKey(ref: PlacementRef): string {
  return `${ref.page}:${ref.z}`;
}

/** A photo slot under the pointer. */
export interface DropTarget {
  placement: PlacementRef;
  /** On a locked spread: shown as refused, never sent. */
  locked: boolean;
}

/** The slice of `Element` that hit-testing needs, so tests can pass a fake. */
export interface HitElement {
  closest(selector: string): { getAttribute(name: string): string | null } | null;
}

/**
 * The slot an element under the pointer belongs to. The element is usually
 * the slot's `<img>`, or a badge drawn over it, so this walks up to the
 * nearest `data-placement`. `null` off every photo, and for a key that does not
 * parse, so a malformed attribute can never become a swap with page NaN.
 */
export function dropTargetAt(el: HitElement | null): DropTarget | null {
  const slot = el?.closest("[data-placement]");
  if (!slot) return null;
  const match = /^(\d+):(\d+)$/.exec(slot.getAttribute("data-placement") ?? "");
  if (!match) return null;
  return {
    placement: { page: Number(match[1]), z: Number(match[2]) },
    locked: slot.getAttribute("data-locked") === "true",
  };
}

export type DropOutcome = { kind: "swap"; edit: BookEdit } | { kind: "locked" } | { kind: "none" };

/** What releasing the dragged photo over `target` does. */
export function dropOutcome(source: PlacementRef, target: DropTarget | null): DropOutcome {
  if (!target) return { kind: "none" };
  const { placement } = target;
  if (placement.page === source.page && placement.z === source.z) return { kind: "none" };
  if (target.locked) return { kind: "locked" };
  return { kind: "swap", edit: { kind: "swapPhotos", a: source, b: placement } };
}

/** How close to the desk's top or bottom edge a drag starts scrolling it. */
export const AUTO_SCROLL_EDGE_PX = 72;
/** Pixels per animation frame at the edge itself. */
export const AUTO_SCROLL_MAX_PX = 24;

/**
 * Pixels to scroll the desk this frame, negative for up: zero in the middle,
 * rising linearly across the last `AUTO_SCROLL_EDGE_PX` to the cap at the
 * edge and past it, so a drag aimed at a spread out of view gets there.
 */
export function autoScrollSpeed(pointerY: number, viewport: { top: number; bottom: number }): number {
  const intoTop = viewport.top + AUTO_SCROLL_EDGE_PX - pointerY;
  if (intoTop > 0) return -Math.round(AUTO_SCROLL_MAX_PX * Math.min(1, intoTop / AUTO_SCROLL_EDGE_PX));
  const intoBottom = pointerY - (viewport.bottom - AUTO_SCROLL_EDGE_PX);
  if (intoBottom > 0) return Math.round(AUTO_SCROLL_MAX_PX * Math.min(1, intoBottom / AUTO_SCROLL_EDGE_PX));
  return 0;
}

/**
 * The click route, for anyone who does not drag. A click only selects. A swap
 * needs "Swap with…" first (`armed`), so a stray click can never move a photo,
 * which is what the old click-then-click gesture did.
 */
export interface SwapSelection {
  selected: PlacementRef | null;
  armed: boolean;
}

export function swapStepFor(
  state: SwapSelection,
  clicked: PlacementRef,
): { next: SwapSelection; edit: BookEdit | null } {
  const same = state.selected !== null && state.selected.page === clicked.page && state.selected.z === clicked.z;
  if (state.armed && state.selected) {
    if (same) return { next: { selected: state.selected, armed: false }, edit: null };
    return { next: { selected: null, armed: false }, edit: { kind: "swapPhotos", a: state.selected, b: clicked } };
  }
  return { next: { selected: same ? null : clicked, armed: false }, edit: null };
}

/**
 * The toast after an edit that moved a photo, or `null` for one whose result
 * is already plain on screen. A swap across a long book changes two spreads
 * that may never both be in view, so it is said out loud, next to Undo.
 */
export function doneToast(edit: BookEdit): string | null {
  switch (edit.kind) {
    case "swapPhotos":
      return edit.a.page === edit.b.page
        ? `Swapped photos on page ${edit.a.page}`
        : `Swapped photos on pages ${edit.a.page} and ${edit.b.page}`;
    case "replacePhoto":
      return `Replaced the photo on page ${edit.placement.page}`;
    default:
      return null;
  }
}

/** The photo in the slot `ref` names, on any page, or `null` if there is no such slot. */
export function photoAt(layout: BookLayout, ref: PlacementRef): PreviewPhoto | null {
  const placement = layout.pages
    .find((page) => page.number === ref.page)
    ?.placements.find((p) => p.z === ref.z);
  return placement ? photoFor(layout, placement.photoIndex) : null;
}
