<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  canRelayout,
  leftOutPhotos,
  nextSwapStep,
  openingFor,
  pageSide,
  setCropEdit,
  spreadTemplates,
  templateLabel,
  toSpreads,
  type BookEdit,
  type BookLayout,
  type PlacementRef,
  type PreviewRect,
} from "~/types/preview";

const { layout, busy = false } = defineProps<{
  layout: BookLayout;
  /** True while a command is in flight; every control waits for it. */
  busy?: boolean;
}>();

const emit = defineEmits<{
  /** One spread-level edit for `useBook.editBook` to send and apply. */
  edit: [edit: BookEdit];
}>();

/**
 * The openings, paired with their controls. Every control here sends ONE
 * edit and shows what Rust returns; nothing is decided in the webview. What a
 * button can do is read off `PreviewOpening` -- a locked spread, or one whose
 * every layout has been shown or rejected, has its buttons disabled rather
 * than failing on click.
 */
const spreads = computed(() =>
  toSpreads(layout.pages).map((spread, index) => ({
    ...spread,
    index,
    opening: openingFor(layout, index),
  })),
);

const anyLocked = computed(() => layout.openings.some((opening) => opening.locked));
const anyToShuffle = computed(() => layout.openings.some(canRelayout));

/**
 * The photo picked for a swap, or `null`. One selection for the whole book,
 * so a photo can be swapped across spreads; the gesture itself is
 * `nextSwapStep`, which is unit-tested.
 */
const selected = ref<PlacementRef | null>(null);

function onSelect(placement: PlacementRef) {
  const step = nextSwapStep(selected.value, placement);
  selected.value = step.selected;
  if (step.edit) emit("edit", step.edit);
}

// A new layout means the swap either happened or was refused; either way the
// selection belongs to the previous book.
watch(
  () => layout,
  () => {
    selected.value = null;
  },
);

function onCrop(placement: PlacementRef, crop: PreviewRect) {
  selected.value = null;
  emit("edit", setCropEdit(placement, crop));
}

function layoutMenu(index: number, alternatives: string[]) {
  return alternatives.map((templateId) => ({
    label: templateLabel(templateId),
    onSelect: () => emit("edit", { kind: "setTemplate", opening: index, templateId }),
  }));
}

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
  <section class="space-y-4 border-t border-default pt-6" @keydown.esc="selected = null">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
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
        Shuffle re-lays every UNLOCKED opening on a layout it has not shown
        yet, with the same photos in place. Locking is how the user keeps the
        spreads they like while the rest keep changing.
      -->
      <UButton
        icon="i-lucide-shuffle"
        color="neutral"
        variant="outline"
        size="sm"
        :disabled="busy || !anyToShuffle"
        @click="emit('edit', { kind: 'shuffle' })"
      >
        {{ anyLocked ? "Shuffle unlocked spreads" : "Shuffle every spread" }}
      </UButton>
    </div>

    <!--
      What the coloured lines mean, and how to swap. Stated once here rather
      than repeated on every page: the guides are the same predicates
      `book::score` and `book::preflight` enforce, so a photo crossing one is
      a real problem, not a preview artefact.
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
      <!--
        Said on screen, not only in a doc. These are the 400px contact-sheet
        thumbnails, roughly 4x under-sampled against the 300 DPI export, so the
        preview reads sharper and cleaner than it prints. The geometry is the
        engine's own numbers and is exact; the pixels are not the pixels that
        go to the printer. Someone judging a crop needs to know which of those
        they are looking at.
      -->
      <li class="flex items-center gap-1.5">
        <UIcon name="i-lucide-info" class="size-3.5" />
        Thumbnails — placement and crop are exact, sharpness and colour are not
      </li>
      <li class="flex items-center gap-1.5">
        <UIcon name="i-lucide-arrow-left-right" class="size-3.5" />
        Click a photo, then another anywhere in the book, to swap them
      </li>
      <li class="flex items-center gap-1.5">
        <UIcon name="i-lucide-move" class="size-3.5" />
        Drag a photo to move its crop, scroll over it to zoom
      </li>
    </ul>

    <UAlert
      v-if="selected"
      icon="i-lucide-arrow-left-right"
      color="primary"
      variant="subtle"
      :title="`Swapping the photo on page ${selected.page}`"
      description="Click the photo to exchange it with. Click it again, or press Escape, to cancel."
      :actions="[
        {
          label: 'Cancel',
          icon: 'i-lucide-x',
          color: 'neutral',
          variant: 'outline',
          onClick: () => (selected = null),
        },
      ]"
    />

    <ol class="space-y-6">
      <li v-for="spread in spreads" :key="spread.key" class="space-y-1.5">
        <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          <!--
            The template id next to the page label, so a template repeating
            across consecutive openings is directly visible. It is one of the
            defects this preview exists to reveal, and without the id on screen
            it can only be inferred from layout shape.
          -->
          <p
            class="flex flex-wrap items-baseline gap-x-2 font-mono text-xs text-muted tabular-nums"
          >
            <span>{{ spread.label }}</span>
            <span v-for="id in spreadTemplates(spread)" :key="id" class="text-dimmed">{{
              id
            }}</span>
            <UBadge
              v-if="spread.opening.locked"
              icon="i-lucide-lock"
              color="primary"
              variant="subtle"
              size="sm"
              label="Locked"
            />
          </p>
          <!--
            Per-opening controls. Regenerate and reject are disabled, not
            hidden, when there is nothing else to show: a hidden button reads
            as a missing feature, a disabled one as a fact about this spread.
          -->
          <div class="flex items-center gap-0.5">
            <UTooltip text="Show a different layout for these photos">
              <UButton
                icon="i-lucide-dices"
                color="neutral"
                variant="ghost"
                size="xs"
                :aria-label="`Regenerate ${spread.label}`"
                :disabled="busy || !canRelayout(spread.opening)"
                @click="emit('edit', { kind: 'regenerate', opening: spread.index })"
              />
            </UTooltip>
            <UTooltip text="Never use this layout here again">
              <UButton
                icon="i-lucide-thumbs-down"
                color="neutral"
                variant="ghost"
                size="xs"
                :aria-label="`Reject the layout of ${spread.label}`"
                :disabled="busy || !canRelayout(spread.opening)"
                @click="emit('edit', { kind: 'rejectTemplate', opening: spread.index })"
              />
            </UTooltip>
            <UDropdownMenu
              :items="layoutMenu(spread.index, spread.opening.alternatives)"
              :content="{ align: 'end' }"
            >
              <UButton
                icon="i-lucide-layout-template"
                color="neutral"
                variant="ghost"
                size="xs"
                :aria-label="`Choose a layout for ${spread.label}`"
                :disabled="busy || !canRelayout(spread.opening)"
              />
            </UDropdownMenu>
            <UTooltip
              :text="
                spread.opening.locked
                  ? 'Unlock, so shuffle and regenerate can change it'
                  : 'Lock, so shuffle leaves it as it is'
              "
            >
              <UButton
                :icon="spread.opening.locked ? 'i-lucide-lock' : 'i-lucide-lock-open'"
                :color="spread.opening.locked ? 'primary' : 'neutral'"
                variant="ghost"
                size="xs"
                :aria-label="`${spread.opening.locked ? 'Unlock' : 'Lock'} ${spread.label}`"
                :aria-pressed="spread.opening.locked"
                :disabled="busy"
                @click="
                  emit('edit', {
                    kind: 'setLocked',
                    opening: spread.index,
                    locked: !spread.opening.locked,
                  })
                "
              />
            </UTooltip>
          </div>
        </div>
        <!--
          Both halves side by side with the fold between them. A single page
          keeps its half of the width and is drawn facing an inside cover, so
          page 1 and the last page never read as half of a spread.
        -->
        <div
          class="flex max-w-[1400px] gap-px rounded-md bg-default p-px ring ring-default"
          :class="spread.opening.locked && 'ring-primary/60'"
        >
          <div class="w-1/2">
            <BookPreviewPage
              :layout
              :page="spread.left"
              :side="pageSide(spread.left, 'left')"
              :selectable="!busy && !spread.opening.locked"
              :selected
              @select="onSelect"
              @crop="onCrop"
            />
          </div>
          <div class="w-1/2">
            <BookPreviewPage
              :layout
              :page="spread.right"
              :side="pageSide(spread.right, 'right')"
              :selectable="!busy && !spread.opening.locked"
              :selected
              @select="onSelect"
              @crop="onCrop"
            />
          </div>
        </div>
      </li>
    </ol>

    <div v-if="leftOut.length > 0" class="space-y-2">
      <h4 class="text-sm font-medium text-highlighted">Left out ({{ leftOut.length }})</h4>
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
