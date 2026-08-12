<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import type { AnalyzedPhoto } from "~/types/features";

const { photo, burstSize = 1 } = defineProps<{
  photo: AnalyzedPhoto;
  /** How many near-duplicate frames this photo was picked from. 1 means no burst. */
  burstSize?: number;
}>();

const src = computed(() => (photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : null));
const smilePct = computed(() =>
  photo.smileFraction === null ? null : Math.round(photo.smileFraction * 100),
);
</script>

<template>
  <figure class="relative aspect-square overflow-hidden rounded-lg bg-elevated ring ring-default">
    <img
      v-if="src"
      :src="src"
      alt=""
      loading="lazy"
      width="400"
      height="400"
      class="size-full object-cover"
    />
    <div v-else class="flex size-full items-center justify-center">
      <UIcon name="i-lucide-image-off" class="size-6 text-dimmed" />
    </div>

    <UBadge
      v-if="burstSize > 1"
      color="neutral"
      size="sm"
      class="absolute top-2 left-2 gap-1 bg-black/60 text-white ring-0"
      :title="`Best of ${burstSize} near-duplicate frames`"
    >
      <UIcon name="i-lucide-images" class="size-3" />
      <span class="font-mono tabular-nums">{{ burstSize }}</span>
    </UBadge>

    <figcaption
      class="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 bg-gradient-to-t from-black/85 via-black/55 to-transparent px-2 pt-6 pb-1.5 text-white"
    >
      <span class="flex items-center gap-2 text-[11px] leading-none">
        <span class="flex items-center gap-1" title="Aesthetic percentile within this folder">
          <UIcon name="i-lucide-sparkles" class="size-3" />
          <span class="font-mono tabular-nums">{{ Math.round(photo.aestheticPct) }}</span>
        </span>
        <span class="flex items-center gap-1" title="Sharpness percentile within this folder">
          <UIcon name="i-lucide-focus" class="size-3" />
          <span class="font-mono tabular-nums">{{ Math.round(photo.sharpnessPct) }}</span>
        </span>
      </span>

      <span v-if="photo.faceCount > 0" class="flex items-center gap-2 text-[11px] leading-none">
        <span class="flex items-center gap-1" title="Faces detected">
          <UIcon name="i-lucide-user-round" class="size-3" />
          <span class="font-mono tabular-nums">{{ photo.faceCount }}</span>
        </span>
        <span
          v-if="smilePct !== null"
          class="flex items-center gap-1"
          title="Share of faces smiling"
        >
          <UIcon name="i-lucide-smile" class="size-3" />
          <span class="font-mono tabular-nums">{{ smilePct }}%</span>
        </span>
      </span>
    </figcaption>
  </figure>
</template>
