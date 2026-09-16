<script setup lang="ts">
import {
  defaultProjectName,
  exportOutcome,
  generatedLabel,
  lastExportLabel,
  canGenerateAt,
  includeOverflowLabel,
  optionFor,
  projectDetailLabel,
  selectionLabel,
  recommendedOption,
  revealTarget,
  summarizeExport,
} from "~/types/book";
import type { AnalyzedPhoto, PhotoOverrides } from "~/types/features";

const {
  photos = [],
  overrides = {},
  photoSetId = 0,
  folder = null,
  openProjectId = null,
} = defineProps<{
  /** The analysed photos, exactly as Rust sent them -- see `useBook`. Omitted when this is mounted to view a project opened from disk rather than a freshly analysed folder. */
  photos?: AnalyzedPhoto[];
  /** The user's own include/exclude decisions, persisted with the project by `generate_book`. */
  overrides?: PhotoOverrides;
  /**
   * Identity of the analysed SET, bumped once per analysis and never per
   * override -- see `usePhotoOverrides.photoSetId`. Watched instead of
   * `photos`, whose array identity changes on every toggle.
   */
  photoSetId?: number;
  folder?: string | null;
  /** A saved project to load on mount, without analysing anything -- see `useBook`'s `openProject`. */
  openProjectId?: number | null;
}>();

const emit = defineEmits<{
  /** The user wants to edit a reopened project's photo selection: re-analyse its folder, then restore these decisions. */
  editSelection: [payload: { sourceFolder: string; overrides: PhotoOverrides }];
}>();

// `toRef` rather than passing the props straight through: `useBook` holds
// these across async command calls, and a plain value captured at setup time
// would go stale the moment the user analyses a different folder.
const photosRef = toRef(() => photos);
const folderRef = toRef<string | null>(() => folder);
const overridesRef = toRef(() => overrides);

const {
  recommendation,
  generated,
  activeProject,
  exportResult,
  layout,
  progress,
  projects,
  outputDir,
  busy,
  error,
  refreshRecommendation,
  generate,
  openProject,
  deleteProject,
  renameProject,
  pickOutputDir,
  exportBook,
  editBook,
  loadProjects,
  reset,
  reveal,
} = useBook(photosRef, folderRef, overridesRef);

const name = ref(defaultProjectName(folder ?? ""));
/**
 * `null` only before the first recommendation arrives -- the watcher below
 * seeds it with the recommended length, so the control is never rendered
 * empty and the user is always overriding a real default rather than
 * choosing from nothing.
 */
const chosenPages = ref<number | null>(null);

/**
 * The "pick a length and generate" flow needs an analysed photo set to
 * recommend against -- it is hidden entirely (rather than rendered disabled)
 * when this component is showing a project opened straight from disk, since
 * there is nothing to regenerate it from.
 */
const canGenerate = computed(() => photos.length > 0);

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
    // Not merely expensive -- unbuildable. The engine refuses to choose which
    // of the user's own picks to discard, so generating at this length fails
    // rather than producing a shorter book.
    disabled: !canGenerateAt(option),
  })),
);
/** Why the chosen length cannot be generated, or `null` when it can. */
const overflowMessage = computed(() =>
  chosenOption.value ? includeOverflowLabel(chosenOption.value) : null,
);
const counts = computed(() => (exportResult.value ? summarizeExport(exportResult.value) : null));
const outcome = computed(() => (exportResult.value ? exportOutcome(exportResult.value) : null));
const revealPath = computed(() => (exportResult.value ? revealTarget(exportResult.value) : null));
const exportPercent = computed(() =>
  progress.value.total > 0
    ? Math.round((progress.value.completed / progress.value.total) * 100)
    : 0,
);
/** The "Generated and saved" panel's heading: the loaded project's own name, or the name the user is generating one under. */
const panelTitle = computed(() => activeProject.value?.name ?? name.value);
/** The panel's subheading: the loaded project's counts and export history, or what THIS generation just produced. */
/**
 * The decisions a reopened project was generated with, or `null`. Shown so
 * they are visible at all -- returning them over the wire and rendering
 * nothing is the same "persisted but unreachable" defect this feature exists
 * to fix.
 */
const restoredSelection = computed(() =>
  activeProject.value ? selectionLabel(activeProject.value) : null,
);

const panelSubtitle = computed(() => {
  if (activeProject.value) return projectDetailLabel(activeProject.value);
  return generated.value ? generatedLabel(generated.value) : "";
});

// A DIFFERENT ANALYSED SET -- not a different array.
//
// This used to watch `photos`, which `usePhotoOverrides` replaces on every
// toggle: clicking include on one photo therefore threw away the generated
// book, the opened project, the chosen output folder, the export report, a
// name the user had typed, and a page length they had chosen, silently
// reverting a 40-page book to the recommended 20. `photoSetId` changes once
// per analysis, which is the actual question being asked here.
//
// `immediate` so the first render is covered.
watch(
  () => photoSetId,
  () => {
    // Everything below belongs to the PREVIOUS folder (or opened project): a
    // generated book, an opened project, the output directory chosen for it,
    // and its export report. Carrying any of them across would leave an
    // "Export" button wired to a book that is no longer on screen.
    reset();
    name.value = defaultProjectName(folder ?? "");
    chosenPages.value = null;
    if (canGenerate.value) void refreshRecommendation();
  },
  { immediate: true },
);

// The overrides change what the book contains, so they change both the keeper
// count and whether a length can hold every photo the user asked for. This is
// the ONLY refresh a toggle triggers -- the watcher above used to fire on the
// same click, sending the whole analysed array twice per click.
//
// Nothing else is touched: no reset, no name, no page length. The user is
// refining a selection, not starting over.
watch(
  () => overrides,
  () => {
    if (canGenerate.value) void refreshRecommendation();
  },
);

watch(recommendation, (next) => {
  if (next && chosenPages.value === null) chosenPages.value = next.recommendedPages;
});

function onGenerate() {
  if (pages.value === null || overflowMessage.value !== null) return;
  void generate(name.value, pages.value);
}

onMounted(() => {
  void loadProjects();
  // Runs AFTER the `photos` watcher's `reset()` above (which fires
  // synchronously during setup, before `onMounted`), so this is never
  // clobbered by it.
  if (openProjectId !== null) void openProject(openProjectId);
});
</script>

<template>
  <section class="space-y-6">
    <div v-if="canGenerate" class="flex flex-wrap items-end gap-4">
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
        :disabled="busy || !recommendation || overflowMessage !== null"
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
    <p v-if="canGenerate && recommendation" class="text-sm text-muted">
      <span class="font-mono tabular-nums text-default">{{ recommendation.keeperCount }}</span>
      keepers<template v-if="recommendation.includedCount > 0">
        (<span class="font-mono tabular-nums text-default">{{ recommendation.includedCount }}</span>
        you picked)</template
      >
      ·
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

    <!--
      A length that cannot hold every photo the user explicitly asked for is
      a refusal, not a cost -- so it is stated separately from
      "N would be left out" and it disables the button. The engine will not
      choose which of their own picks to discard.
    -->
    <UAlert
      v-if="overflowMessage"
      icon="i-lucide-triangle-alert"
      color="warning"
      variant="subtle"
      title="This length cannot hold everything you picked"
      :description="`${overflowMessage}. Exclude some photos, or choose a longer book.`"
    />

    <UAlert
      v-if="error"
      icon="i-lucide-triangle-alert"
      color="error"
      variant="subtle"
      title="Something went wrong"
      :description="error"
      :ui="{ description: 'break-words' }"
    />

    <!--
      Generated and saved, OR opened from disk -- either way the book
      already survived a quit, and export works identically for both. See
      `panelTitle`/`panelSubtitle` for which one is showing.
    -->
    <div v-if="generated || activeProject" class="space-y-4 rounded-lg border border-default p-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="space-y-1">
          <h3 class="text-sm font-medium text-highlighted">{{ panelTitle }}</h3>
          <p class="text-sm text-muted">{{ panelSubtitle }}</p>
          <p v-if="restoredSelection" class="flex items-center gap-1.5 text-xs text-muted">
            <UIcon name="i-lucide-hand" class="size-3 shrink-0" />
            <span>{{ restoredSelection }}</span>
          </p>
        </div>
        <div class="flex items-center gap-2">
          <!--
            Reopening a project restores its decisions, but they cannot be
            EDITED without the photos they refer to -- and a saved project is
            deliberately reopened without re-running Vision. This re-analyses
            the project's own folder (every photo is a features-cache hit, so
            no Vision work) and hands the decisions back to the contact sheet.
          -->
          <UButton
            v-if="activeProject && restoredSelection"
            icon="i-lucide-square-pen"
            color="neutral"
            variant="outline"
            size="sm"
            :disabled="busy"
            @click="
              emit('editSelection', {
                sourceFolder: activeProject.sourceFolder,
                overrides: activeProject.overrides,
              })
            "
          >
            Edit the selection
          </UButton>
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
      The book itself, spread by spread, as it will print, with the
      spread-level controls: regenerate, reject, change layout, lock, shuffle,
      and swapping two photos. Every control sends one `edit_book` and shows
      what came back -- `useBook.editBook`. Rendered for a book generated in
      this session AND for one reopened from disk, since both are saved
      projects by the time `book_layout` reads them.
    -->
    <BookPreview v-if="layout" :layout :busy @edit="editBook" />

    <!--
      Pre-flight. Blocks and Warns are rendered as two separate lists from
      two separate arrays, never one filtered list: they are not the same
      kind of thing, and the difference between them is whether any file was
      written at all.
    -->
    <div v-if="exportResult && counts" class="space-y-4">
      <UAlert
        v-if="outcome === 'blocked'"
        icon="i-lucide-octagon-x"
        color="error"
        variant="subtle"
        title="Nothing was exported"
        description="Pre-flight found problems that would print badly. No files were written; fix these and export again."
      />
      <!--
        `blocked: false` with nothing written is an export where every single
        item failed. It is not a success and must not be dressed as one.
      -->
      <UAlert
        v-else-if="outcome === 'failed'"
        icon="i-lucide-triangle-alert"
        color="error"
        variant="subtle"
        title="No files could be written"
        description="Pre-flight passed, but every photo failed to export. The reasons are listed below."
      />
      <UAlert
        v-else
        :icon="outcome === 'partial' ? 'i-lucide-triangle-alert' : 'i-lucide-check'"
        :color="outcome === 'partial' ? 'warning' : 'success'"
        variant="subtle"
        :title="`${counts.writtenCount} ${counts.writtenCount === 1 ? 'file' : 'files'} written to ${exportResult.outputDir}`"
        :description="
          exportResult.manifestError
            ? `Format: ${exportResult.format}. The files are there, but manifest.json could not be written: ${exportResult.manifestError}`
            : `Format: ${exportResult.format}. A manifest of what went where is in the same folder.`
        "
        :ui="{ description: 'break-words' }"
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
        <!--
          `@open` reopens a SAVED project without re-analysing anything: this
          is what makes it possible to export a book from a previous session
          without paying for another Vision pass over the same folder.
          `@delete` routes through `useBook.deleteProject`, which clears
          `generated`/`activeProject` through `withProjectDeleted` when the
          project deleted is the one THIS panel is currently showing -- so
          deleting it here can never leave the panel displayed or leave
          Export still wired to it.
        -->
        <ProjectListRow
          v-for="project in projects"
          :key="project.id"
          :project
          :busy
          @open="openProject"
          @rename="renameProject"
          @delete="deleteProject"
        >
          <template #meta>
            <span class="font-mono tabular-nums">{{ project.pageCount }}</span> pages ·
            <span class="font-mono tabular-nums">{{ project.photoCount }}</span> photos ·
            {{ lastExportLabel(project) }}
          </template>
        </ProjectListRow>
      </ul>
    </div>
  </section>
</template>
