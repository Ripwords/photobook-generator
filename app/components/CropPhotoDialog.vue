<script setup lang="ts">
import {
  cropAtZoom,
  cropStyle,
  cropZoom,
  cropZoomed,
  MAX_CROP_ZOOM,
  rectStyle,
  wheelZoomFactor,
  windowMoved,
  type PreviewRect,
} from "~/types/preview";

/**
 * Crop one photo: the whole photo, with the part that prints lit and the rest
 * dimmed. Drag the frame to move it; zoom with the slider, the buttons, or a
 * wheel or pinch over the photo. Nothing behind a modal scrolls, so the wheel
 * here can only ever mean zoom -- on the page it fought the book's scrolling.
 *
 * Nothing is saved until Save, and then as ONE edit, so a session of
 * fiddling is one entry in the undo timeline.
 */
const { src, width, height, crop, label } = defineProps<{
  /** The photo's thumbnail URL, already through `convertFileSrc`. */
  src: string | null;
  /** Oriented pixel size, the frame `crop` is normalised against. */
  width: number;
  height: number;
  /** The crop saved now. Its shape is the slot's and never changes here. */
  crop: PreviewRect;
  /** Which photo, as the dialog's title says it, e.g. "Photo on page 4". */
  label: string;
}>();

const open = defineModel<boolean>("open", { required: true });
const emit = defineEmits<{ save: [crop: PreviewRect] }>();

const draft = ref<PreviewRect>({ ...crop });
// Reopened on the same photo, it starts from what is saved now, not from
// whatever was left on screen last time.
watch(open, (now) => {
  if (now) draft.value = { ...crop };
});
const zoom = computed({
  get: () => cropZoom(draft.value),
  set: (value: number) => {
    draft.value = cropAtZoom(draft.value, value);
  },
});
const changed = computed(() =>
  (["x", "y", "w"] as const).some((k) => Math.abs(draft.value[k] - crop[k]) > 1e-6),
);

const stage = useTemplateRef<HTMLDivElement>("stage");
let drag: { x: number; y: number; origin: PreviewRect; w: number; h: number } | null = null;

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0 || !stage.value) return;
  const box = stage.value.getBoundingClientRect();
  drag = { x: event.clientX, y: event.clientY, origin: draft.value, w: box.width || 1, h: box.height || 1 };
  stage.value.setPointerCapture(event.pointerId);
}

function onPointerMove(event: PointerEvent) {
  if (!drag) return;
  draft.value = windowMoved(drag.origin, {
    dx: (event.clientX - drag.x) / drag.w,
    dy: (event.clientY - drag.y) / drag.h,
  });
}

function onPointerUp() {
  drag = null;
}

/** A trackpad pinch arrives as a Ctrl-wheel with small deltas; scale it up to feel like a pinch. */
const PINCH_GAIN = 8;

function onWheel(event: WheelEvent) {
  event.preventDefault();
  const delta = event.ctrlKey ? event.deltaY * PINCH_GAIN : event.deltaY;
  draft.value = cropZoomed(draft.value, wheelZoomFactor(delta));
  if (cropZoom(draft.value) > MAX_CROP_ZOOM) draft.value = cropAtZoom(draft.value, MAX_CROP_ZOOM);
}

const STEP = 1.25;
const NUDGE = 0.01;

defineShortcuts({
  arrowleft: () => (draft.value = windowMoved(draft.value, { dx: -NUDGE, dy: 0 })),
  arrowright: () => (draft.value = windowMoved(draft.value, { dx: NUDGE, dy: 0 })),
  arrowup: () => (draft.value = windowMoved(draft.value, { dx: 0, dy: -NUDGE })),
  arrowdown: () => (draft.value = windowMoved(draft.value, { dx: 0, dy: NUDGE })),
  enter: () => save(),
});

function save() {
  if (changed.value) emit("save", draft.value);
  open.value = false;
}

/** As large as the window allows, at the photo's own shape. */
const stageWidth = computed(() => `min(100%, calc((100dvh - 15rem) * ${width / height}))`);
const frame = computed(() => rectStyle(draft.value));
const zoomLabel = computed(() => `${zoom.value.toFixed(1)}×`);
</script>

<template>
  <UModal
    v-model:open="open"
    :title="`Crop: ${label}`"
    description="Drag the frame to choose what prints. Scroll or pinch over the photo to zoom."
    :ui="{ content: 'max-w-[calc(100vw-2rem)] w-full', body: 'flex justify-center sm:p-4', footer: 'justify-between gap-4' }"
  >
    <template #body>
      <div
        ref="stage"
        class="relative cursor-move touch-none overflow-hidden bg-neutral-100 select-none dark:bg-neutral-800"
        :style="{ width: stageWidth, aspectRatio: `${width} / ${height}` }"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
        @wheel="onWheel"
      >
        <img
          v-if="src"
          :src
          :alt="label"
          draggable="false"
          class="absolute inset-0 size-full"
        />
        <!--
          The frame: what prints. The shadow dims everything outside it, so the
          part that is cut off is still visible, just quiet.
        -->
        <div
          class="pointer-events-none absolute border-2 border-white shadow-[0_0_0_9999px_rgb(0_0_0/0.55)]"
          :style="frame"
        >
          <div class="absolute inset-y-0 left-1/3 border-l border-white/50" />
          <div class="absolute inset-y-0 left-2/3 border-l border-white/50" />
          <div class="absolute inset-x-0 top-1/3 border-t border-white/50" />
          <div class="absolute inset-x-0 top-2/3 border-t border-white/50" />
        </div>
      </div>
    </template>

    <template #footer>
      <div class="flex w-full max-w-md items-center gap-3">
        <UButton
          icon="i-lucide-zoom-out"
          color="neutral"
          variant="ghost"
          aria-label="Zoom out"
          :disabled="zoom <= 1.001"
          @click="zoom = zoom / STEP"
        />
        <USlider
          v-model="zoom"
          :min="1"
          :max="MAX_CROP_ZOOM"
          :step="0.01"
          color="neutral"
          aria-label="Zoom"
          class="flex-1"
        />
        <UButton
          icon="i-lucide-zoom-in"
          color="neutral"
          variant="ghost"
          aria-label="Zoom in"
          :disabled="zoom >= MAX_CROP_ZOOM - 0.001"
          @click="zoom = zoom * STEP"
        />
        <span class="w-12 text-sm text-muted tabular-nums">{{ zoomLabel }}</span>
      </div>
      <div class="flex items-center gap-2">
        <UButton color="neutral" variant="ghost" :disabled="!changed" @click="draft = { ...crop }">
          Reset
        </UButton>
        <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
        <UButton color="primary" @click="save">
          Save crop
          <UKbd value="enter" />
        </UButton>
      </div>
    </template>
  </UModal>
</template>
