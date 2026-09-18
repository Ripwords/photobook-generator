<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  isRanked,
  toggledOverride,
  type AnalyzedPhoto,
  type PartialAnalyzedPhoto,
  type PhotoOverride,
} from "~/types/features";

const {
  photo,
  burstSize = 1,
  isHero = false,
  override = "auto",
  isKept = true,
} = defineProps<{
  photo: AnalyzedPhoto | PartialAnalyzedPhoto;
  /** How many near-duplicate frames this photo was picked from. 1 means no burst. */
  burstSize?: number;
  /** The single top-ranked photo in its event group. Outlined, and badged with a star. */
  isHero?: boolean;
  /**
   * The user's own decision about this photo. Display only -- this component
   * decides nothing about whether the photo survives; it emits the decision
   * and Rust sends back the verdict (see `usePhotoOverrides`).
   */
  override?: PhotoOverride;
  /**
   * Whether this photo survives culling -- RUST'S verdict, read off
   * `AnalyzedPhoto.kept` by the caller, never re-derived here. Drives nothing
   * but the tile's appearance.
   */
  isKept?: boolean;
}>();

const emit = defineEmits<{ setOverride: [state: PhotoOverride] }>();

/**
 * Pressing the state a photo is already in returns it to "auto", so both
 * buttons are toggles and there is always a way back to letting the engine
 * decide.
 */
function press(state: Exclude<PhotoOverride, "auto">) {
  emit("setOverride", toggledOverride(override, state));
}

/**
 * A photo the book will not contain is dimmed rather than hidden: it has to
 * stay on screen for the user to be able to ask for it back, which is the
 * whole point of the include control.
 */
const dimmed = computed(() => !isKept);

const src = computed(() => (photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : null));
// While a photo is still streaming in, it has no aesthetic/sharpness
// percentile yet -- those are whole-set derivations that only exist once
// every photo in the folder has been analyzed (see `AnalysisEvent` in
// commands.rs). Rendering a provisional value here that later jumps under
// the viewer is exactly what the design review ruled out; omitting the
// badges entirely until `ranked` is honest about the tile's real state.
const ranked = computed(() => isRanked(photo));
// Narrowed accessors rather than reading `photo.aestheticPct` directly in
// the template: `photo`'s type is a union, and this keeps the "only exists
// once ranked" contract enforced by the type checker instead of by
// convention.
const aestheticPct = computed(() => (isRanked(photo) ? photo.aestheticPct : null));
const sharpnessPct = computed(() => (isRanked(photo) ? photo.sharpnessPct : null));
</script>

<template>
  <figure
    class="group relative aspect-square overflow-hidden rounded-lg bg-elevated"
    :class="[
      isHero ? 'ring-2 ring-inverted ring-offset-2 ring-offset-(--ui-bg)' : 'ring ring-default',
      dimmed ? 'opacity-45 grayscale' : '',
    ]"
  >
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
      <UIcon name="i-lucide-image-off" class="size-6 text-muted" />
    </div>

    <UBadge
      v-if="burstSize > 1"
      color="neutral"
      size="sm"
      class="absolute top-2 left-2 gap-1 bg-black/60 text-white ring-0"
      :title="`Best of ${burstSize} near-duplicate frames`"
    >
      <UIcon name="i-lucide-images" class="size-3" />
      <span class="tabular-nums">{{ burstSize }}</span>
    </UBadge>

    <!--
      The user's own decision. Rendered on hover (and always, once a decision
      has been made) so the contact sheet stays photo-first, which is the
      whole point of it.

      These buttons carry no rule: they emit, `usePhotoOverrides` sends the
      map to Rust, and Rust sends back records with `kept` re-stamped by the
      single culling authority. Do not "helpfully" filter here -- that is the
      second implementation Phase 2 removed.
    -->
    <div
      class="absolute inset-x-0 top-0 flex justify-center gap-1 p-2 transition-opacity"
      :class="override === 'auto' ? 'opacity-0 group-hover:opacity-100 focus-within:opacity-100' : 'opacity-100'"
    >
      <UButton
        :icon="override === 'include' ? 'i-lucide-check-circle-2' : 'i-lucide-circle-plus'"
        size="sm"
        :color="override === 'include' ? 'primary' : 'neutral'"
        :class="override === 'include' ? '' : 'bg-black/60 text-white'"
        :aria-pressed="override === 'include'"
        :title="override === 'include' ? 'Included by you - click to let the engine decide' : 'Always include this photo'"
        aria-label="Always include this photo"
        @click="press('include')"
      />
      <UButton
        :icon="override === 'exclude' ? 'i-lucide-x-circle' : 'i-lucide-circle-minus'"
        size="sm"
        :color="override === 'exclude' ? 'error' : 'neutral'"
        :class="override === 'exclude' ? '' : 'bg-black/60 text-white'"
        :aria-pressed="override === 'exclude'"
        :title="override === 'exclude' ? 'Excluded by you - click to let the engine decide' : 'Never include this photo'"
        aria-label="Never include this photo"
        @click="press('exclude')"
      />
    </div>

    <UBadge
      v-if="isHero"
      size="sm"
      class="absolute top-2 right-2 gap-1 bg-white text-black ring-0"
      title="Highest-ranked photo in this event"
    >
      <UIcon name="i-lucide-star" class="size-3" />
    </UBadge>
    <UBadge
      v-else-if="dimmed"
      color="neutral"
      size="sm"
      class="absolute top-2 right-2 gap-1 bg-black/60 text-white ring-0"
      :title="
        override === 'exclude'
          ? 'You excluded this photo'
          : 'Not selected for the book - click + to include it'
      "
    >
      <UIcon
        :name="override === 'exclude' ? 'i-lucide-x' : 'i-lucide-minus'"
        class="size-3"
      />
    </UBadge>

    <figcaption
      class="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 bg-gradient-to-t from-black/85 via-black/55 to-transparent px-2 pt-6 pb-1.5 text-white"
    >
      <span
        v-if="ranked && aestheticPct !== null && sharpnessPct !== null"
        class="flex items-center gap-2 text-[11px] leading-none"
      >
        <span class="flex items-center gap-1" title="Aesthetic percentile within this folder">
          <UIcon name="i-lucide-sparkles" class="size-3" />
          <span class="tabular-nums">{{ Math.round(aestheticPct) }}</span>
        </span>
        <span class="flex items-center gap-1" title="Sharpness percentile within this folder">
          <UIcon name="i-lucide-focus" class="size-3" />
          <span class="tabular-nums">{{ Math.round(sharpnessPct) }}</span>
        </span>
      </span>
      <span
        v-else
        class="flex items-center gap-1 text-[11px] leading-none text-white"
        title="Rank is computed once every photo in the folder has been analyzed"
      >
        <UIcon name="i-lucide-clock" class="size-3" />
        <span>unranked</span>
      </span>

      <span v-if="photo.faceCount > 0" class="flex items-center gap-2 text-[11px] leading-none">
        <span class="flex items-center gap-1" title="Faces detected">
          <UIcon name="i-lucide-user-round" class="size-3" />
          <span class="tabular-nums">{{ photo.faceCount }}</span>
        </span>
        <!--
          Smile badge intentionally removed. `smileFraction` is still
          computed by the sidecar and carried on `photo` end to end -- only
          rendering it is suppressed here. Real calibration data (15 faces,
          all confirmed smiling by a human) shows the threshold has a 100%
          false-negative rate: the strongest measured lift was 0.072, but
          `confidence > 0.5` requires lift > 0.10, so no genuine smile can
          ever cross it. A confidently wrong percentage is worse than no
          signal. See docs/PROJECT-STATUS.md, "Deferred: smile detection
          calibration" under What is NOT built, for the full measurement,
          the disqualification of `midpointLift`, and what is needed to
          finish this (or drop `smile_fraction` entirely).
        -->
      </span>
    </figcaption>
  </figure>
</template>
