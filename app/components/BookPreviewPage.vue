<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cropMoved,
  cropStyle,
  cropZoomed,
  gutterRect,
  pageSlots,
  rectStyle,
  safeRect,
  samePlacement,
  trimRect,
  type BookLayout,
  type PageSide,
  type PlacementRef,
  type PreviewPage,
  type PreviewRect,
} from "~/types/preview";

const {
  layout,
  page = null,
  side,
  selectable = false,
  selected = null,
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
}>();

const emit = defineEmits<{
  /** The user clicked a photo while swapping -- see `nextSwapStep`. */
  select: [placement: PlacementRef];
  /** The user dragged or zoomed a photo inside its slot and let go. */
  crop: [placement: PlacementRef, crop: PreviewRect];
}>();

/**
 * Hand-cropping, as a gesture on the slot itself: drag the photo to move the
 * window, scroll to zoom it. The window is drawn live from a local copy while
 * the pointer is down and sent as ONE edit on release, so a drag is one save
 * and one round trip, not one per pixel. Rust re-derives the height from the
 * slot, keeps the window inside the photo and re-runs the hard constraints;
 * whatever it returns is what stays on screen. A press that never moved past
 * `DRAG_THRESHOLD_PX` is a click, which selects the photo for a swap.
 */
const DRAG_THRESHOLD_PX = 3;
const ZOOM_PER_WHEEL_UNIT = 0.0015;
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
  if (!selectable) return;
  event.preventDefault();
  // Scrolling up (negative deltaY) zooms in, as in every image viewer.
  const next = cropZoomed(cropOf(key, crop), Math.exp(-event.deltaY * ZOOM_PER_WHEEL_UNIT));
  liveCrops.value = { ...liveCrops.value, [key]: next };
  clearTimeout(wheelTimer);
  wheelTimer = setTimeout(() => emit("crop", ref, next), WHEEL_SETTLE_MS);
}
const WHEEL_SETTLE_MS = 250;
let wheelTimer: ReturnType<typeof setTimeout> | undefined;

// A new layout is the saved truth; whatever was being previewed is either in
// it now or was refused, and either way the live copy is stale.
watch(
  () => layout,
  () => {
    liveCrops.value = {};
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
    const live = liveCrops.value[box.key];
    return {
      ...box,
      // Thumbnails only. `tauri.conf.json`'s `assetProtocol.scope` is
      // `$APPDATA/thumbnails/*`, so originals are not loadable without widening
      // it -- and a 6718px spread is far past what WKWebView will composite.
      src: box.photo?.thumbnailPath ? convertFileSrc(box.photo.thumbnailPath) : null,
      ref: { page: page.number, z: box.z } satisfies PlacementRef,
      saved,
      // Mid-gesture the live window is drawn; otherwise the saved one.
      crop: live ? cropStyle(live) : box.crop,
    };
  });
});

function isSelected(ref: PlacementRef): boolean {
  return selected !== null && samePlacement(selected, ref);
}

// The three guides, from the constants `geometry.rs` shipped on the wire --
// the same predicates `book::score` and `book::preflight` enforce, so what is
// drawn is what was validated.
const trim = computed(() => rectStyle(trimRect(layout.geometry, side)));
const safe = computed(() => rectStyle(safeRect(layout.geometry, side)));
const gutter = computed(() => rectStyle(gutterRect(layout.geometry, side)));
</script>

<template>
  <!--
    The page box carries the PRINTED page's real aspect (11.197" x 8.894"),
    so every normalised rect inside it is a straight percentage. At the
    ~700px per page this renders at, that is 1/9.6 of print resolution.
  -->
  <div
    class="relative overflow-hidden"
    :style="{ aspectRatio: `${layout.geometry.pageWIn} / ${layout.geometry.pageHIn}` }"
    :class="page ? 'bg-white' : 'bg-elevated'"
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
        class="absolute overflow-hidden bg-neutral-100 text-left dark:bg-neutral-800"
        :class="[
          selectable && 'cursor-grab touch-none focus-visible:outline-2 focus-visible:outline-primary',
          isSelected(box.ref) && 'z-10 ring-3 ring-primary ring-inset',
        ]"
        :style="box.slot"
        :title="[box.photo?.path, box.filename].filter(Boolean).join('\n')"
        :aria-label="selectable ? `Photo on page ${page.number}, slot ${box.z}: ${isSelected(box.ref) ? 'selected for swap' : 'click to swap, drag to move the crop, scroll to zoom'}` : undefined"
        :aria-pressed="selectable ? isSelected(box.ref) : undefined"
        @pointerdown="onPointerDown($event, box.key, box.ref, box.saved)"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerCancel"
        @wheel="onWheel($event, box.key, box.ref, box.saved)"
        @keydown.enter.space.prevent="selectable && emit('select', box.ref)"
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
          loading="lazy"
          class="absolute top-0 left-0"
          :style="box.crop"
        />
        <div v-else class="flex size-full items-center justify-center">
          <UIcon name="i-lucide-image-off" class="size-4 text-muted" />
        </div>
      </component>

      <!--
        A page that prints white, said out loud. It is a real printed page,
        not a preview gap: the photos ran out before this slot.
      -->
      <div
        v-if="page.blank"
        class="absolute inset-0 flex items-center justify-center text-center"
      >
        <span class="text-xs text-neutral-400">This page prints blank</span>
      </div>

      <!-- Guides, above the photos and inert to the pointer. -->
      <div class="pointer-events-none absolute inset-0">
        <div class="absolute border border-dashed border-red-500/70" :style="trim" />
        <div class="absolute border border-dashed border-sky-500/70" :style="safe" />
        <div class="absolute bg-amber-500/20" :style="gutter" />
      </div>

      <span
        class="pointer-events-none absolute bottom-1 font-mono text-[10px] text-neutral-400 tabular-nums"
        :class="side === 'left' ? 'left-1.5' : 'right-1.5'"
      >
        {{ page.number }}
      </span>
    </template>

    <div v-else class="flex size-full items-center justify-center">
      <span class="text-xs text-muted">
        Inside {{ side === "left" ? "back" : "front" }} cover
      </span>
    </div>
  </div>
</template>
