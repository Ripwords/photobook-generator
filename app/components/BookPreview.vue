<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { leftOutPhotos, toSpreads, type BookLayout } from "~/types/preview";

const { layout } = defineProps<{ layout: BookLayout }>();

/**
 * READ-ONLY, deliberately. No swap, lock, reject, regenerate or drag-editing.
 * Those are worth designing once someone has looked at a real book and knows
 * which of them they would actually reach for -- and one of them
 * (regenerate) is blocked on the seed never reaching `best_spread`, see
 * `docs/PROJECT-STATUS.md` open item 3.
 */
const spreads = computed(() => toSpreads(layout.pages));

/**
 * The photos no page placed. `Book::dropped` is only a count; the layout
 * carries the whole photo list precisely so the count can be shown as
 * pictures.
 */
const leftOut = computed(() =>
  leftOutPhotos(layout).map((photo) => ({
    ...photo,
    src: photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : null,
  })),
);

/** How many printed pages come out white -- see `BookPreviewPage`. */
const blankPages = computed(() => layout.pages.filter((page) => page.blank).length);
</script>

<template>
  <section class="space-y-4 border-t border-default pt-6">
    <div class="flex flex-wrap items-baseline justify-between gap-3">
      <h3 class="text-sm font-medium text-highlighted">The book</h3>
      <p class="text-xs text-muted">
        <span class="font-mono tabular-nums text-default">{{ layout.pageCount }}</span> pages ·
        <span class="font-mono tabular-nums text-default">{{ layout.placedPhotos }}</span> photos
        placed
        <template v-if="blankPages > 0">
          ·
          <span class="font-mono tabular-nums text-default">{{ blankPages }}</span>
          {{ blankPages === 1 ? "page prints" : "pages print" }} blank
        </template>
        <template v-if="layout.droppedPhotos > 0">
          ·
          <span class="font-mono tabular-nums text-default">{{ layout.droppedPhotos }}</span> left
          out
        </template>
      </p>
    </div>

    <!--
      What the coloured lines mean. Stated once here rather than repeated on
      every page: the guides are the same predicates `book::score` and
      `book::preflight` enforce, so a photo crossing one is a real problem,
      not a preview artefact.
    -->
    <ul class="flex flex-wrap gap-x-5 gap-y-1.5 text-xs text-muted">
      <li class="flex items-center gap-1.5">
        <span class="h-0 w-4 border-t border-dashed border-red-500/70" />
        Trim — cut here
      </li>
      <li class="flex items-center gap-1.5">
        <span class="h-0 w-4 border-t border-dashed border-sky-500/70" />
        Safe area — keep faces inside
      </li>
      <li class="flex items-center gap-1.5">
        <span class="h-2.5 w-4 bg-amber-500/20" />
        Gutter — curls into the binding
      </li>
    </ul>

    <ol class="space-y-6">
      <li v-for="spread in spreads" :key="spread.key" class="space-y-1.5">
        <p class="font-mono text-xs text-muted tabular-nums">{{ spread.label }}</p>
        <!--
          Both halves side by side with the fold between them. A single page
          keeps its half of the width and is drawn facing an inside cover, so
          page 1 and the last page never read as half of a spread.
        -->
        <div class="flex max-w-[1400px] gap-px rounded-md bg-default p-px ring ring-default">
          <div class="w-1/2">
            <BookPreviewPage :layout :page="spread.left" side="left" />
          </div>
          <div class="w-1/2">
            <BookPreviewPage :layout :page="spread.right" side="right" />
          </div>
        </div>
      </li>
    </ol>

    <div v-if="leftOut.length > 0" class="space-y-2">
      <h4 class="text-sm font-medium text-highlighted">
        Left out ({{ leftOut.length }})
      </h4>
      <p class="text-xs text-muted">
        Culled, trimmed to fit the page count, or not placeable in any template.
      </p>
      <ul class="flex flex-wrap gap-2">
        <li
          v-for="photo in leftOut"
          :key="photo.path"
          class="size-16 overflow-hidden rounded bg-elevated ring ring-default"
          :title="photo.path"
        >
          <img
            v-if="photo.src"
            :src="photo.src"
            alt=""
            loading="lazy"
            class="size-full object-cover opacity-45 grayscale"
          />
          <div v-else class="flex size-full items-center justify-center">
            <UIcon name="i-lucide-image-off" class="size-4 text-muted" />
          </div>
        </li>
      </ul>
    </div>
  </section>
</template>
