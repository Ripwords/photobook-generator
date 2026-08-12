<script setup lang="ts">
import { basename, burstSizes, groupByEvent, keepers } from "~/types/features";

const { summary, running, error, folder, pickFolderAndAnalyze, retry } = useAnalysis();

type ViewState = "entry" | "running" | "error" | "no-images" | "no-analyzed" | "results";

const state = computed<ViewState>(() => {
  if (running.value) return "running";
  if (error.value) return "error";
  if (!summary.value) return "entry";
  if (summary.value.total === 0) return "no-images";
  if (summary.value.photos.length === 0) return "no-analyzed";
  return "results";
});

const kept = computed(() => (summary.value ? keepers(summary.value.photos) : []));
const eventGroups = computed(() => groupByEvent(kept.value));
const burstMap = computed<Map<number, number>>(() =>
  summary.value ? burstSizes(summary.value.photos) : new Map(),
);
const folderLabel = computed(() => (folder.value ? basename(folder.value) : "the selected folder"));

const gridClass = "grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-3";
</script>

<template>
  <div class="flex h-screen flex-col bg-default text-default">
    <header class="flex shrink-0 items-center justify-between gap-4 border-b border-default px-6 py-4">
      <div class="flex items-center gap-2.5">
        <UIcon name="i-lucide-images" class="size-5 text-primary" />
        <h1 class="text-sm font-semibold text-highlighted">Photobook Generator</h1>
      </div>
      <div class="flex items-center gap-2">
        <UButton
          v-if="state !== 'entry'"
          icon="i-lucide-folder-open"
          color="neutral"
          variant="outline"
          size="sm"
          :loading="running"
          :disabled="running"
          @click="pickFolderAndAnalyze"
        >
          Choose a different folder
        </UButton>
        <UColorModeButton size="sm" />
      </div>
    </header>

    <main class="flex-1 overflow-y-auto p-6">
      <UEmpty
        v-if="state === 'entry'"
        icon="i-lucide-images"
        title="Choose a photo folder to begin"
        description="Photobook Generator analyzes every photo in a folder on this Mac: sharpness, faces, color palette, and Apple's aesthetic model. It groups burst shots and events, then ranks each photo against the rest of the folder so you can see what's worth printing."
        :actions="[
          {
            label: 'Choose photo folder',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            loading: running,
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <div v-else-if="state === 'running'" class="space-y-6">
        <div class="max-w-sm space-y-2">
          <UProgress color="primary" size="sm" />
          <p class="text-sm text-muted">
            Analyzing photos in <span class="text-default">{{ folderLabel }}</span
            >. This can take a few minutes for large folders.
          </p>
        </div>
        <div :class="gridClass" aria-hidden="true">
          <USkeleton v-for="n in 24" :key="n" class="aspect-square w-full" />
        </div>
      </div>

      <UEmpty
        v-else-if="state === 'error'"
        icon="i-lucide-triangle-alert"
        title="Analysis failed"
        :description="error ?? undefined"
        :ui="{ description: 'break-words' }"
        :actions="[
          { label: 'Try again', icon: 'i-lucide-rotate-ccw', color: 'primary', onClick: retry },
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'neutral',
            variant: 'outline',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="state === 'no-images'"
        icon="i-lucide-image-off"
        title="No photos found"
        description="This folder does not contain any supported image files. Choose a different folder to continue."
        :actions="[
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="state === 'no-analyzed' && summary"
        icon="i-lucide-triangle-alert"
        title="None of the photos in this folder could be analyzed"
        :description="`${summary.failed} of ${summary.total} files failed. They may be corrupted or in an unsupported format.`"
        :actions="[
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <div v-else-if="state === 'results' && summary" class="space-y-8">
        <div class="space-y-1">
          <div class="flex flex-wrap items-center gap-x-6 gap-y-1 text-sm text-muted">
            <span
              ><span class="font-mono tabular-nums text-default">{{ summary.total }}</span>
              scanned</span
            >
            <span
              ><span class="font-mono tabular-nums text-default">{{
                summary.photos.length
              }}</span>
              analyzed</span
            >
            <span
              ><span class="font-mono tabular-nums text-default">{{ summary.cached }}</span> from
              cache</span
            >
            <span
              ><span class="font-mono tabular-nums text-default">{{ summary.failed }}</span>
              failed</span
            >
            <span class="font-medium text-highlighted"
              ><span class="font-mono tabular-nums">{{ kept.length }}</span> keepers</span
            >
          </div>
          <p class="text-xs text-dimmed">
            Percentiles are ranked within this folder. Sparkle is aesthetic, focus is sharpness.
          </p>
        </div>

        <UEmpty
          v-if="eventGroups.length === 0"
          icon="i-lucide-image-off"
          title="No photos were kept"
          description="Every analyzed photo was flagged as a screenshot, document, or similar non-photo image."
          :actions="[
            {
              label: 'Choose a different folder',
              icon: 'i-lucide-folder-open',
              color: 'primary',
              onClick: pickFolderAndAnalyze,
            },
          ]"
          class="mx-auto max-w-lg"
        />

        <section
          v-for="(group, index) in eventGroups"
          :key="group.eventCluster"
          class="space-y-3"
        >
          <h2 class="text-sm font-medium text-toned">
            Event {{ index + 1 }}
            <span class="text-dimmed"
              >&middot; {{ group.photos.length }}
              {{ group.photos.length === 1 ? "photo" : "photos" }}</span
            >
          </h2>
          <div :class="gridClass">
            <PhotoTile
              v-for="photo in group.photos"
              :key="photo.path"
              :photo="photo"
              :burst-size="burstMap.get(photo.nearDupCluster) ?? 1"
            />
          </div>
        </section>
      </div>
    </main>
  </div>
</template>
