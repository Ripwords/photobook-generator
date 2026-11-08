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
import {
  photoFor,
  type BookEdit,
  type BookLayout,
  type CoverSide,
  type PlacementRef,
  type PreviewPhoto,
} from "~/types/preview";

/** The `data-placement` value on a photo slot: `"page:z"`. */
export function placementKey(ref: PlacementRef): string {
  return `${ref.page}:${ref.z}`;
}

/** What is being dragged: a photo on a page, or a left-out photo from the tray. */
export type DragSource =
  | {
      kind: "slot";
      placement: PlacementRef;
      /** Its index in `layout.photos`, for dropping on the cover; `null` if unknown. */
      photo: number | null;
      /** The opening it is on: a drop on that opening's own empty space does nothing. */
      opening: number | null;
    }
  | { kind: "tray"; photo: number };

/** What is under the pointer. `locked` targets are shown as refused, never sent. */
export type DropTarget =
  | { kind: "slot"; placement: PlacementRef; locked: boolean }
  | { kind: "page"; opening: number; page: number; locked: boolean }
  | { kind: "cover"; side: CoverSide };

/** The slice of `Element` that hit-testing needs, so tests can pass a fake. */
export interface HitElement {
  closest(selector: string): { getAttribute(name: string): string | null } | null;
}

const PAIR = /^(\d+):(\d+)$/;

/**
 * What an element under the pointer belongs to. The element is usually a
 * slot's `<img>`, or a layer drawn over it, so this walks up: a photo slot
 * first, then the page it sits on, then a cover panel. `null` off all three,
 * and for a key that does not parse, so a malformed attribute can never
 * become an edit with page NaN.
 */
export function dropTargetAt(el: HitElement | null): DropTarget | null {
  if (!el) return null;
  const slot = el.closest("[data-placement]");
  if (slot) {
    const match = PAIR.exec(slot.getAttribute("data-placement") ?? "");
    if (!match) return null;
    return {
      kind: "slot",
      placement: { page: Number(match[1]), z: Number(match[2]) },
      locked: slot.getAttribute("data-locked") === "true",
    };
  }
  const page = el.closest("[data-drop-page]");
  if (page) {
    const match = PAIR.exec(page.getAttribute("data-drop-page") ?? "");
    if (!match) return null;
    return {
      kind: "page",
      opening: Number(match[1]),
      page: Number(match[2]),
      locked: page.getAttribute("data-locked") === "true",
    };
  }
  const side = el.closest("[data-cover-side]")?.getAttribute("data-cover-side");
  if (side === "front" || side === "back") return { kind: "cover", side };
  return null;
}

export type DropOutcome =
  | { kind: "edit"; edit: BookEdit }
  | { kind: "locked"; target: DropTarget }
  | { kind: "none" };

/** What releasing `source` over `target` does. */
export function dropOutcome(source: DragSource, target: DropTarget | null): DropOutcome {
  if (!target) return { kind: "none" };
  if (target.kind === "cover") {
    return source.photo === null
      ? { kind: "none" }
      : { kind: "edit", edit: { kind: "setCoverPhoto", side: target.side, photo: source.photo } };
  }
  if (source.kind === "slot") {
    if (target.kind === "slot") {
      const a = source.placement;
      const b = target.placement;
      if (a.page === b.page && a.z === b.z) return { kind: "none" };
      if (target.locked) return { kind: "locked", target };
      return { kind: "edit", edit: { kind: "swapPhotos", a, b } };
    }
    // Its own spread's empty space: there is nowhere to move it to.
    if (target.opening === source.opening) return { kind: "none" };
    if (target.locked) return { kind: "locked", target };
    return { kind: "edit", edit: { kind: "movePhoto", from: source.placement, opening: target.opening } };
  }
  if (target.locked) return { kind: "locked", target };
  return target.kind === "slot"
    ? { kind: "edit", edit: { kind: "replacePhoto", placement: target.placement, photo: source.photo } }
    : { kind: "edit", edit: { kind: "addPhoto", opening: target.opening, photo: source.photo } };
}

/** The label riding on the dragged photo: what letting go here would do. */
export interface DropBadge {
  label: string;
  icon: string;
  refused: boolean;
}

export function dropBadge(outcome: DropOutcome, target: DropTarget | null): DropBadge | null {
  if (outcome.kind === "none") return null;
  if (outcome.kind === "locked") return { label: "Locked", icon: "i-lucide-lock", refused: true };
  const { edit } = outcome;
  switch (edit.kind) {
    case "swapPhotos":
      return { label: "Swap", icon: "i-lucide-arrow-left-right", refused: false };
    case "replacePhoto":
      return { label: "Replace", icon: "i-lucide-replace", refused: false };
    case "addPhoto":
      return {
        label: target?.kind === "page" ? `Add to page ${target.page}` : "Add to page",
        icon: "i-lucide-image-plus",
        refused: false,
      };
    case "movePhoto":
      return {
        label: target?.kind === "page" ? `Move to page ${target.page}` : "Move to page",
        icon: "i-lucide-move-right",
        refused: false,
      };
    case "setCoverPhoto":
      return { label: `Use on ${edit.side} cover`, icon: "i-lucide-book", refused: false };
    default:
      return null;
  }
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
export function doneToast(edit: BookEdit, pagesOf?: (opening: number) => string): string | null {
  switch (edit.kind) {
    case "swapPhotos":
      return edit.a.page === edit.b.page
        ? `Swapped photos on page ${edit.a.page}`
        : `Swapped photos on pages ${edit.a.page} and ${edit.b.page}`;
    case "replacePhoto":
      return `Replaced the photo on page ${edit.placement.page}`;
    case "addPhoto":
      return pagesOf ? `Added a photo to ${pagesOf(edit.opening)}` : "Added a photo";
    case "setCoverPhoto":
      return edit.photo === null ? null : `Put a photo on the ${edit.side} cover`;
    case "movePhoto":
      return pagesOf
        ? `Moved a photo from page ${edit.from.page} to ${pagesOf(edit.opening)}`
        : "Moved a photo to another page";
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
