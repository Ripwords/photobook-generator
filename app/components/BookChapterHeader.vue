<script setup lang="ts">
import { dateRange } from "~/types/dates";
import type { PreviewChapter } from "~/types/preview";

const { chapter } = defineProps<{ chapter: PreviewChapter }>();

const dates = computed(() =>
  chapter.start !== null && chapter.end !== null ? dateRange(chapter.start, chapter.end) : null,
);
const pages = computed(() =>
  chapter.firstPage === chapter.lastPage
    ? `Page ${chapter.firstPage}`
    : `Pages ${chapter.firstPage}–${chapter.lastPage}`,
);
</script>

<template>
  <header class="flex flex-col items-start gap-1.5 border-t border-default pt-5">
    <p class="flex flex-wrap items-center gap-x-2 text-sm text-muted tabular-nums">
      <span class="font-medium text-toned">Chapter {{ chapter.number }}</span>
      <span aria-hidden="true">·</span>
      <span>{{ pages }}</span>
      <template v-if="dates">
        <span aria-hidden="true">·</span>
        <span>{{ dates }}</span>
      </template>
      <template v-if="chapter.events.length > 1">
        <span aria-hidden="true">·</span>
        <span>{{ chapter.events.length }} events</span>
      </template>
    </p>
  </header>
</template>
