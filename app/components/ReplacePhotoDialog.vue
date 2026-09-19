<script setup lang="ts">
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  cropStyle,
  refusalText,
  replaceCandidates,
  type BookEdit,
  type BookLayout,
  type CandidateFilter,
  type CandidateRow,
  type CandidateSort,
  type PlacementRef,
  type SlotCandidate,
} from "~/types/preview";

/**
 * Picks any analysed photo for one slot: one the engine left out, or one
 * already in the book, which then trades places. Each tile shows the photo
 * cropped as this slot would print it, and a photo a hard constraint refuses
 * says why instead of failing after the click. Rust decides both (see
 * `slot_candidates`); the dialog only filters and sorts what it is told.
 */
const open = defineModel<boolean>("open", { required: true });

const { layout, target } = defineProps<{
  layout: BookLayout;
  /** The slot being filled. */
  target: PlacementRef;
}>();

const emit = defineEmits<{ edit: [edit: BookEdit] }>();

const candidates = ref<SlotCandidate[] | null>(null);
const loadError = ref<string | null>(null);
const filter = ref<CandidateFilter>("leftOut");
const sort = ref<CandidateSort>("best");
const chosen = ref<number | null>(null);
/** Only the latest request may land, so a slow answer for another slot never shows here. */
let request = 0;

async function load() {
  const mine = ++request;
  candidates.value = null;
  loadError.value = null;
  try {
    const answer = await invoke<SlotCandidate[]>("slot_candidates", {
      projectId: layout.projectId,
      placement: target,
    });
    if (mine === request) candidates.value = answer;
  } catch (e) {
    if (mine === request) loadError.value = String(e);
  }
}

// Immediate: the first open mounts the dialog already open.
watch(
  open,
  (isOpen) => {
    if (!isOpen) return;
    chosen.value = null;
    filter.value = "leftOut";
    void load();
  },
  { immediate: true },
);

const rows = computed(() =>
  candidates.value ? replaceCandidates(layout, candidates.value, target, filter.value, sort.value) : [],
);

const counts = computed(() => {
  const all = candidates.value ? replaceCandidates(layout, candidates.value, target, "all", "best") : [];
  const inBook = all.filter((row) => row.placedAt).length;
  return { leftOut: all.length - inBook, inBook, all: all.length };
});

const FILTERS: { value: CandidateFilter; label: string }[] = [
  { value: "leftOut", label: "Left out" },
  { value: "inBook", label: "In the book" },
  { value: "all", label: "All" },
];
const SORTS = [
  { value: "best", label: "Best first" },
  { value: "taken", label: "Time taken" },
];

/** The slot's printed shape, so every tile is framed the way the page frames it. */
const aspect = computed(() => {
  const page = layout.pages.find((p) => p.number === target.page);
  const rect = page?.placements.find((p) => p.z === target.z)?.slotRect;
  if (!rect) return 1;
  return (rect.w * layout.geometry.pageWIn) / (rect.h * layout.geometry.pageHIn);
});

function blocked(row: CandidateRow): string | null {
  if (row.current) return "In this slot now";
  if (row.locked) return "On a locked page";
  return row.refused ? refusalText(row.refused) : null;
}

const tiles = computed(() =>
  rows.value.map((row) => ({
    ...row,
    path: row.photo.path,
    src: row.photo.thumbnailPath ? convertFileSrc(row.photo.thumbnailPath) : null,
    style: cropStyle(row.crop),
    blocked: blocked(row),
  })),
);

const selection = computed(() => tiles.value.find((tile) => tile.index === chosen.value) ?? null);

const confirmLabel = computed(() =>
  selection.value?.placedAt ? `Swap with page ${selection.value.placedAt.page}` : "Replace",
);

function choose(tile: { index: number; blocked: string | null }) {
  if (tile.blocked === null) chosen.value = tile.index;
}

function confirm() {
  const tile = selection.value;
  if (!tile || tile.blocked !== null) return;
  emit("edit", { kind: "replacePhoto", placement: target, photo: tile.index });
  open.value = false;
}

function name(path: string): string {
  return path.split("/").pop() ?? path;
}

const scroller = useTemplateRef<HTMLElement>("scroller");
</script>

<template>
  <UModal
    v-model:open="open"
    :title="`Replace the photo on page ${target.page}`"
    description="Each photo is shown as this slot would print it."
    :ui="{
      content: 'sm:max-w-4xl',
      body: 'flex min-h-0 flex-col overflow-hidden p-0 sm:p-0',
      footer: 'justify-between',
    }"
  >
    <template #body>
      <div class="flex shrink-0 flex-wrap items-center gap-3 border-b border-default px-4 py-3 sm:px-6">
        <UFieldGroup size="sm" aria-label="Which photos to show">
          <UButton
            v-for="option in FILTERS"
            :key="option.value"
            color="neutral"
            :variant="filter === option.value ? 'solid' : 'outline'"
            :aria-pressed="filter === option.value"
            @click="filter = option.value"
          >
            {{ option.label }}
            <span class="tabular-nums opacity-70">{{ counts[option.value] }}</span>
          </UButton>
        </UFieldGroup>
        <USelect v-model="sort" :items="SORTS" size="sm" class="ml-auto w-36" aria-label="Sort photos" />
      </div>

      <div
        ref="scroller"
        class="h-[560px] min-h-0 shrink overflow-y-auto px-4 py-4 sm:px-6"
        @keydown.enter.prevent="confirm"
      >
        <div v-if="loadError" role="alert" class="flex h-full flex-col items-center justify-center gap-3 text-center">
          <p class="text-sm text-highlighted">These photos could not be loaded.</p>
          <p class="max-w-md text-xs text-muted">{{ loadError }}</p>
          <UButton color="neutral" variant="outline" size="sm" icon="i-lucide-rotate-cw" @click="load">
            Try again
          </UButton>
        </div>

        <div
          v-else-if="!candidates"
          class="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3"
          aria-busy="true"
          aria-label="Loading photos"
        >
          <USkeleton v-for="n in 12" :key="n" class="aspect-square rounded-md" />
        </div>

        <div
          v-else-if="tiles.length === 0"
          class="flex h-full flex-col items-center justify-center gap-3 text-center"
        >
          <p class="text-sm text-highlighted">
            {{ filter === "leftOut" ? "Every photo is already in the book." : "No photos to show." }}
          </p>
          <UButton
            v-if="filter === 'leftOut'"
            color="neutral"
            variant="outline"
            size="sm"
            @click="filter = 'inBook'"
          >
            Show photos in the book
          </UButton>
        </div>

        <ContactSheet
          v-else
          :groups="[{ eventCluster: 0, photos: tiles }]"
          :tile-size="140"
          :scroll-element="scroller"
          :headers="false"
        >
          <template #tile="{ photo: tile }">
            <button
              type="button"
              class="group relative flex aspect-square flex-col overflow-hidden rounded-md bg-elevated text-left ring-offset-2 ring-offset-default outline-none focus-visible:ring-2 focus-visible:ring-inverted"
              :class="[
                tile.index === chosen ? 'ring-2 ring-inverted' : 'ring-1 ring-default',
                tile.blocked ? 'cursor-not-allowed' : 'cursor-pointer',
              ]"
              :aria-pressed="tile.index === chosen"
              :aria-disabled="tile.blocked !== null"
              :aria-label="`${name(tile.photo.path)}${tile.placedAt ? `, on page ${tile.placedAt.page}` : ''}${tile.blocked ? `. ${tile.blocked}` : ''}`"
              :title="tile.blocked ?? tile.photo.path"
              @click="choose(tile)"
              @dblclick="choose(tile), confirm()"
            >
              <div class="flex min-h-0 flex-1 items-center justify-center p-2">
                <div
                  class="relative max-h-full max-w-full overflow-hidden bg-default"
                  :class="aspect >= 1 ? 'w-full' : 'h-full'"
                  :style="{ aspectRatio: aspect }"
                >
                  <img
                    v-if="tile.src"
                    :src="tile.src"
                    alt=""
                    loading="lazy"
                    class="absolute top-0 left-0"
                    :class="{ 'opacity-40 grayscale': tile.blocked }"
                    :style="tile.style"
                  />
                  <div v-else class="flex size-full items-center justify-center">
                    <UIcon name="i-lucide-image-off" class="size-4 text-muted" />
                  </div>
                </div>
              </div>
              <div class="flex h-7 shrink-0 items-center gap-1.5 px-2 text-xs">
                <template v-if="tile.blocked">
                  <UIcon name="i-lucide-ban" class="size-3.5 shrink-0 text-muted" />
                  <span class="truncate text-muted">{{ tile.blocked }}</span>
                </template>
                <template v-else-if="tile.placedAt">
                  <UIcon name="i-lucide-book-open" class="size-3.5 shrink-0 text-muted" />
                  <span class="truncate text-toned">On page {{ tile.placedAt.page }}</span>
                </template>
                <span v-else class="truncate text-muted">{{ name(tile.photo.path) }}</span>
              </div>
              <span
                v-if="tile.index === chosen"
                class="absolute top-1.5 right-1.5 flex size-5 items-center justify-center rounded-full bg-inverted text-inverted"
              >
                <UIcon name="i-lucide-check" class="size-3.5" />
              </span>
            </button>
          </template>
        </ContactSheet>
      </div>
    </template>
    <template #footer>
      <p class="truncate text-xs text-muted">
        <template v-if="selection">{{ name(selection.photo.path) }}</template>
        <template v-else>Choose a photo. Double-click to use it at once.</template>
      </p>
      <div class="flex shrink-0 gap-2">
        <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
        <UButton color="primary" :disabled="!selection" @click="confirm">{{ confirmLabel }}</UButton>
      </div>
    </template>
  </UModal>
</template>
