<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cropMoved,
  cropStyle,
  cropZoomed,
  insideCover,
  pageGuides,
  pageSlots,
  rectStyle,
  samePlacement,
  slotMoved,
  slotResized,
  type BookLayout,
  type Corner,
  type PageSide,
  type PlacementRef,
  type PreviewGeometry,
  type PreviewPage,
  type PreviewRect,
  wheelZoomFactor,
} from "~/types/preview";

const {
  layout,
  page = null,
  side,
  selectable = false,
  selected = null,
  editSlots = false,
  busy = false,
  geometry = null,
} = defineProps<{
  layout: BookLayout;
  /**
   * `null` renders the INSIDE COVER facing a single page -- page 1 faces the
   * inside front cover and the last page faces the inside back cover, so
   * neither is half of a spread. It is drawn as an explicit non-page rather
   * than left empty, because an empty half would read as a blank PAGE, which
   * is a different and much more alarming thing.
   */
  page?: PreviewPage | null;
  side: PageSide;
  /** Whether a photo can be picked for a swap. Off while busy or locked. */
  selectable?: boolean;
  /** The photo currently picked for a swap, anywhere in the book. */
  selected?: PlacementRef | null;
  /**
   * Layout mode: dragging a slot MOVES it and its corner handles resize it,
   * instead of the drag moving the photo's crop. Off by default because
   * moving a box is a rarer, more consequential edit than adjusting a crop,
   * and the two gestures cannot share one drag.
   */
  editSlots?: boolean;
  /** True while an edit is in flight; its end is when a live gesture is settled. */
  busy?: boolean;
  /**
   * Guides to draw INSTEAD of the book's own: the Print size panel's proposal,
   * derived in Rust by `check_print_spec`, so the page reshapes as the user
   * types. `null` draws the book as saved.
   */
  geometry?: PreviewGeometry | null;
}>();

const shown = computed(() => geometry ?? layout.geometry);

const emit = defineEmits<{
  /** The user clicked a photo while swapping -- see `nextSwapStep`. */
  select: [placement: PlacementRef];
  /** The user dragged or zoomed a photo inside its slot and let go. */
  crop: [placement: PlacementRef, crop: PreviewRect];
  /** The user moved or resized a slot in layout mode and let go. */
  slot: [placement: PlacementRef, rect: PreviewRect];
}>();

/**
 * Slot editing, the same shape as cropping: a local live rect while the
 * pointer is down, one edit on release. Edges snap to the page's guides --
 * the trim, safe and gutter lines the engine enforces, the canvas edge, and
 * the other slots on the page -- within `SNAP_THRESHOLD` of the page.
 */
const SNAP_THRESHOLD = 0.012;
const MIN_SLOT_SIZE = 0.05;
const liveRects = ref<Record<string, PreviewRect>>({});
interface SlotDrag {
  key: string;
  ref: PlacementRef;
  startX: number;
  startY: number;
  origin: PreviewRect;
  pageW: number;
  pageH: number;
  corner: Corner | null;
  moved: boolean;
}
let slotDrag: SlotDrag | null = null;
const pageEl = useTemplateRef<HTMLDivElement>("pageEl");

function guidesFor(key: string) {
  const others = (page?.placements ?? [])
    .filter((p) => `p${page?.number}-z${p.z}` !== key)
    .map((p) => liveRects.value[`p${page?.number}-z${p.z}`] ?? p.slotRect);
  return pageGuides(shown.value, side, others);
}

function onSlotPointerDown(
  event: PointerEvent,
  key: string,
  ref: PlacementRef,
  rect: PreviewRect,
  corner: Corner | null,
) {
  if (!selectable || !editSlots || event.button !== 0) return;
  event.stopPropagation();
  const box = pageEl.value?.getBoundingClientRect();
  slotDrag = {
    key,
    ref,
    startX: event.clientX,
    startY: event.clientY,
    origin: liveRects.value[key] ?? rect,
    pageW: box?.width || 1,
    pageH: box?.height || 1,
    corner,
    moved: false,
  };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function onSlotPointerMove(event: PointerEvent) {
  if (!slotDrag) return;
  const dxPx = event.clientX - slotDrag.startX;
  const dyPx = event.clientY - slotDrag.startY;
  if (!slotDrag.moved && Math.hypot(dxPx, dyPx) < DRAG_THRESHOLD_PX) return;
  slotDrag.moved = true;
  const delta = { dx: dxPx / slotDrag.pageW, dy: dyPx / slotDrag.pageH };
  const guides = guidesFor(slotDrag.key);
  const next = slotDrag.corner
    ? slotResized(slotDrag.origin, slotDrag.corner, delta, guides, SNAP_THRESHOLD, MIN_SLOT_SIZE)
    : slotMoved(slotDrag.origin, delta, guides, SNAP_THRESHOLD);
  liveRects.value = { ...liveRects.value, [slotDrag.key]: next };
}

function onSlotPointerUp(event: PointerEvent) {
  if (!slotDrag) return;
  const finished = slotDrag;
  slotDrag = null;
  (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  const rect = liveRects.value[finished.key];
  if (finished.moved && rect) emit("slot", finished.ref, rect);
}

function onSlotPointerCancel() {
  if (!slotDrag) return;
  const { [slotDrag.key]: _dropped, ...rest } = liveRects.value;
  liveRects.value = rest;
  slotDrag = null;
}

const CORNERS: Corner[] = ["nw", "ne", "sw", "se"];
const cornerClass: Record<Corner, string> = {
  nw: "top-0 left-0 cursor-nwse-resize",
  ne: "top-0 right-0 cursor-nesw-resize",
  sw: "bottom-0 left-0 cursor-nesw-resize",
  se: "bottom-0 right-0 cursor-nwse-resize",
};

/**
 * Hand-cropping, as a gesture on the slot itself: drag the photo to move the
 * window, ⌘-scroll to zoom it. The window is drawn live from a local copy while
 * the pointer is down and sent as ONE edit on release, so a drag is one save
 * and one round trip, not one per pixel. Rust re-derives the height from the
 * slot, keeps the window inside the photo and re-runs the hard constraints;
 * whatever it returns is what stays on screen. A press that never moved past
 * `DRAG_THRESHOLD_PX` is a click, which selects the photo for a swap.
 */
const DRAG_THRESHOLD_PX = 3;
/** Live crops for slots mid-gesture, keyed like `PreviewSlot.key`. */
const liveCrops = ref<Record<string, PreviewRect>>({});
interface Drag {
  key: string;
  ref: PlacementRef;
  startX: number;
  startY: number;
  origin: PreviewRect;
  slotW: number;
  slotH: number;
  moved: boolean;
}
let drag: Drag | null = null;

function cropOf(key: string, fallback: PreviewRect): PreviewRect {
  return liveCrops.value[key] ?? fallback;
}

function onPointerDown(event: PointerEvent, key: string, ref: PlacementRef, crop: PreviewRect) {
  if (!selectable || event.button !== 0) return;
  const el = event.currentTarget as HTMLElement;
  const box = el.getBoundingClientRect();
  drag = {
    key,
    ref,
    startX: event.clientX,
    startY: event.clientY,
    origin: cropOf(key, crop),
    slotW: box.width || 1,
    slotH: box.height || 1,
    moved: false,
  };
  el.setPointerCapture(event.pointerId);
}

function onPointerMove(event: PointerEvent) {
  if (!drag) return;
  const dxPx = event.clientX - drag.startX;
  const dyPx = event.clientY - drag.startY;
  if (!drag.moved && Math.hypot(dxPx, dyPx) < DRAG_THRESHOLD_PX) return;
  drag.moved = true;
  liveCrops.value = {
    ...liveCrops.value,
    [drag.key]: cropMoved(drag.origin, { dx: dxPx / drag.slotW, dy: dyPx / drag.slotH }),
  };
}

function onPointerUp(event: PointerEvent) {
  if (!drag) return;
  const finished = drag;
  drag = null;
  (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  if (finished.moved) {
    const crop = liveCrops.value[finished.key];
    if (crop) emit("crop", finished.ref, crop);
  } else {
    emit("select", finished.ref);
  }
}

function onPointerCancel() {
  if (!drag) return;
  const { [drag.key]: _dropped, ...rest } = liveCrops.value;
  liveCrops.value = rest;
  drag = null;
}

function onWheel(event: WheelEvent, key: string, ref: PlacementRef, crop: PreviewRect) {
  const factor = wheelZoomFactor(event);
  if (!selectable || factor === null) return;
  event.preventDefault();
  const next = cropZoomed(cropOf(key, crop), factor);
  liveCrops.value = { ...liveCrops.value, [key]: next };
  clearTimeout(wheelTimer);
  wheelTimer = setTimeout(() => emit("crop", ref, next), WHEEL_SETTLE_MS);
}
const WHEEL_SETTLE_MS = 250;
let wheelTimer: ReturnType<typeof setTimeout> | undefined;

// The saved book is the truth. Once the edit a gesture produced has been
// answered -- a new layout arrived, or `busy` fell back to false because the
// edit was REFUSED and the layout did not change -- the live copy is stale.
// Without the second trigger a refused drag left the box where the pointer
// let go of it while the saved book, and the error beside it, said otherwise.
watch(
  () => layout,
  () => {
    liveCrops.value = {};
    liveRects.value = {};
  },
);
watch(
  () => busy,
  (now, before) => {
    if (before && !now) {
      liveCrops.value = {};
      liveRects.value = {};
    }
  },
);

/**
 * Every slot on the page: which photo, where it lands, and which part of it
 * prints. All of it computed by `~/types/preview`, which is unit-tested --
 * a component's template is not testable in this project, so nothing but
 * assembly happens here.
 */
const boxes = computed(() => {
  if (!page) return [];
  return pageSlots(layout, page).map((box) => {
    const placement = page.placements.find((p) => p.z === box.z);
    const saved = placement?.crop ?? { x: 0, y: 0, w: 1, h: 1 };
    const savedRect = placement?.slotRect ?? { x: 0, y: 0, w: 1, h: 1 };
    const live = liveCrops.value[box.key];
    const liveRect = liveRects.value[box.key];
    return {
      ...box,
      // Thumbnails only. `tauri.conf.json`'s `assetProtocol.scope` is
      // `$APPDATA/thumbnails/*`, so originals are not loadable without widening
      // it -- and a 6718px spread is far past what WKWebView will composite.
      src: box.photo?.thumbnailPath ? convertFileSrc(box.photo.thumbnailPath) : null,
      ref: { page: page.number, z: box.z } satisfies PlacementRef,
      saved,
      savedRect,
      // Mid-gesture the live window and box are drawn; otherwise the saved ones.
      crop: live ? cropStyle(live) : box.crop,
      slot: liveRect ? rectStyle(liveRect) : box.slot,
    };
  });
});

function isSelected(ref: PlacementRef): boolean {
  return selected !== null && samePlacement(selected, ref);
}

// The three guides, as Rust derived them from the book's spec -- the same
// rects `book::score` and `book::preflight` enforce, so what is drawn is what
// was validated.
const trim = computed(() => rectStyle(shown.value[side].trim));
const safe = computed(() => rectStyle(shown.value[side].safe));
const gutter = computed(() => rectStyle(shown.value[side].gutter));
</script>

<template>
  <!--
    The page box carries the PRINTED page's real aspect (the book's own spec,
    11.197" x 8.894" by default),
    so every normalised rect inside it is a straight percentage. At the
    ~700px per page this renders at, that is 1/9.6 of print resolution.
  -->
  <div
    ref="pageEl"
    class="relative overflow-hidden"
    :style="{ aspectRatio: `${shown.pageWIn} / ${shown.pageHIn}` }"
    :class="page ? 'bg-white' : 'bg-neutral-200 dark:bg-neutral-800'"
  >
    <template v-if="page">
      <!--
        A button while a swap is possible, a plain box otherwise, so the
        photos are keyboard-reachable exactly when clicking them does
        something. The title carries the source path and the basename the
        exporter writes this placement under, so something odd on screen can
        be traced to its file and checked against `manifest.json`. The
        filename comes from Rust's `export::output_filename` -- the same
        function the exporter uses -- rather than being rebuilt here.
      -->
      <component
        :is="selectable ? 'button' : 'div'"
        v-for="box in boxes"
        :key="box.key"
        :type="selectable ? 'button' : undefined"
        class="absolute overflow-hidden bg-neutral-100 text-left"
        :class="[
          selectable && 'touch-none focus-visible:outline-2 focus-visible:outline-primary',
          selectable && (editSlots ? 'cursor-move' : 'cursor-grab'),
          isSelected(box.ref) && 'z-10 ring-3 ring-primary ring-inset',
          editSlots && 'outline-2 outline-dashed outline-primary/60 -outline-offset-2',
        ]"
        :style="box.slot"
        :title="[box.photo?.path, box.filename].filter(Boolean).join('\n')"
        :aria-label="selectable ? `Photo on page ${page.number}, slot ${box.z}: ${editSlots ? 'drag to move the box, drag a corner to resize it' : isSelected(box.ref) ? 'selected for swap' : 'click to swap, drag to move the crop, command-scroll to zoom'}` : undefined"
        :aria-pressed="selectable && !editSlots ? isSelected(box.ref) : undefined"
        @pointerdown="editSlots ? onSlotPointerDown($event, box.key, box.ref, box.savedRect, null) : onPointerDown($event, box.key, box.ref, box.saved)"
        @pointermove="editSlots ? onSlotPointerMove($event) : onPointerMove($event)"
        @pointerup="editSlots ? onSlotPointerUp($event) : onPointerUp($event)"
        @pointercancel="editSlots ? onSlotPointerCancel() : onPointerCancel()"
        @wheel="onWheel($event, box.key, box.ref, box.saved)"
        @keydown.enter.space.prevent="selectable && !editSlots && emit('select', box.ref)"
      >
        <!--
          The CROP, not the photo. The image is enlarged to 1/crop of the slot
          and slid to the window's corner, so the slot shows exactly the
          region the exporter writes. `maxWidth: none` comes from `cropStyle`
          because the CSS reset's `img { max-width: 100% }` would otherwise
          clamp it back and quietly reveal more of the photo than prints.
        -->
        <img
          v-if="box.src"
          :src="box.src"
          :alt="`Photo on page ${page.number}, slot ${box.z}`"
          draggable="false"
          loading="lazy"
          class="absolute top-0 left-0"
          :style="box.crop"
        />
        <div v-else class="flex size-full items-center justify-center">
          <UIcon name="i-lucide-image-off" class="size-4 text-neutral-500" />
        </div>
        <!--
          Corner handles, layout mode only. Each starts a resize with its
          opposite corner fixed; the slot body starts a move.
        -->
        <template v-if="selectable && editSlots">
          <span
            v-for="corner in CORNERS"
            :key="corner"
            class="absolute z-10 size-3 border border-white bg-primary shadow"
            :class="cornerClass[corner]"
            :aria-label="`Resize from the ${corner} corner`"
            role="presentation"
            @pointerdown="onSlotPointerDown($event, box.key, box.ref, box.savedRect, corner)"
            @pointermove="onSlotPointerMove"
            @pointerup="onSlotPointerUp"
            @pointercancel="onSlotPointerCancel"
          />
        </template>
      </component>

      <!--
        A page that prints white, said out loud. It is a real printed page,
        not a preview gap: the photos ran out before this slot.
      -->
      <div
        v-if="page.blank"
        class="absolute inset-0 flex items-center justify-center text-center"
      >
        <span class="text-xs text-neutral-500">This page prints blank</span>
      </div>

      <!-- Guides, above the photos and inert to the pointer. -->
      <div class="pointer-events-none absolute inset-0">
        <div class="absolute border border-dashed border-red-500/70" :style="trim" />
        <div class="absolute border border-dashed border-sky-500/70" :style="safe" />
        <div class="absolute bg-amber-500/20" :style="gutter" />
      </div>

      <span
        class="pointer-events-none absolute bottom-1 text-[10px] text-neutral-500 tabular-nums"
        :class="side === 'left' ? 'left-1.5' : 'right-1.5'"
      >
        {{ page.number }}
      </span>
    </template>

    <div v-else class="flex size-full items-center justify-center">
      <span class="text-xs text-muted">Inside {{ insideCover(side) }} cover</span>
    </div>
  </div>
</template>
