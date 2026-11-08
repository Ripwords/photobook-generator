<script setup lang="ts">
import { cropStyle, rectStyle, type BookLayout, type CoverSide } from "~/types/preview";

const { layout, busy = false } = defineProps<{
  layout: BookLayout;
  /** True while an edit is in flight; every control waits for it. */
  busy?: boolean;
}>();

const graphics = useGraphics();

const emit = defineEmits<{
  /** Open the photo picker for one side. */
  choose: [side: CoverSide];
  /** Open the crop dialog for one side. */
  cropOpen: [side: CoverSide];
  /** A new plain spine colour, `#rrggbb`. */
  spine: [rgb: string];
}>();

/**
 * A click opens the picker for that side; framing is the crop button's. The
 * cover is also a drop target for the book's photo drag: a photo from any page
 * or from the left-out tray, let go over a panel, goes on that side. The cover
 * keeps its own copy, so nothing leaves the page it came from.
 */
const photoDrag = usePhotoDrag();
function isDropTarget(side: CoverSide): boolean {
  const hover = photoDrag?.hover.value;
  return hover?.kind === "cover" && hover.side === side && photoDrag?.outcome.value.kind === "edit";
}

/** Back on the left, front on the right, as the flat cover lies face down. */
const panels = computed(() =>
  (["back", "front"] as const).map((side) => {
    const cover = layout.cover[side];
    const photo = cover.photo;
    const shot = photo ? layout.photos[photo.photoIndex] : undefined;
    return {
      side,
      label: side === "front" ? "Front" : "Back",
      board: rectStyle(cover.board),
      visible: rectStyle(cover.visible),
      photo: photo && {
        crop: cropStyle(photo.crop),
        src: shot ? graphics.src(shot) : null,
        title: [shot?.path, photo.filename].filter(Boolean).join("\n"),
      },
    };
  }),
);

function onSpine(event: Event) {
  const rgb = (event.target as HTMLInputElement).value.toLowerCase();
  if (rgb !== layout.cover.spine) emit("spine", rgb);
}
</script>

<template>
  <div class="space-y-2.5">
    <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
      <p class="text-sm font-medium text-toned">Cover</p>
      <!--
        A native colour well: the layout carries no palette to offer swatches
        from, and the hex beside it says exactly what the manifest records.
      -->
      <label
        class="flex items-center gap-2 text-sm text-muted"
        :class="busy ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'"
      >
        Spine colour
        <span class="relative size-5 overflow-hidden rounded-sm ring-1 ring-default">
          <input
            type="color"
            class="absolute -inset-2 size-9 cursor-[inherit] border-0 p-0"
            :value="layout.cover.spine"
            :disabled="busy"
            aria-label="Spine colour"
            @change="onSpine"
          />
        </span>
        <span class="font-mono text-sm text-toned tabular-nums">{{ layout.cover.spine }}</span>
      </label>
    </div>

    <!--
      Back, spine, front, each panel at the real cover-panel aspect: the board
      plus the wrap that folds under it. The spine has no published width, so
      it is drawn at a nominal one in the chosen colour.
    -->
    <div
      class="flex items-stretch gap-px bg-neutral-300 shadow-[0_1px_2px_rgb(0_0_0/0.08),0_8px_24px_-6px_rgb(0_0_0/0.18)] dark:bg-neutral-600 dark:shadow-[0_1px_2px_rgb(0_0_0/0.4),0_12px_32px_-8px_rgb(0_0_0/0.7)]"
    >
      <template v-for="(panel, index) in panels" :key="panel.side">
        <div
          v-if="index === 1"
          class="w-5 shrink-0"
          :style="{ backgroundColor: layout.cover.spine }"
          :title="`Spine ${layout.cover.spine}`"
          aria-hidden="true"
        />
        <div
          class="relative min-w-0 flex-1 overflow-hidden bg-white"
          :style="{ aspectRatio: layout.cover.aspect }"
          :data-cover-side="panel.side"
        >
          <button
            v-if="panel.photo"
            type="button"
            class="absolute inset-0 overflow-hidden bg-neutral-100 text-left focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary"
            :class="busy ? 'cursor-wait' : 'cursor-pointer'"
            :disabled="busy"
            :title="panel.photo.title"
            :aria-label="`${panel.label} cover photo: click to change or remove it`"
            @click="emit('choose', panel.side)"
          >
            <img
              v-if="panel.photo.src"
              :src="panel.photo.src"
              :alt="`${panel.label} cover photo`"
              draggable="false"
              class="absolute top-0 left-0"
              :style="panel.photo.crop"
            />
            <span v-else class="flex size-full items-center justify-center">
              <UIcon name="i-lucide-image-off" class="size-4 text-neutral-500" />
            </span>
          </button>
          <button
            v-else
            type="button"
            class="absolute flex flex-col items-center justify-center gap-2 text-sm text-neutral-600 transition-colors hover:bg-neutral-100 hover:text-neutral-900 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary disabled:cursor-wait"
            :style="panel.board"
            :disabled="busy"
            @click="emit('choose', panel.side)"
          >
            <UIcon name="i-lucide-image-plus" class="size-5" />
            Choose a {{ panel.side }} cover photo
          </button>

          <!--
            The wrap, shaded: everything outside the board folds under it and
            never shows. The shadow is cast outward from the board rect, so the
            shading ends exactly on the trim line drawn over it.
          -->
          <div class="pointer-events-none absolute inset-0">
            <div
              class="absolute border border-dashed border-red-500/70 shadow-[0_0_0_9999px_rgb(0_0_0/0.35)]"
              :style="panel.board"
            />
            <div class="absolute border border-dashed border-sky-500/70" :style="panel.visible" />
          </div>
          <!-- A photo dragged over this side: letting go puts it here. -->
          <div v-if="isDropTarget(panel.side)" class="pointer-events-none absolute inset-0 ring-3 ring-primary ring-inset" />
          <!-- Zoom and framing, in the crop dialog. -->
          <div
            v-if="panel.photo"
            class="pointer-events-none absolute flex items-start justify-end p-2"
            :style="panel.visible"
          >
            <UTooltip :text="`Crop the ${panel.side} cover photo`">
              <UButton
                icon="i-lucide-crop"
                color="neutral"
                variant="solid"
                class="pointer-events-auto"
                :disabled="busy"
                :aria-label="`Crop the ${panel.side} cover photo`"
                @click="emit('cropOpen', panel.side)"
              />
            </UTooltip>
          </div>
        </div>
      </template>
    </div>
    <div class="flex gap-px text-sm text-dimmed" aria-hidden="true">
      <p class="flex-1">Back</p>
      <p class="w-5 shrink-0" />
      <p class="flex-1">Front</p>
    </div>
  </div>
</template>
