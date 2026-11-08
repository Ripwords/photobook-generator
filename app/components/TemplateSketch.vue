<script setup lang="ts">
import { layoutSketch, type PreviewRect } from "~/types/preview";

/**
 * A template drawn small, so the layout menu shows what a layout looks like
 * instead of only naming it. Decorative: the menu item beside it carries the
 * name, so the drawing is hidden from assistive technology.
 */
const { pages, aspect, large = false } = defineProps<{
  /** The template's `sketches` entry: one list of slot rects per page. */
  pages: PreviewRect[][];
  /** The book's page width over its height. */
  aspect: number;
  /** Card size, for the Layouts screen; the menu's is the default. */
  large?: boolean;
}>();

const sketch = computed(() => layoutSketch(pages, aspect));
</script>

<template>
  <svg :viewBox="sketch.viewBox" class="w-auto shrink-0" :class="large ? 'h-24 max-w-full' : 'h-10'" aria-hidden="true">
    <rect
      v-for="(page, i) in sketch.pages"
      :key="`page-${i}`"
      :x="page.x"
      :y="page.y"
      :width="page.w"
      :height="page.h"
      class="fill-(--ui-bg) stroke-(--ui-border-accented)"
      stroke-width="1"
      vector-effect="non-scaling-stroke"
    />
    <rect
      v-for="(slot, i) in sketch.slots"
      :key="`slot-${i}`"
      :x="slot.x + 3"
      :y="slot.y + 3"
      :width="Math.max(slot.w - 6, 1)"
      :height="Math.max(slot.h - 6, 1)"
      class="fill-(--ui-text-dimmed)"
    />
  </svg>
</template>
