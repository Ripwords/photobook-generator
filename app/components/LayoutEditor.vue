<script setup lang="ts">
import { isPrintRule, problemSlots, type LayoutShape, type Problem } from "~/types/customLayouts";
import {
  addBox,
  canvasAspect,
  editorGuides,
  moveBox,
  resizeBox,
  SNAP,
  type Guides,
  type Handle,
} from "~/types/layoutEditor";
import type { PreviewRect } from "~/types/preview";
import type { PrintSpec } from "~/types/printSpec";

/**
 * Draws one layout's photo boxes: drag a box to move it, a corner to resize
 * it. Edges snap to the paper edge, the trim, the fold and gutter, and to the
 * other boxes. Rust checks the shape after every change and this shows what
 * it says; a structural problem blocks Save, a print-size one is a warning.
 */
const {
  shape: initialShape,
  name: initialName,
  spec,
  saving = false,
  isNew = false,
} = defineProps<{
  shape: LayoutShape;
  name: string;
  /** The size checked against and guided by: the size new books get. */
  spec: PrintSpec | null;
  saving?: boolean;
  isNew?: boolean;
}>();

const emit = defineEmits<{ save: [name: string, shape: LayoutShape]; cancel: [] }>();

const { check } = useCustomLayouts();

const shape = ref<LayoutShape>(structuredClone(toRaw(initialShape)));
const name = ref(initialName);
const selected = ref<number | null>(shape.value.boxes.length > 0 ? 0 : null);
const problems = ref<Problem[]>([]);

const pageAspect = computed(() => (spec ? spec.pageWIn / spec.pageHIn : 11.22047244094486 / 8.74015748031494));
const aspect = computed(() => canvasAspect(shape.value.kind, pageAspect.value));

const printGuides = computed<Guides>(() => {
  if (!spec) return { x: [0, 1], y: [0, 1] };
  return shape.value.kind === "spread"
    ? editorGuides({ kind: "spread" }, spec)
    : editorGuides({ kind: "page", side: shape.value.side }, spec);
});

/** The print guides plus every OTHER box's edges. */
function guidesFor(index: number): Guides {
  const others = shape.value.boxes.filter((_, i) => i !== index).map((b) => b.rect);
  return {
    x: [...printGuides.value.x, ...others.flatMap((r) => [r.x, r.x + r.w])],
    y: [...printGuides.value.y, ...others.flatMap((r) => [r.y, r.y + r.h])],
  };
}

async function recheck() {
  problems.value = await check(shape.value, spec);
}
void recheck();

const blocking = computed(() => problems.value.filter((p) => !isPrintRule(p.kind)));
const warnings = computed(() => problems.value.filter((p) => isPrintRule(p.kind)));
const flagged = computed(() => new Set(problems.value.flatMap((p) => problemSlots(p))));
const trimmedName = computed(() => name.value.trim());
const canSave = computed(() => !saving && trimmedName.value.length > 0 && blocking.value.length === 0);

function onAdd() {
  shape.value = addBox(shape.value);
  selected.value = shape.value.boxes.length - 1;
  void recheck();
}

function onRemove() {
  if (selected.value === null) return;
  shape.value = { ...shape.value, boxes: shape.value.boxes.filter((_, i) => i !== selected.value) };
  selected.value = shape.value.boxes.length > 0 ? Math.min(selected.value, shape.value.boxes.length - 1) : null;
  void recheck();
}

function onToggleHero() {
  const index = selected.value;
  if (index === null) return;
  shape.value = {
    ...shape.value,
    boxes: shape.value.boxes.map((b, i) =>
      i === index ? { ...b, role: b.role === "hero" ? "support" : "hero" } : b,
    ),
  };
}

const selectedBox = computed(() => (selected.value === null ? null : shape.value.boxes[selected.value] ?? null));

const canvas = useTemplateRef("canvas");

/** The drag in progress: which box, how, and where it started. */
let drag: {
  index: number;
  handle: Handle | null;
  start: PreviewRect;
  x: number;
  y: number;
  w: number;
  h: number;
  guides: Guides;
} | null = null;

function onPointerDown(event: PointerEvent, index: number, handle: Handle | null) {
  if (event.button !== 0 || !canvas.value) return;
  event.preventDefault();
  event.stopPropagation();
  selected.value = index;
  const bounds = canvas.value.getBoundingClientRect();
  const box = shape.value.boxes[index];
  if (!box) return;
  (event.currentTarget as Element).setPointerCapture(event.pointerId);
  drag = {
    index,
    handle,
    start: { ...box.rect },
    x: event.clientX,
    y: event.clientY,
    w: bounds.width,
    h: bounds.height,
    guides: guidesFor(index),
  };
}

function onPointerMove(event: PointerEvent) {
  if (!drag) return;
  const dx = (event.clientX - drag.x) / drag.w;
  const dy = (event.clientY - drag.y) / drag.h;
  const rect = drag.handle
    ? resizeBox(drag.start, drag.handle, dx, dy, shape.value.kind, drag.guides, SNAP)
    : moveBox(drag.start, dx, dy, drag.guides, SNAP);
  const index = drag.index;
  shape.value = { ...shape.value, boxes: shape.value.boxes.map((b, i) => (i === index ? { ...b, rect } : b)) };
}

function onPointerUp() {
  if (!drag) return;
  drag = null;
  void recheck();
}

/** Arrow keys nudge the selected box by 0.5% of the canvas, or 5% with Shift. */
function onKeydown(event: KeyboardEvent) {
  const index = selected.value;
  if (index === null) return;
  if (event.key === "Delete" || event.key === "Backspace") {
    event.preventDefault();
    onRemove();
    return;
  }
  const step = event.shiftKey ? 0.05 : 0.005;
  const moves: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  };
  const move = moves[event.key];
  const box = shape.value.boxes[index];
  if (!move || !box) return;
  event.preventDefault();
  const rect = moveBox(box.rect, move[0], move[1], { x: [], y: [] }, 0);
  shape.value = { ...shape.value, boxes: shape.value.boxes.map((b, i) => (i === index ? { ...b, rect } : b)) };
  void recheck();
}

const pct = (n: number) => `${n * 100}%`;
function boxStyle(rect: PreviewRect) {
  return { left: pct(rect.x), top: pct(rect.y), width: pct(rect.w), height: pct(rect.h) };
}

/** The dashed lines drawn on the canvas: trim and gutter. The fold is drawn solid. */
const dashedX = computed(() => printGuides.value.x.filter((x) => x > 0 && x < 1 && x !== 0.5));
const dashedY = computed(() => printGuides.value.y.filter((y) => y > 0 && y < 1));

const handles: Handle[] = ["nw", "ne", "sw", "se"];
const handleClass: Record<Handle, string> = {
  nw: "-left-1.5 -top-1.5 cursor-nwse-resize",
  ne: "-right-1.5 -top-1.5 cursor-nesw-resize",
  sw: "-bottom-1.5 -left-1.5 cursor-nesw-resize",
  se: "-right-1.5 -bottom-1.5 cursor-nwse-resize",
};

const kindLabel = computed(() =>
  shape.value.kind === "spread" ? "Spread" : `${shape.value.side === "left" ? "Left" : "Right"} page`,
);
const photoCount = computed(() => {
  const n = shape.value.boxes.length;
  return `${n} ${n === 1 ? "photo" : "photos"}`;
});

function save() {
  if (!canSave.value) return;
  emit("save", trimmedName.value, shape.value);
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center gap-3 border-b border-default px-4 py-3">
      <UButton icon="i-lucide-arrow-left" color="neutral" variant="ghost" aria-label="Back to layouts" @click="emit('cancel')" />
      <UInput v-model="name" aria-label="Layout name" placeholder="Layout name" class="w-72" @keydown.enter="save" />
      <span class="text-sm text-muted">{{ kindLabel }} · {{ photoCount }}</span>
      <div class="ml-auto flex items-center gap-2">
        <UButton color="neutral" variant="outline" @click="emit('cancel')">Cancel</UButton>
        <UButton :disabled="!canSave" :loading="saving" @click="save">{{ isNew ? "Save layout" : "Save changes" }}</UButton>
      </div>
    </header>

    <div class="flex min-h-0 flex-1">
      <div class="flex min-w-0 flex-1 flex-col items-center gap-4 overflow-auto p-6">
        <div class="flex items-center gap-1 self-center rounded-lg border border-default bg-default p-1">
          <UButton icon="i-lucide-square-plus" color="neutral" variant="ghost" @click="onAdd">Add box</UButton>
          <UButton
            icon="i-lucide-star"
            color="neutral"
            :variant="selectedBox?.role === 'hero' ? 'soft' : 'ghost'"
            :disabled="!selectedBox"
            :aria-pressed="selectedBox?.role === 'hero'"
            @click="onToggleHero"
          >
            Hero
          </UButton>
          <UButton icon="i-lucide-trash-2" color="neutral" variant="ghost" :disabled="!selectedBox" @click="onRemove">
            Remove box
          </UButton>
        </div>

        <div
          ref="canvas"
          tabindex="0"
          role="application"
          aria-label="Layout canvas. Arrow keys move the selected box; Delete removes it."
          class="relative w-full max-w-5xl shrink-0 bg-default shadow-sm ring-1 ring-default outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :style="{ aspectRatio: aspect, maxWidth: `min(64rem, calc((100vh - 15rem) * ${aspect}))` }"
          @pointerdown.self="selected = null"
          @keydown="onKeydown"
        >
          <!-- Trim and gutter lines, then the fold. -->
          <div
            v-for="x in dashedX"
            :key="`x-${x}`"
            class="pointer-events-none absolute inset-y-0 border-l border-dashed border-accented"
            :style="{ left: pct(x) }"
          />
          <div
            v-for="y in dashedY"
            :key="`y-${y}`"
            class="pointer-events-none absolute inset-x-0 border-t border-dashed border-accented"
            :style="{ top: pct(y) }"
          />
          <div v-if="shape.kind === 'spread'" class="pointer-events-none absolute inset-y-0 left-1/2 border-l border-inverted/40" />

          <div
            v-for="(box, i) in shape.boxes"
            :key="i"
            data-layout-box
            class="absolute flex cursor-move touch-none items-center justify-center bg-elevated text-sm font-medium text-toned select-none"
            :class="[
              selected === i ? 'z-10 ring-2 ring-primary' : 'ring-1 ring-accented',
              flagged.has(i) ? 'bg-error/10 ring-error' : '',
            ]"
            :style="boxStyle(box.rect)"
            @pointerdown="onPointerDown($event, i, null)"
            @pointermove="onPointerMove"
            @pointerup="onPointerUp"
            @pointercancel="onPointerUp"
          >
            <span class="flex items-center gap-1">
              <UIcon v-if="box.role === 'hero'" name="i-lucide-star" class="size-4" />
              {{ i + 1 }}
            </span>
            <template v-if="selected === i">
              <span
                v-for="handle in handles"
                :key="handle"
                class="absolute size-3 rounded-full border border-default bg-inverted"
                :class="handleClass[handle]"
                @pointerdown="onPointerDown($event, i, handle)"
                @pointermove="onPointerMove"
                @pointerup="onPointerUp"
                @pointercancel="onPointerUp"
              />
            </template>
          </div>
        </div>
        <p class="text-sm text-muted">
          Dashed lines are the trim and the gutter. Boxes snap to them, to the fold and to each other.
        </p>
      </div>

      <aside class="w-80 shrink-0 space-y-5 overflow-y-auto border-l border-default p-4 text-sm" aria-label="Layout check">
        <section v-if="blocking.length > 0" aria-labelledby="layout-blocking">
          <h2 id="layout-blocking" class="mb-2 flex items-center gap-2 font-medium text-highlighted">
            <UIcon name="i-lucide-circle-x" class="size-4 text-error" />
            Fix before saving
          </h2>
          <ul class="space-y-2 text-default">
            <li v-for="(p, i) in blocking" :key="i">{{ p.message }}</li>
          </ul>
        </section>
        <section v-if="warnings.length > 0" aria-labelledby="layout-warnings">
          <h2 id="layout-warnings" class="mb-2 flex items-center gap-2 font-medium text-highlighted">
            <UIcon name="i-lucide-triangle-alert" class="size-4" />
            Won't print at the standard size
          </h2>
          <p class="mb-2 text-muted">
            You can still save it. Books at this size won't offer it; books at a size it fits will.
          </p>
          <ul class="space-y-2 text-default">
            <li v-for="(p, i) in warnings" :key="i">{{ p.message }}</li>
          </ul>
        </section>
        <p v-if="problems.length === 0" class="flex items-center gap-2 text-default">
          <UIcon name="i-lucide-circle-check" class="size-4" />
          Prints at the standard size.
        </p>
        <section aria-labelledby="layout-help" class="space-y-2 text-muted">
          <h2 id="layout-help" class="font-medium text-highlighted">How it's used</h2>
          <p>Each box holds one photo, cropped to fit. The hero box gets the strongest photo.</p>
          <p>
            Your layouts always appear in a page's layout menu when the photo count matches. Turn on
            "Use in auto layout" in Layouts to let the book pick it on its own.
          </p>
        </section>
      </aside>
    </div>
  </div>
</template>
