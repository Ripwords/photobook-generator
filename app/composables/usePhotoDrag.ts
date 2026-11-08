/**
 * One photo drag for the whole book, so a photo can be dropped on any page or
 * the cover. `BookPreview` provides it; each `BookPreviewPage` injects it to
 * start a drag from a slot and to draw its own highlights, and the left-out
 * tray starts one with `pressToDrag`. Every decision -- what is under the
 * pointer, what a drop does, how fast to scroll -- is in `~/types/photoDrag`;
 * this file only wires those functions to the pointer and the DOM.
 *
 * Pointer events rather than HTML5 drag and drop: WKWebView's native drag
 * competes with the OS file drop Tauri takes, and a ghost that shows the
 * photo at the size it is picked up needs the pointer coordinates anyway.
 */
import type { ComputedRef, InjectionKey, Ref } from "vue";
import {
  autoScrollSpeed,
  dropBadge,
  dropOutcome,
  dropTargetAt,
  type DragSource,
  type DropBadge,
  type DropOutcome,
  type DropTarget,
} from "~/types/photoDrag";
import { pressIsDrag } from "~/types/preview";

/** The picture that follows the pointer, in viewport pixels. */
export interface DragGhost {
  src: string | null;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Sliding back to where it came from after a drop that did nothing. */
  returning: boolean;
}

export interface StartArgs {
  source: DragSource;
  src: string | null;
  from: HTMLElement;
  x: number;
  y: number;
}

export interface PhotoDrag {
  source: Ref<DragSource | null>;
  hover: Ref<DropTarget | null>;
  /** What letting go now would do. */
  outcome: ComputedRef<DropOutcome>;
  /** The label for it, riding on the ghost; `null` when it would do nothing. */
  badge: ComputedRef<DropBadge | null>;
  ghost: Ref<DragGhost | null>;
  start(args: StartArgs): void;
  /**
   * A press on something draggable that has no click of its own: it becomes a
   * drag once it travels `DRAG_THRESHOLD_PX`, and a press that never does is
   * nothing at all.
   */
  pressToDrag(event: PointerEvent, args: Omit<StartArgs, "from" | "x" | "y">): void;
}

const KEY: InjectionKey<PhotoDrag> = Symbol("photoDrag");

/** The ghost's longer side. Big enough to recognise the photo, small enough not to hide the target. */
const GHOST_PX = 140;
/** How long a refused or missed drop takes to slide back. */
export const RETURN_MS = 180;

export function providePhotoDrag(onDrop: (outcome: DropOutcome) => void): PhotoDrag {
  const source = ref<DragSource | null>(null);
  const hover = ref<DropTarget | null>(null);
  const ghost = ref<DragGhost | null>(null);
  const outcome = computed(() => (source.value ? dropOutcome(source.value, hover.value) : ({ kind: "none" } as const)));
  const badge = computed(() => dropBadge(outcome.value, hover.value));

  let origin: DOMRect | null = null;
  /** Where in the ghost the pointer holds it, so it does not jump to the corner. */
  let grab = { x: 0, y: 0 };
  let pointer = { x: 0, y: 0 };
  let scroller: HTMLElement | null = null;
  let frame = 0;
  let returnTimer: ReturnType<typeof setTimeout> | null = null;

  function hitTest() {
    // The ghost is `pointer-events: none`, so this sees the page under it.
    hover.value = dropTargetAt(document.elementFromPoint(pointer.x, pointer.y));
  }

  function place() {
    if (!ghost.value) return;
    ghost.value = { ...ghost.value, x: pointer.x - grab.x, y: pointer.y - grab.y };
  }

  function tick() {
    if (!source.value || !scroller) return;
    const speed = autoScrollSpeed(pointer.y, scroller.getBoundingClientRect());
    if (speed !== 0) {
      scroller.scrollBy(0, speed);
      hitTest();
    }
    frame = requestAnimationFrame(tick);
  }

  function onMove(event: PointerEvent) {
    pointer = { x: event.clientX, y: event.clientY };
    place();
    hitTest();
  }

  function finish(result: DropOutcome) {
    stopListening();
    const wasGhost = ghost.value;
    source.value = null;
    hover.value = null;
    if (result.kind === "edit" || !wasGhost || !origin) {
      ghost.value = null;
    } else {
      // Missed or refused: slide back, so it is plain nothing happened.
      ghost.value = { ...wasGhost, x: origin.left, y: origin.top, w: origin.width, h: origin.height, returning: true };
      returnTimer = setTimeout(() => (ghost.value = null), RETURN_MS);
    }
    onDrop(result);
  }

  function onUp(event: PointerEvent) {
    pointer = { x: event.clientX, y: event.clientY };
    hitTest();
    if (!source.value) return;
    finish(outcome.value);
  }

  function onCancel() {
    if (source.value) finish({ kind: "none" });
  }

  // Capture phase, and stopped there, so Esc cancels the drag without also
  // dropping the selection or reaching a global shortcut.
  function onKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || !source.value) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    finish({ kind: "none" });
  }

  function stopListening() {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
    window.removeEventListener("keydown", onKey, true);
    cancelAnimationFrame(frame);
    document.documentElement.classList.remove("photo-dragging");
  }

  function start({ source: what, src, from, x, y }: StartArgs) {
    if (returnTimer) clearTimeout(returnTimer);
    origin = from.getBoundingClientRect();
    const scale = Math.min(1, GHOST_PX / Math.max(origin.width, origin.height, 1));
    const w = origin.width * scale;
    const h = origin.height * scale;
    grab = { x: ((x - origin.left) / (origin.width || 1)) * w, y: ((y - origin.top) / (origin.height || 1)) * h };
    pointer = { x, y };
    scroller = from.closest<HTMLElement>("[data-desk]");
    source.value = what;
    hover.value = null;
    ghost.value = { src, x: x - grab.x, y: y - grab.y, w, h, returning: false };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey, true);
    document.documentElement.classList.add("photo-dragging");
    frame = requestAnimationFrame(tick);
  }

  onBeforeUnmount(() => {
    stopListening();
    if (returnTimer) clearTimeout(returnTimer);
  });

  function pressToDrag(event: PointerEvent, args: Omit<StartArgs, "from" | "x" | "y">) {
    if (event.button !== 0 || source.value) return;
    const el = event.currentTarget as HTMLElement;
    const startX = event.clientX;
    const startY = event.clientY;
    el.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      if (!pressIsDrag(e.clientX - startX, e.clientY - startY)) return;
      done(e);
      start({ ...args, from: el, x: e.clientX, y: e.clientY });
    };
    const done = (e: PointerEvent) => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", done);
      el.removeEventListener("pointercancel", done);
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", done);
    el.addEventListener("pointercancel", done);
  }

  const drag: PhotoDrag = { source, hover, outcome, badge, ghost, start, pressToDrag };
  provide(KEY, drag);
  return drag;
}

/** The book's drag, or `null` outside a `BookPreview` (the page viewer, for one). */
export function usePhotoDrag(): PhotoDrag | null {
  return inject(KEY, null);
}
