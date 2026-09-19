<script setup lang="ts">
import { formatLength, specDiagram, type LengthUnit, type PrintSizeField, type PrintSpec } from "~/types/printSpec";

/**
 * A labelled spread for the Print size panel: bleed, safe margin and fold as
 * the typed numbers describe them. The field being edited lights up its band.
 * Drawn in the editor guides' colours, so the legend also explains them.
 */
const {
  spec,
  unit,
  active = null,
} = defineProps<{
  spec: PrintSpec;
  unit: LengthUnit;
  active?: PrintSizeField | null;
}>();

const d = computed(() => specDiagram(spec));
const aspect = computed(() => d.value.width / d.value.height);
/** Dash length in viewBox inches, so the dashes scale with the drawing. */
const dash = computed(() => `${d.value.width * 0.012} ${d.value.width * 0.008}`);
const len = (inches: number) => `${formatLength(inches, unit)} ${unit}`;

const legend = computed(() => [
  { key: "bleed", label: "Bleed", value: len(spec.bleedIn), swatch: "bg-red-500/60" },
  { key: "safeMargin", label: "Safe margin", value: len(spec.safeMarginIn), swatch: "bg-sky-500/60" },
  { key: "fold", label: "Fold", value: len(spec.gutterIn), swatch: "bg-amber-500/60" },
]);

function strength(key: PrintSizeField, rest: string, lit: string): string {
  return active === key ? lit : rest;
}
</script>

<template>
  <figure class="space-y-2" aria-label="Diagram of one spread with its bleed, safe margin and fold">
    <div class="flex items-center justify-center gap-2">
      <div class="min-w-0" :style="{ width: `min(100%, calc(10rem * ${aspect}))` }">
        <div
          class="mb-1 w-1/2 text-center text-[11px] tabular-nums"
          :class="active === 'trimW' ? 'font-medium text-highlighted' : 'text-muted'"
        >
          {{ len(spec.pageWIn - spec.bleedIn) }}
        </div>
        <svg :viewBox="`0 0 ${d.width} ${d.height}`" class="block h-auto w-full" aria-hidden="true">
          <template v-for="side in ['left', 'right'] as const" :key="side">
            <rect
              :x="d[side].page.x"
              :y="d[side].page.y"
              :width="d[side].page.w"
              :height="d[side].page.h"
              class="fill-white"
            />
            <rect
              :x="d[side].page.x"
              :y="d[side].page.y"
              :width="d[side].page.w"
              :height="d[side].page.h"
              :class="strength('bleed', 'fill-red-500/25', 'fill-red-500/60')"
            />
            <rect
              :x="d[side].trim.x"
              :y="d[side].trim.y"
              :width="d[side].trim.w"
              :height="d[side].trim.h"
              :class="strength('safeMargin', 'fill-sky-500/15', 'fill-sky-500/50')"
            />
            <rect
              :x="d[side].safe.x"
              :y="d[side].safe.y"
              :width="d[side].safe.w"
              :height="d[side].safe.h"
              class="fill-white"
            />
            <rect
              :x="d[side].fold.x"
              :y="d[side].fold.y"
              :width="d[side].fold.w"
              :height="d[side].fold.h"
              :class="strength('fold', 'fill-amber-500/30', 'fill-amber-500/70')"
            />
            <rect
              :x="d[side].trim.x"
              :y="d[side].trim.y"
              :width="d[side].trim.w"
              :height="d[side].trim.h"
              fill="none"
              class="stroke-red-500"
              vector-effect="non-scaling-stroke"
              :stroke-dasharray="dash"
            />
            <rect
              :x="d[side].safe.x"
              :y="d[side].safe.y"
              :width="d[side].safe.w"
              :height="d[side].safe.h"
              fill="none"
              class="stroke-sky-500"
              vector-effect="non-scaling-stroke"
              :stroke-dasharray="dash"
            />
          </template>
          <line
            :x1="d.width / 2"
            :x2="d.width / 2"
            y1="0"
            :y2="d.height"
            class="stroke-neutral-500"
            vector-effect="non-scaling-stroke"
          />
        </svg>
      </div>
      <div
        class="shrink-0 text-[11px] tabular-nums [writing-mode:vertical-rl]"
        :class="active === 'trimH' ? 'font-medium text-highlighted' : 'text-muted'"
      >
        {{ len(spec.pageHIn - 2 * spec.bleedIn) }}
      </div>
    </div>
    <figcaption class="space-y-1">
      <ul class="flex flex-wrap justify-center gap-x-4 gap-y-1 text-xs">
        <li
          v-for="item in legend"
          :key="item.key"
          class="flex items-center gap-1.5"
          :class="active === item.key ? 'text-highlighted' : 'text-muted'"
        >
          <span class="size-2.5 shrink-0 rounded-sm" :class="item.swatch" />
          {{ item.label }} <span class="tabular-nums">{{ item.value }}</span>
        </li>
      </ul>
      <p v-if="d.exaggerated" class="text-center text-[11px] text-dimmed">
        Thin margins are drawn wider than scale.
      </p>
    </figcaption>
  </figure>
</template>
