<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  canRelayout,
  leftOutPhotos,
  nextSwapStep,
  openingFor,
  pageSide,
  setCropEdit,
  setSlotEdit,
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

/**
 * Layout mode: drag moves a photo box and its corners resize it, instead of
 * the drag moving the crop. A mode switch rather than a modifier key so it is
 * discoverable and cannot be entered by accident mid-crop. Turning it on
 * drops any swap selection, because a click no longer selects.
 *
 * A model, so the editor's status bar can say what a drag does right now.
 */
const editSlots = defineModel<boolean>("editSlots", { default: false });
watch(editSlots, () => {
  selected.value = null;
});

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

function onSlot(placement: PlacementRef, rect: PreviewRect) {
  emit("edit", setSlotEdit(placement, rect));
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
</script>

<template>
  <section @keydown.esc="selected = null">
    <!-- The book's own toolbar, pinned over the desk while the spreads scroll under it. -->
    <div
      class="sticky top-0 z-20 flex h-11 items-center gap-3 border-b border-default bg-default/95 px-6 backdrop-blur"
    >
      <UFieldGroup size="xs" aria-label="What dragging a photo does">
        <UButton
          icon="i-lucide-crop"
          color="neutral"
          :variant="editSlots ? 'outline' : 'solid'"
          :aria-pressed="!editSlots"
          :disabled="busy"
          @click="editSlots = false"
        >
          Crop and swap
        </UButton>
        <UButton
          icon="i-lucide-move"
          color="neutral"
          :variant="editSlots ? 'solid' : 'outline'"
          :aria-pressed="editSlots"
          :disabled="busy"
          @click="editSlots = true"
        >
          Move and resize boxes
        </UButton>
      </UFieldGroup>

      <div class="ml-auto flex items-center gap-2">
        <!--
          What the coloured lines mean. Stated once here rather than repeated
          on every page: the guides are the same predicates `book::score` and
          `book::preflight` enforce, so a photo crossing one is a real problem,
          not a preview artefact.
        -->
        <UPopover :content="{ align: 'end' }">
          <UButton icon="i-lucide-ruler" color="neutral" variant="ghost" size="xs">Guides</UButton>
          <template #content>
            <ul class="w-80 space-y-2.5 p-3 text-xs text-muted">
              <li class="flex items-center gap-2">
                <span class="h-0 w-4 shrink-0 border-t border-dashed border-red-500/70" />
                Trim — cut here
              </li>
              <li class="flex items-center gap-2">
                <span class="h-0 w-4 shrink-0 border-t border-dashed border-sky-500/70" />
                Safe area — keep faces inside
              </li>
              <li class="flex items-center gap-2">
                <span class="h-2.5 w-4 shrink-0 bg-amber-500/20" />
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
              <li class="flex items-start gap-2">
                <UIcon name="i-lucide-info" class="size-4 shrink-0" />
                Thumbnails — placement and crop are exact, sharpness and colour are not
              </li>
            </ul>
          </template>
        </UPopover>
        <!--
          Shuffle re-lays every UNLOCKED opening on a layout it has not shown
          yet, with the same photos in place. Locking is how the user keeps the
          spreads they like while the rest keep changing.
        -->
        <UButton
          icon="i-lucide-shuffle"
          color="neutral"
          variant="outline"
          size="xs"
          :disabled="busy || !anyToShuffle"
          @click="emit('edit', { kind: 'shuffle' })"
        >
          {{ anyLocked ? "Shuffle unlocked spreads" : "Shuffle every spread" }}
        </UButton>
      </div>
    </div>

    <div class="mx-auto max-w-[1400px] space-y-10 p-6">
      <ol class="space-y-10">
        <li v-for="spread in spreads" :key="spread.key" class="space-y-2.5">
          <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
            <!--
              The template id next to the page label, so a template repeating
              across consecutive openings is directly visible. It is one of the
              defects this preview exists to reveal, and without the id on screen
              it can only be inferred from layout shape.
            -->
            <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted tabular-nums">
              <span class="font-medium text-toned">{{ spread.label }}</span>
              <span
                v-for="id in spreadTemplates(spread)"
                :key="id"
                class="font-mono text-[11px] text-dimmed"
                >{{ id }}</span
              >
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
            class="flex max-w-[1400px] gap-px bg-neutral-300 shadow-[0_1px_2px_rgb(0_0_0/0.08),0_8px_24px_-6px_rgb(0_0_0/0.18)] dark:bg-neutral-600 dark:shadow-[0_1px_2px_rgb(0_0_0/0.4),0_12px_32px_-8px_rgb(0_0_0/0.7)]"
            :class="spread.opening.locked && 'outline-2 outline-offset-4 outline-primary/70'"
          >
            <div class="w-1/2">
              <BookPreviewPage
                :layout
                :page="spread.left"
                :side="pageSide(spread.left, 'left')"
                :selectable="!busy && !spread.opening.locked"
                :selected
                :edit-slots="editSlots"
                :busy
                @select="onSelect"
                @crop="onCrop"
                @slot="onSlot"
              />
            </div>
            <div class="w-1/2">
              <BookPreviewPage
                :layout
                :page="spread.right"
                :side="pageSide(spread.right, 'right')"
                :selectable="!busy && !spread.opening.locked"
                :selected
                :edit-slots="editSlots"
                :busy
                @select="onSelect"
                @crop="onCrop"
                @slot="onSlot"
              />
            </div>
          </div>
        </li>
      </ol>

      <div v-if="leftOut.length > 0" class="space-y-2 border-t border-default pt-5">
        <h4 class="text-sm font-semibold text-highlighted">
          Left out <span class="font-normal text-muted tabular-nums">{{ leftOut.length }}</span>
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
    </div>

    <!--
      A swap in progress, pinned to the bottom of the desk so it stays in view
      while the user scrolls to the photo to exchange it with.
    -->
    <div
      v-if="selected"
      role="status"
      class="sticky bottom-4 z-20 mx-auto flex w-fit items-center gap-3 rounded-full bg-inverted py-1.5 pr-1.5 pl-4 text-sm text-inverted shadow-lg"
    >
      <UIcon name="i-lucide-arrow-left-right" class="size-4 shrink-0" />
      <span>
        <span class="font-medium">Swapping the photo on page {{ selected.page }}.</span>
        Click the photo to exchange it with. Click it again, or press Escape, to cancel.
      </span>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="solid"
        size="xs"
        class="rounded-full bg-default text-default hover:bg-elevated"
        @click="selected = null"
      >
        Cancel
      </UButton>
    </div>
  </section>
</template>
