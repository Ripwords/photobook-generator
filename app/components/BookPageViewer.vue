<script setup lang="ts">
import { pageSide, toSpreads, viewerStep, type BookLayout } from "~/types/preview";

/**
 * One opening at a time, as large as the window allows, stepped through like
 * a slideshow. Read-only: editing stays on the desk, where every control
 * for it lives. Mounted only while open, so its arrow-key shortcuts are only
 * live while it is on screen.
 */
const { layout } = defineProps<{ layout: BookLayout }>();

/** The opening on screen; the editor sets it to open the viewer, and `null` closes it. */
const index = defineModel<number | null>("index", { required: true });

const spreads = computed(() => toSpreads(layout.pages));
const current = computed(() => {
  const at = viewerStep(index.value ?? 0, 0, spreads.value.length);
  return { at, spread: spreads.value[at] };
});

function step(delta: number) {
  index.value = viewerStep(current.value.at, delta, spreads.value.length);
}

defineShortcuts({
  arrowleft: () => step(-1),
  arrowright: () => step(1),
});

/**
 * As wide as the window, unless that would make it taller than the window
 * less the modal's header and padding: then as wide as that height allows.
 */
const spreadWidth = computed(() => {
  const aspect = (2 * layout.geometry.pageWIn) / layout.geometry.pageHIn;
  return `min(100%, calc((100dvh - 11rem) * ${aspect}))`;
});

const atStart = computed(() => current.value.at === 0);
const atEnd = computed(() => current.value.at >= spreads.value.length - 1);
</script>

<template>
  <UModal
    :open="index !== null"
    :title="current.spread?.label ?? ''"
    :description="`${current.at + 1} of ${spreads.length}. Use ← and → to step through the book.`"
    :ui="{
      content: 'max-w-[calc(100vw-2rem)] w-full',
      body: 'flex items-center justify-center gap-3 sm:p-4',
    }"
    @update:open="(open) => !open && (index = null)"
  >
    <template #body>
      <UTooltip text="Previous opening" :kbds="['arrowleft']">
        <UButton
          icon="i-lucide-chevron-left"
          color="neutral"
          variant="outline"
          size="xl"
          aria-label="Previous opening"
          :disabled="atStart"
          @click="step(-1)"
        />
      </UTooltip>

      <div
        v-if="current.spread"
        class="flex min-w-0 gap-px bg-neutral-300 dark:bg-neutral-600"
        :style="{ width: spreadWidth }"
      >
        <div class="w-1/2">
          <BookPreviewPage
            :layout
            :page="current.spread.left"
            :side="pageSide(current.spread.left, 'left')"
          />
        </div>
        <div class="w-1/2">
          <BookPreviewPage
            :layout
            :page="current.spread.right"
            :side="pageSide(current.spread.right, 'right')"
          />
        </div>
      </div>

      <UTooltip text="Next opening" :kbds="['arrowright']">
        <UButton
          icon="i-lucide-chevron-right"
          color="neutral"
          variant="outline"
          size="xl"
          aria-label="Next opening"
          :disabled="atEnd"
          @click="step(1)"
        />
      </UTooltip>
    </template>
  </UModal>
</template>
