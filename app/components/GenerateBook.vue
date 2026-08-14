<script setup lang="ts">
import {
  defaultProjectName,
  generatedLabel,
  lastExportLabel,
  optionFor,
  recommendedOption,
  revealTarget,
  summarizeExport,
} from "~/types/book";
import type { AnalyzedPhoto } from "~/types/features";

const { photos, folder } = defineProps<{
  /** The analysed photos, exactly as Rust sent them -- see `useBook`. */
  photos: AnalyzedPhoto[];
  folder: string;
}>();

// `toRef` rather than passing the props straight through: `useBook` holds
// these across async command calls, and a plain value captured at setup time
// would go stale the moment the user analyses a different folder.
const photosRef = toRef(() => photos);
const folderRef = toRef<string | null>(() => folder);

const {
  recommendation,
  generated,
  exportResult,
  progress,
  projects,
  outputDir,
  busy,
  error,
  refreshRecommendation,
  generate,
  pickOutputDir,
  exportBook,
  loadProjects,
  reveal,
} = useBook(photosRef, folderRef);

const name = ref(defaultProjectName(folder));
/**
 * `null` only before the first recommendation arrives -- the watcher below
 * seeds it with the recommended length, so the control is never rendered
 * empty and the user is always overriding a real default rather than
 * choosing from nothing.
 */
const chosenPages = ref<number | null>(null);

const pages = computed(() => chosenPages.value ?? recommendation.value?.recommendedPages ?? null);
const chosenOption = computed(() => {
  if (!recommendation.value) return undefined;
  return pages.value === null
    ? recommendedOption(recommendation.value)
    : optionFor(recommendation.value, pages.value);
});
/** True while the user is still on the recommended length. */
const isRecommended = computed(
  () => pages.value !== null && pages.value === recommendation.value?.recommendedPages,
);
const pageItems = computed(() =>
  (recommendation.value?.options ?? []).map((option) => ({
    label: `${option.pages} pages`,
    value: option.pages,
  })),
);
const counts = computed(() => (exportResult.value ? summarizeExport(exportResult.value) : null));
const revealPath = computed(() => (exportResult.value ? revealTarget(exportResult.value) : null));
const exportPercent = computed(() =>
  progress.value.total > 0
    ? Math.round((progress.value.completed / progress.value.total) * 100)
    : 0,
);

// The recommendation depends on the whole analysed set, so it is refreshed
// whenever that set changes -- including the first render, hence `immediate`.
// The chosen length resets with it: a length picked against one folder's
// keeper count is not a decision the user made about a different folder.
watch(
  () => photos,
  () => {
    name.value = defaultProjectName(folder);
    chosenPages.value = null;
    void refreshRecommendation();
  },
  { immediate: true },
);

watch(recommendation, (next) => {
  if (next && chosenPages.value === null) chosenPages.value = next.recommendedPages;
});

function onGenerate() {
  if (pages.value === null) return;
  void generate(name.value, pages.value);
}

onMounted(() => {
  void loadProjects();
});
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-end gap-4">
      <UFormField label="Book name" class="w-64">
        <UInput v-model="name" :disabled="busy" placeholder="Untitled photobook" />
      </UFormField>

      <UFormField label="Length" help="Both are real Pixajoy SKUs.">
        <USelect
          v-model="chosenPages"
          :items="pageItems"
          :disabled="busy || pageItems.length === 0"
          value-key="value"
          class="w-36"
        />
      </UFormField>

      <UButton
        icon="i-lucide-book-open"
        color="primary"
        :loading="busy"
        :disabled="busy || !recommendation"
        @click="onGenerate"
      >
        Generate book
      </UButton>
    </div>

    <!--
      The recommendation and its cost, stated before the user commits: a page
      length is a purchase decision, and "26 keepers, 24 fit" is the only
      thing that makes the two SKUs distinguishable.
    -->
    <p v-if="recommendation" class="text-sm text-muted">
      <span class="font-mono tabular-nums text-default">{{ recommendation.keeperCount }}</span>
      keepers ·
      <template v-if="chosenOption">
        <span class="font-mono tabular-nums text-default">{{ chosenOption.capacityPhotos }}</span>
        fit in
        <span class="font-mono tabular-nums text-default">{{ chosenOption.pages }}</span> pages
        <template v-if="chosenOption.droppedPhotos > 0">
          &middot;
          <span class="font-medium text-highlighted"
            ><span class="font-mono tabular-nums">{{ chosenOption.droppedPhotos }}</span> would be
            left out</span
          >
        </template>
        <template v-else> &middot; nothing left out</template>
      </template>
      <template v-if="isRecommended"> &middot; recommended length</template>
      <template v-else>
        &middot; recommended:
        <span class="font-mono tabular-nums text-default">{{
          recommendation.recommendedPages
        }}</span>
        pages
      </template>
    </p>

    <UAlert
      v-if="error"
      icon="i-lucide-triangle-alert"
      color="error"
      variant="subtle"
      title="Something went wrong"
      :description="error"
      :ui="{ description: 'break-words' }"
    />

    <!-- Generated and saved: from here the book survives a quit. -->
    <div v-if="generated" class="space-y-4 rounded-lg border border-default p-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="space-y-1">
          <h3 class="text-sm font-medium text-highlighted">{{ name }}</h3>
          <p class="text-sm text-muted">{{ generatedLabel(generated) }}</p>
        </div>
        <div class="flex items-center gap-2">
          <UButton
            icon="i-lucide-folder-output"
            color="neutral"
            variant="outline"
            size="sm"
            :disabled="busy"
            @click="pickOutputDir"
          >
            {{ outputDir ? "Change output folder" : "Choose output folder" }}
          </UButton>
          <UButton
            icon="i-lucide-download"
            color="primary"
            size="sm"
            :loading="busy"
            :disabled="busy || !outputDir"
            @click="exportBook"
          >
            Export
          </UButton>
        </div>
      </div>

      <p v-if="outputDir" class="truncate text-xs text-muted">Exporting to {{ outputDir }}</p>
      <p v-else class="text-xs text-muted">Choose where the print files should be written.</p>

      <div v-if="progress.running" class="max-w-sm space-y-2">
        <UProgress
          color="primary"
          size="sm"
          :model-value="progress.completed"
          :max="progress.total"
        />
        <p class="text-sm text-muted">
          <span class="font-mono tabular-nums text-default">{{ progress.completed }}</span> /
          <span class="font-mono tabular-nums text-default">{{ progress.total }}</span> photos
          exported ({{ exportPercent }}%)
        </p>
      </div>
    </div>

    <!--
      Pre-flight. Blocks and Warns are rendered as two separate lists from
      two separate arrays, never one filtered list: they are not the same
      kind of thing, and the difference between them is whether any file was
      written at all.
    -->
    <div v-if="exportResult && counts" class="space-y-4">
      <UAlert
        v-if="exportResult.blocked"
        icon="i-lucide-octagon-x"
        color="error"
        variant="subtle"
        title="Nothing was exported"
        description="Pre-flight found problems that would print badly. No files were written; fix these and export again."
      />
      <UAlert
        v-else
        icon="i-lucide-check"
        color="success"
        variant="subtle"
        :title="`${counts.writtenCount} files written`"
        :description="`Format: ${exportResult.format}. A manifest of what went where is in the same folder.`"
        :actions="[
          {
            label: 'Reveal in Finder',
            icon: 'i-lucide-folder-open',
            color: 'neutral',
            variant: 'outline',
            onClick: () => revealPath && reveal(revealPath),
          },
        ]"
      />

      <div v-if="counts.blockingCount > 0" class="space-y-2">
        <h4 class="flex items-center gap-2 text-sm font-medium text-highlighted">
          <UIcon name="i-lucide-octagon-x" class="size-4 text-error" />
          Blocking ({{ counts.blockingCount }})
        </h4>
        <ul class="space-y-1 text-sm text-muted">
          <li v-for="(f, i) in exportResult.blocking" :key="`block-${i}`" class="break-words">
            <span v-if="f.page > 0" class="font-mono tabular-nums text-default"
              >p{{ f.page }}</span
            >
            {{ f.message }}
          </li>
        </ul>
      </div>

      <div v-if="counts.warningCount > 0" class="space-y-2">
        <h4 class="flex items-center gap-2 text-sm font-medium text-highlighted">
          <UIcon name="i-lucide-triangle-alert" class="size-4 text-warning" />
          Warnings ({{ counts.warningCount }})
        </h4>
        <ul class="space-y-1 text-sm text-muted">
          <li v-for="(f, i) in exportResult.warnings" :key="`warn-${i}`" class="break-words">
            <span v-if="f.page > 0" class="font-mono tabular-nums text-default"
              >p{{ f.page }}</span
            >
            {{ f.message }}
          </li>
        </ul>
      </div>

      <div v-if="counts.failedCount > 0" class="space-y-2">
        <h4 class="text-sm font-medium text-highlighted">
          Failed to write ({{ counts.failedCount }})
        </h4>
        <ul class="space-y-1 text-sm text-muted">
          <li v-for="failure in exportResult.failures" :key="failure.filename" class="break-words">
            <span class="font-mono text-default">{{ failure.filename }}</span>
            {{ failure.message }}
          </li>
        </ul>
      </div>
    </div>

    <!-- Saved books, with what each one last produced. -->
    <div v-if="projects.length > 0" class="space-y-3 border-t border-default pt-6">
      <h3 class="text-sm font-medium text-highlighted">Saved books</h3>
      <ul class="space-y-2">
        <li
          v-for="project in projects"
          :key="project.id"
          class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 rounded-lg border border-default px-3 py-2"
        >
          <span class="text-sm text-default">{{ project.name }}</span>
          <span class="text-xs text-muted">
            <span class="font-mono tabular-nums">{{ project.pageCount }}</span> pages ·
            <span class="font-mono tabular-nums">{{ project.photoCount }}</span> photos ·
            {{ lastExportLabel(project) }}
          </span>
        </li>
      </ul>
    </div>
  </section>
</template>
