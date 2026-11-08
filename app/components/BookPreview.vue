<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  canRelayout,
  chapterOpening,
  cropSubject,
  leftOutPhotos,
  nextSwapStep,
  openingFor,
  pageSide,
  setCoverCropEdit,
  setCropEdit,
  setSlotEdit,
  spreadTemplates,
  templateLabel,
  toSpreads,
  type BookEdit,
  type BookLayout,
  type CoverSide,
  type CropTarget,
  type PickTarget,
  type PlacementRef,
  type PreviewGeometry,
  type PreviewRect,
  escapeDeselects,
} from "~/types/preview";

const {
  layout,
  busy = false,
  geometry = null,
} = defineProps<{
  layout: BookLayout;
  /** True while a command is in flight; every control waits for it. */
  busy?: boolean;
  /** Guides to draw instead of the book's own -- see `BookPreviewPage`. */
  geometry?: PreviewGeometry | null;
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
    chapter: chapterOpening(layout, spread),
  })),
);

/**
 * The chapters menu: each chapter's title and pages, jumping to its header.
 * Only with a real choice -- a one-chapter book has nowhere to jump.
 */
const chapterMenu = computed(() =>
  layout.chapters.length > 1
    ? [
        layout.chapters.map((chapter) => ({
          label: chapter.title === "" ? `Chapter ${chapter.number}` : chapter.title,
          description:
            chapter.firstPage === chapter.lastPage
              ? `Page ${chapter.firstPage}`
              : `Pages ${chapter.firstPage}–${chapter.lastPage}`,
          onSelect: () =>
            document
              .getElementById(`chapter-${chapter.number}`)
              ?.scrollIntoView({ behavior: "smooth", block: "start" }),
        })),
      ]
    : null,
);

const anyLocked = computed(() => layout.openings.some((opening) => opening.locked));

/** The opening shown large in the page viewer, or `null` when it is closed. */
const viewing = ref<number | null>(null);
const anyToShuffle = computed(() => layout.openings.some(canRelayout));
const shuffleLabel = computed(() => (anyLocked.value ? "Shuffle unlocked spreads" : "Shuffle every spread"));

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

/**
 * Esc drops the selection. On the window rather than on the book, because
 * WebKit does not focus a clicked button, so the key never reaches the book.
 */
function onEscape(event: KeyboardEvent) {
  if (!selected.value) return;
  const target = event.target instanceof Element ? event.target : null;
  const press = {
    key: event.key,
    inField: !!target?.closest("input, textarea, select, [contenteditable]"),
    inOverlay: !!target?.closest('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]'),
  };
  if (escapeDeselects(press)) selected.value = null;
}
onMounted(() => window.addEventListener("keydown", onEscape));
onBeforeUnmount(() => window.removeEventListener("keydown", onEscape));

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

/**
 * The slot the "choose from all photos" dialog is filling. Its own ref, not
 * `selected`: the new layout that follows the edit clears the selection while
 * the dialog is still closing.
 */
const replaceTarget = ref<PickTarget | null>(null);
const replaceOpen = ref(false);

function chooseFromAll() {
  if (!selected.value) return;
  replaceTarget.value = { kind: "slot", placement: selected.value };
  replaceOpen.value = true;
}

/** "Add a photo…" on a blank page: the picker, then `addPhoto` for its opening. */
function addToPage(opening: number, page: number) {
  selected.value = null;
  replaceTarget.value = { kind: "opening", opening, page };
  replaceOpen.value = true;
}

function chooseCover(side: CoverSide) {
  selected.value = null;
  replaceTarget.value = { kind: "cover", side };
  replaceOpen.value = true;
}

function onCrop(placement: PlacementRef, crop: PreviewRect) {
  selected.value = null;
  emit("edit", setCropEdit(placement, crop));
}

/**
 * The photo the crop dialog is open on. Like `replaceTarget`, its own ref:
 * saving clears `selected` while the dialog is still closing.
 */
const cropTarget = ref<CropTarget | null>(null);
const cropOpen = ref(false);
const cropping = computed(() => (cropTarget.value ? cropSubject(layout, cropTarget.value) : null));

function openCrop(target: CropTarget) {
  cropTarget.value = target;
  cropOpen.value = true;
}

function onCropSaved(crop: PreviewRect) {
  const target = cropTarget.value;
  if (!target) return;
  if (target.kind === "cover") onCoverCrop(target.side, crop);
  else onCrop(target.placement, crop);
}

function onCoverCrop(side: CoverSide, crop: PreviewRect) {
  emit("edit", setCoverCropEdit(side, crop));
}

function onSpine(rgb: string) {
  emit("edit", { kind: "setSpineColour", rgb });
}

function onSlot(placement: PlacementRef, rect: PreviewRect) {
  emit("edit", setSlotEdit(placement, rect));
}

/**
 * The layout menu, headed by the template ids this opening uses now. The ids
 * used to sit beside the page label, where a template repeating across
 * consecutive openings was visible at a glance; they are machine data, so
 * they moved here, one click away for whoever is chasing that defect.
 */
function layoutMenu(index: number, alternatives: string[], current: string[]) {
  const [shown] = current;
  return [
    [
      {
        type: "label" as const,
        label: shown ? `Now: ${templateLabel(shown)}` : "Now",
        description: current.join(", "),
        sketch: shown ? layout.sketches[shown] : undefined,
      },
    ],
    alternatives.map((templateId) => ({
      label: templateLabel(templateId),
      sketch: layout.sketches[templateId],
      onSelect: () => emit("edit", { kind: "setTemplate", opening: index, templateId }),
    })),
  ];
}

/** The book's page width over its height, for the layout menu's sketches. */
const pageAspect = computed(() => layout.geometry.pageWIn / layout.geometry.pageHIn);

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
  <section class="@container">
    <!-- The book's own toolbar, pinned over the desk while the spreads scroll under it. -->
    <div
      class="sticky top-0 z-20 flex h-11 items-center gap-3 border-b border-default bg-default/95 px-6 backdrop-blur"
    >
      <!--
        What a drag does in each mode is said in its tooltip, not in a status line.
        Labels fold to icons as the column narrows (measured at md: the modes need
        ~30rem with icon-only actions, everything labelled needs ~42rem, and
        ~54rem with Chapters labelled too), so the bar never wraps; each button
        keeps its name as an aria-label.
      -->
      <UFieldGroup aria-label="What dragging a photo does">
        <UTooltip
          text="Drag a photo to move its crop, ⌘-scroll over it to zoom. Click a photo, then another anywhere in the book, to swap them"
          :content="{ side: 'bottom', align: 'start' }"
        >
          <UButton
            icon="i-lucide-crop"
            color="neutral"
            :variant="editSlots ? 'outline' : 'solid'"
            :aria-pressed="!editSlots"
            :disabled="busy"
            aria-label="Crop and swap"
            @click="editSlots = false"
          >
            <span class="hidden @[30rem]:inline">Crop and swap</span>
          </UButton>
        </UTooltip>
        <UTooltip
          text="Drag a box to move it, drag a corner to resize it; edges and centres snap to the guides"
          :content="{ side: 'bottom', align: 'start' }"
        >
          <UButton
            icon="i-lucide-move"
            color="neutral"
            :variant="editSlots ? 'solid' : 'outline'"
            :aria-pressed="editSlots"
            :disabled="busy"
            aria-label="Move and resize boxes"
            @click="editSlots = true"
          >
            <span class="hidden @[30rem]:inline">Move and resize boxes</span>
          </UButton>
        </UTooltip>
      </UFieldGroup>

      <div class="ml-auto flex items-center gap-2">
        <UDropdownMenu v-if="chapterMenu" :items="chapterMenu" :content="{ align: 'end' }">
          <UTooltip text="Jump to a chapter">
            <UButton icon="i-lucide-list" color="neutral" variant="ghost" aria-label="Chapters">
              <span class="hidden @[54rem]:inline">Chapters</span>
            </UButton>
          </UTooltip>
        </UDropdownMenu>
        <!--
          What the coloured lines mean. Stated once here rather than repeated
          on every page: the guides are the same predicates `book::score` and
          `book::preflight` enforce, so a photo crossing one is a real problem,
          not a preview artefact.
        -->
        <UPopover :content="{ align: 'end' }">
          <UTooltip text="Guides">
            <UButton icon="i-lucide-ruler" color="neutral" variant="ghost" aria-label="Guides">
              <span class="hidden @[42rem]:inline">Guides</span>
            </UButton>
          </UTooltip>
          <template #content>
            <ul class="w-80 space-y-2.5 p-3 text-sm text-muted">
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
              <li class="flex items-center gap-2">
                <span class="h-2.5 w-4 shrink-0 bg-black/35" />
                Cover wrap — folds under the board, never shows
              </li>
              <li class="flex items-center gap-2">
                <span class="h-2.5 w-4 shrink-0 border border-dashed border-violet-500" />
                Title spot — not printed; add the title here in Pixajoy if you want one
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
        <UTooltip :text="shuffleLabel">
          <UButton
            icon="i-lucide-shuffle"
            color="neutral"
            variant="outline"
            :aria-label="shuffleLabel"
            :disabled="busy || !anyToShuffle"
            @click="emit('edit', { kind: 'shuffle' })"
          >
            <span class="hidden @[42rem]:inline">{{ shuffleLabel }}</span>
          </UButton>
        </UTooltip>
      </div>
    </div>

    <div class="mx-auto max-w-[1400px] space-y-10 p-6">
      <BookPreviewCover
        :layout
        :busy
        @choose="chooseCover"
        @crop="onCoverCrop"
        @crop-open="openCrop({ kind: 'cover', side: $event })"
        @spine="onSpine"
      />
      <ol class="space-y-10">
        <li
          v-for="spread in spreads"
          :id="spread.chapter ? `chapter-${spread.chapter.number}` : undefined"
          :key="spread.key"
          class="scroll-mt-16 space-y-2.5"
        >
          <BookChapterHeader
            v-if="spread.chapter"
            :chapter="spread.chapter"
            :busy
            class="mb-4"
            @edit="emit('edit', $event)"
          />
          <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
            <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted tabular-nums">
              <span class="font-medium text-toned">{{ spread.label }}</span>
              <UBadge
                v-if="spread.opening.locked"
                icon="i-lucide-lock"
                color="primary"
                variant="subtle"
                size="lg"
                label="Locked"
              />
            </p>
            <!--
              Per-opening controls. Regenerate and reject are disabled, not
              hidden, when there is nothing else to show: a hidden button reads
              as a missing feature, a disabled one as a fact about this spread.
            -->
            <div class="flex items-center gap-0.5">
              <UTooltip text="View larger, and step through the book">
                <UButton
                  icon="i-lucide-maximize-2"
                  color="neutral"
                  variant="ghost"
                  :aria-label="`View ${spread.label} larger`"
                  @click="viewing = spread.index"
                />
              </UTooltip>
              <UTooltip text="Show a different layout for these photos">
                <UButton
                  icon="i-lucide-dices"
                  color="neutral"
                  variant="ghost"
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
                  :aria-label="`Reject the layout of ${spread.label}`"
                  :disabled="busy || !canRelayout(spread.opening)"
                  @click="emit('edit', { kind: 'rejectTemplate', opening: spread.index })"
                />
              </UTooltip>
              <UDropdownMenu
                :items="layoutMenu(spread.index, spread.opening.alternatives, spreadTemplates(spread))"
                :content="{ align: 'end' }"
                :ui="{ content: 'max-h-[70vh] min-w-72', item: 'gap-3 py-2', label: 'flex items-center gap-3 py-2' }"
              >
                <template #item-leading="{ item }">
                  <TemplateSketch v-if="item.sketch" :pages="item.sketch" :aspect="pageAspect" />
                </template>
                <UButton
                  icon="i-lucide-layout-template"
                  color="neutral"
                  variant="ghost"
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
                :geometry
                :page="spread.left"
                :side="pageSide(spread.left, 'left')"
                :selectable="!busy && !spread.opening.locked"
                :selected
                :edit-slots="editSlots"
                :busy
                @select="onSelect"
                @crop="onCrop"
                @slot="onSlot"
                @add="spread.left && addToPage(spread.index, spread.left.number)"
              />
            </div>
            <div class="w-1/2">
              <BookPreviewPage
                :layout
                :geometry
                :page="spread.right"
                :side="pageSide(spread.right, 'right')"
                :selectable="!busy && !spread.opening.locked"
                :selected
                :edit-slots="editSlots"
                :busy
                @select="onSelect"
                @crop="onCrop"
                @slot="onSlot"
                @add="spread.right && addToPage(spread.index, spread.right.number)"
              />
            </div>
          </div>
        </li>
      </ol>

      <BookPageViewer v-if="viewing !== null" v-model:index="viewing" :layout />

      <div v-if="leftOut.length > 0" class="space-y-2 border-t border-default pt-5">
        <h4 class="text-sm font-semibold text-highlighted">
          Left out <span class="font-normal text-muted tabular-nums">{{ leftOut.length }}</span>
        </h4>
        <p class="text-sm text-muted">
          Culled, trimmed to fit the page count, or not placeable in any template.
        </p>
        <ul class="flex flex-wrap gap-2">
          <li
            v-for="photo in leftOut"
            :key="photo.path"
            class="size-16 overflow-hidden rounded-sm bg-elevated ring ring-default"
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
      class="sticky bottom-4 z-20 mx-auto flex w-fit max-w-[calc(100%-3rem)] items-center gap-3 rounded-lg border border-default bg-default/95 py-1.5 pr-1.5 pl-3 text-sm shadow-lg backdrop-blur"
    >
      <UIcon name="i-lucide-arrow-left-right" class="size-4 shrink-0 text-muted" />
      <p class="min-w-0 truncate whitespace-nowrap">
        <span class="font-medium text-highlighted">Photo on page {{ selected.page }} selected.</span>
        <span class="hidden text-muted @2xl:inline"> Click another photo to swap them.</span>
      </p>
      <span class="h-5 shrink-0 border-l border-default" aria-hidden="true" />
      <div class="flex shrink-0 items-center gap-1">
        <UButton
          icon="i-lucide-crop"
          color="neutral"
          variant="outline"
          :disabled="busy"
          @click="openCrop({ kind: 'slot', placement: selected })"
        >
          Crop…
        </UButton>
        <UButton
          icon="i-lucide-images"
          color="neutral"
          variant="outline"
          :disabled="busy"
          @click="chooseFromAll"
        >
          Choose any photo…
        </UButton>
        <UButton color="neutral" variant="ghost" @click="selected = null">
          Cancel
          <UKbd value="esc" />
        </UButton>
      </div>
    </div>

    <CropPhotoDialog
      v-if="cropping"
      :key="JSON.stringify(cropTarget)"
      v-model:open="cropOpen"
      :src="cropping.photo.thumbnailPath ? convertFileSrc(cropping.photo.thumbnailPath) : null"
      :width="cropping.photo.width"
      :height="cropping.photo.height"
      :crop="cropping.crop"
      :label="cropping.label"
      @save="onCropSaved"
    />

    <ReplacePhotoDialog
      v-if="replaceTarget"
      v-model:open="replaceOpen"
      :layout
      :target="replaceTarget"
      @edit="emit('edit', $event)"
    />
  </section>
</template>
