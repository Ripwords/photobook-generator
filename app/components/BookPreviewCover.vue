<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cropMoved,
  cropStyle,
  cropZoomed,
  pressIsDrag,
  rectStyle,
  wheelZoomFactor,
  type BookLayout,
  type CoverSide,
  type PreviewRect,
} from "~/types/preview";

const { layout, busy = false } = defineProps<{
  layout: BookLayout;
  /** True while an edit is in flight; every control waits for it. */
  busy?: boolean;
}>();

const emit = defineEmits<{
  /** Open the photo picker for one side. */
  choose: [side: CoverSide];
  /** The user dragged or zoomed a cover photo and let go. */
  crop: [side: CoverSide, crop: PreviewRect];
  /** A new plain spine colour, `#rrggbb`. */
  spine: [rgb: string];
}>();

/**
 * The same gesture as a page photo: drag moves the window, ⌘-scroll zooms,
 * one edit on release. A press that never travels `DRAG_THRESHOLD_PX` is a
 * click, which opens the picker for that side instead of selecting for a swap
 * -- the cover holds a copy of a photo, so there is nothing to swap it with.
 *
 * Release decides, on where the pointer ENDED. A press that drifts out past
 * the threshold and comes back is a click, not a one-pixel crop nudge.
 */
const WHEEL_SETTLE_MS = 250;
const live = ref<Partial<Record<CoverSide, PreviewRect>>>({});
interface Drag {
  side: CoverSide;
  startX: number;
  startY: number;
  origin: PreviewRect;
  w: number;
  h: number;
  moved: boolean;
}
let drag: Drag | null = null;
let wheelTimer: ReturnType<typeof setTimeout> | undefined;

function onPointerDown(event: PointerEvent, side: CoverSide, crop: PreviewRect) {
  if (busy || event.button !== 0) return;
  const el = event.currentTarget as HTMLElement;
  const box = el.getBoundingClientRect();
  drag = {
    side,
    startX: event.clientX,
    startY: event.clientY,
    origin: live.value[side] ?? crop,
    w: box.width || 1,
    h: box.height || 1,
    moved: false,
  };
  el.setPointerCapture(event.pointerId);
}

function onPointerMove(event: PointerEvent) {
  if (!drag) return;
  const dx = event.clientX - drag.startX;
  const dy = event.clientY - drag.startY;
  if (!drag.moved && !pressIsDrag(dx, dy)) return;
  drag.moved = true;
  live.value = { ...live.value, [drag.side]: cropMoved(drag.origin, { dx: dx / drag.w, dy: dy / drag.h }) };
}

function onPointerUp(event: PointerEvent) {
  if (!drag) return;
  const finished = drag;
  drag = null;
  (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  const crop = live.value[finished.side];
  const dragged = pressIsDrag(event.clientX - finished.startX, event.clientY - finished.startY);
  if (dragged && crop) {
    emit("crop", finished.side, crop);
    return;
  }
  const { [finished.side]: _abandoned, ...rest } = live.value;
  live.value = rest;
  emit("choose", finished.side);
}

function onPointerCancel() {
  if (!drag) return;
  const { [drag.side]: _dropped, ...rest } = live.value;
  live.value = rest;
  drag = null;
}

function onWheel(event: WheelEvent, side: CoverSide, crop: PreviewRect) {
  const factor = wheelZoomFactor(event);
  if (busy || factor === null) return;
  event.preventDefault();
  const next = cropZoomed(live.value[side] ?? crop, factor);
  live.value = { ...live.value, [side]: next };
  clearTimeout(wheelTimer);
  wheelTimer = setTimeout(() => emit("crop", side, next), WHEEL_SETTLE_MS);
}

// As on the pages: once an edit is answered, by a new layout or by a refusal
// that leaves `busy` false and the layout unchanged, the saved book is the truth.
watch(
  () => layout,
  () => {
    live.value = {};
  },
);
watch(
  () => busy,
  (now, before) => {
    if (before && !now) live.value = {};
  },
);

/** Back on the left, front on the right, as the flat cover lies face down. */
const panels = computed(() =>
  (["back", "front"] as const).map((side) => {
    const cover = layout.cover[side];
    const photo = cover.photo;
    const shot = photo ? layout.photos[photo.photoIndex] : undefined;
    const liveCrop = live.value[side];
    return {
      side,
      label: side === "front" ? "Front" : "Back",
      board: rectStyle(cover.board),
      visible: rectStyle(cover.visible),
      photo: photo && {
        saved: photo.crop,
        crop: cropStyle(liveCrop ?? photo.crop),
        src: shot?.thumbnailPath ? convertFileSrc(shot.thumbnailPath) : null,
        title: [shot?.path, photo.filename].filter(Boolean).join("\n"),
      },
    };
  }),
);

function onSpine(event: Event) {
  const rgb = (event.target as HTMLInputElement).value.toLowerCase();
  if (rgb !== layout.cover.spine) emit("spine", rgb);
}
</script>

<template>
  <div class="space-y-2.5">
    <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
      <p class="text-xs font-medium text-toned">Cover</p>
      <!--
        A native colour well: the layout carries no palette to offer swatches
        from, and the hex beside it says exactly what the manifest records.
      -->
      <label
        class="flex items-center gap-2 text-xs text-muted"
        :class="busy ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'"
      >
        Spine colour
        <span class="relative size-5 overflow-hidden rounded ring-1 ring-default">
          <input
            type="color"
            class="absolute -inset-2 size-9 cursor-[inherit] border-0 p-0"
            :value="layout.cover.spine"
            :disabled="busy"
            aria-label="Spine colour"
            @change="onSpine"
          />
        </span>
        <span class="font-mono text-[11px] text-toned tabular-nums">{{ layout.cover.spine }}</span>
      </label>
    </div>

    <!--
      Back, spine, front, each panel at the real cover-panel aspect: the board
      plus the wrap that folds under it. The spine has no published width, so
      it is drawn at a nominal one in the chosen colour.
    -->
    <div
      class="flex items-stretch gap-px bg-neutral-300 shadow-[0_1px_2px_rgb(0_0_0/0.08),0_8px_24px_-6px_rgb(0_0_0/0.18)] dark:bg-neutral-600 dark:shadow-[0_1px_2px_rgb(0_0_0/0.4),0_12px_32px_-8px_rgb(0_0_0/0.7)]"
    >
      <template v-for="(panel, index) in panels" :key="panel.side">
        <div
          v-if="index === 1"
          class="w-5 shrink-0"
          :style="{ backgroundColor: layout.cover.spine }"
          :title="`Spine ${layout.cover.spine}`"
          aria-hidden="true"
        />
        <div
          class="relative min-w-0 flex-1 overflow-hidden bg-white"
          :style="{ aspectRatio: layout.cover.aspect }"
          :data-cover-side="panel.side"
        >
          <button
            v-if="panel.photo"
            type="button"
            class="absolute inset-0 touch-none overflow-hidden bg-neutral-100 text-left focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary"
            :class="busy ? 'cursor-wait' : 'cursor-grab'"
            :disabled="busy"
            :title="panel.photo.title"
            :aria-label="`${panel.label} cover photo: click to change or remove it, drag to move the crop, command-scroll to zoom`"
            @pointerdown="onPointerDown($event, panel.side, panel.photo.saved)"
            @pointermove="onPointerMove"
            @pointerup="onPointerUp"
            @pointercancel="onPointerCancel"
            @wheel="onWheel($event, panel.side, panel.photo.saved)"
            @keydown.enter.space.prevent="emit('choose', panel.side)"
          >
            <img
              v-if="panel.photo.src"
              :src="panel.photo.src"
              :alt="`${panel.label} cover photo`"
              draggable="false"
              class="absolute top-0 left-0"
              :style="panel.photo.crop"
            />
            <span v-else class="flex size-full items-center justify-center">
              <UIcon name="i-lucide-image-off" class="size-4 text-neutral-500" />
            </span>
          </button>
          <button
            v-else
            type="button"
            class="absolute flex flex-col items-center justify-center gap-2 text-sm text-neutral-600 transition-colors hover:bg-neutral-100 hover:text-neutral-900 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary disabled:cursor-wait"
            :style="panel.board"
            :disabled="busy"
            @click="emit('choose', panel.side)"
          >
            <UIcon name="i-lucide-image-plus" class="size-5" />
            Choose a {{ panel.side }} cover photo
          </button>

          <!--
            The wrap, shaded: everything outside the board folds under it and
            never shows. The shadow is cast outward from the board rect, so the
            shading ends exactly on the trim line drawn over it.
          -->
          <div class="pointer-events-none absolute inset-0">
            <div
              class="absolute border border-dashed border-red-500/70 shadow-[0_0_0_9999px_rgb(0_0_0/0.35)]"
              :style="panel.board"
            />
            <div class="absolute border border-dashed border-sky-500/70" :style="panel.visible" />
          </div>
        </div>
      </template>
    </div>
    <div class="flex gap-px text-[11px] text-dimmed" aria-hidden="true">
      <p class="flex-1">Back</p>
      <p class="w-5 shrink-0" />
      <p class="flex-1">Front</p>
    </div>
  </div>
</template>
