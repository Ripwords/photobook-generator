<script setup lang="ts">
import {
  exportOutcome,
  folderListLabel,
  projectDetailLabel,
  revealTarget,
  selectionLabel,
  summarizeExport,
} from "~/types/book";
import type { PhotoOverrides } from "~/types/features";

const { projectId } = defineProps<{
  projectId: number;
}>();

const emit = defineEmits<{
  close: [];
  /**
   * The user wants this project's photo selection back on the contact sheet.
   * Re-analyse its folders, then restore these decisions.
   */
  editPhotos: [
    payload: {
      projectId: number;
      name: string;
      sourceFolders: string[];
      overrides: PhotoOverrides;
    },
  ];
}>();

const {
  activeProject,
  exportResult,
  layout,
  progress,
  outputDir,
  busy,
  error,
  openProject,
  renameProject,
  pickOutputDir,
  exportBook,
  editBook,
  reveal,
} = useBook();

const counts = computed(() => (exportResult.value ? summarizeExport(exportResult.value) : null));
const outcome = computed(() => (exportResult.value ? exportOutcome(exportResult.value) : null));
const revealPath = computed(() => (exportResult.value ? revealTarget(exportResult.value) : null));
const exportPercent = computed(() =>
  progress.value.total > 0
    ? Math.round((progress.value.completed / progress.value.total) * 100)
    : 0,
);

/**
 * True only while a change to the BOOK is being written.
 *
 * `useBook.busy` covers every command this screen sends, so reading it raw
 * would announce "Saving…" while the project is still being opened and again
 * for the whole length of an export, beside the export's own progress bar.
 * Neither is a save, and a status line that cries save at the wrong moments
 * is worth less than no status line.
 */
const exporting = ref(false);
const saving = computed(() => busy.value && !exporting.value);

async function runExport() {
  exporting.value = true;
  try {
    await exportBook();
  } finally {
    exporting.value = false;
  }
}

const folderLabel = computed(() =>
  activeProject.value ? folderListLabel(activeProject.value.sourceFolders) : null,
);
const folderTitle = computed(() => activeProject.value?.sourceFolders.join("\n") ?? null);

/**
 * The decisions this project was generated with, or `null`. Shown so they are
 * visible at all -- returning them over the wire and rendering nothing is the
 * same "persisted but unreachable" defect this screen exists to fix.
 */
const restoredSelection = computed(() =>
  activeProject.value ? selectionLabel(activeProject.value) : null,
);

const renaming = ref(false);
const draftName = ref("");

function startRenaming() {
  draftName.value = activeProject.value?.name ?? "";
  renaming.value = true;
}

function cancelRenaming() {
  renaming.value = false;
  // Reset the draft, not just the flag. Hiding the field blurs it, and that
  // blur commits -- leaving the abandoned text here would save the very edit
  // Escape was pressed to throw away.
  draftName.value = activeProject.value?.name ?? "";
}

function commitRenaming() {
  const project = activeProject.value;
  renaming.value = false;
  if (!project) return;
  const trimmed = draftName.value.trim();
  // No-op renames (empty, or unchanged) are silently dropped rather than
  // round-tripped through Rust -- an empty name would be rejected there
  // anyway, and re-sending the same name just to bump `updated_at` would
  // reorder the library for nothing the user asked for.
  if (!trimmed || trimmed === project.name) return;
  void renameProject(project.id, trimmed);
}

function requestEditPhotos() {
  const project = activeProject.value;
  if (!project) return;
  emit("editPhotos", {
    projectId: project.id,
    name: project.name,
    sourceFolders: project.sourceFolders,
    overrides: project.overrides,
  });
}

onMounted(() => {
  void openProject(projectId);
});
</script>

<template>
  <AppHeader
    :title="activeProject?.name ?? 'Loading…'"
    :subtitle="folderLabel"
    :subtitle-title="folderTitle"
    back="All photobooks"
    @back="emit('close')"
  >
    <!--
      The book's name, set in the serif the library uses for titles, and
      renamed in place. One title, not a header title plus a second heading
      repeating it above the book.
    -->
    <template #title>
      <UInput
        v-if="activeProject && renaming"
        v-model="draftName"
        autofocus
        size="sm"
        :disabled="busy"
        aria-label="Photobook name"
        class="w-72"
        @keyup.enter="commitRenaming"
        @keyup.escape="cancelRenaming"
        @blur="commitRenaming"
      />
      <div v-else class="flex min-w-0 items-center gap-1">
        <h1 class="truncate font-serif text-lg leading-tight text-highlighted">
          {{ activeProject?.name ?? "Loading…" }}
        </h1>
        <UButton
          v-if="activeProject"
          icon="i-lucide-pencil"
          color="neutral"
          variant="ghost"
          size="xs"
          :disabled="busy"
          aria-label="Rename this photobook"
          @click="startRenaming"
        />
      </div>
    </template>

    <UTooltip
      v-if="activeProject"
      text="Every change you make here is written to disk as you make it."
    >
      <span class="flex items-center gap-1.5 text-xs text-muted">
        <UIcon
          :name="saving ? 'i-lucide-loader-circle' : 'i-lucide-check'"
          class="size-3.5 shrink-0"
          :class="saving && 'animate-spin'"
        />
        {{ saving ? "Saving…" : "All changes saved" }}
      </span>
    </UTooltip>

    <!--
      Always offered, not gated on the project carrying overrides: re-opening
      a saved book is deliberately done without re-running Vision, so this is
      the only path back to its photos, and a book generated with no overrides
      needs it just as much as one that has them. It re-analyses the project's
      own folders, which is every photo a features-cache hit and no Vision work.
    -->
    <UButton
      icon="i-lucide-square-pen"
      color="neutral"
      variant="outline"
      size="sm"
      :disabled="busy || !activeProject"
      @click="requestEditPhotos"
    >
      Edit photos
    </UButton>
  </AppHeader>

  <!--
    The desk the book lies on: a toned surface, so the white pages read as
    paper and the controls around them recede.
  -->
  <main class="flex-1 overflow-y-auto bg-charcoal-100 p-6 dark:bg-charcoal-950">
    <div class="mx-auto max-w-[1400px] space-y-8">
      <USkeleton v-if="!activeProject && !error" class="h-32 w-full" />

      <UAlert
        v-if="error"
        icon="i-lucide-triangle-alert"
        color="error"
        variant="subtle"
        title="Something went wrong"
        :description="error"
        :ui="{ description: 'break-words' }"
      />

      <div
        v-if="activeProject"
        class="space-y-3 rounded-lg bg-default p-4 ring ring-default"
      >
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="space-y-1">
            <p class="text-sm text-toned tabular-nums">{{ projectDetailLabel(activeProject) }}</p>
            <p v-if="restoredSelection" class="flex items-center gap-1.5 text-xs text-muted">
              <UIcon name="i-lucide-hand" class="size-3 shrink-0" />
              <span>{{ restoredSelection }}</span>
            </p>
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
              @click="runExport"
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
          <p class="text-sm text-muted tabular-nums">
            <span class="text-default">{{ progress.completed }}</span> of
            <span class="text-default">{{ progress.total }}</span> photos exported
            ({{ exportPercent }}%)
          </p>
        </div>
      </div>

      <!--
        The book itself, spread by spread, as it will print, with the
        spread-level controls: regenerate, reject, change layout, lock, shuffle,
        and swapping two photos. Every control sends one `edit_book` and shows
        what came back -- `useBook.editBook`.
      -->
      <BookPreview v-if="layout" :layout :busy @edit="editBook" />

      <!--
        Pre-flight. Blocks and Warns are rendered as two separate lists from
        two separate arrays, never one filtered list: they are not the same
        kind of thing, and the difference between them is whether any file was
        written at all.
      -->
      <div
        v-if="exportResult && counts"
        class="space-y-4 rounded-lg bg-default p-4 ring ring-default"
      >
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
              <span v-if="f.page > 0" class="tabular-nums text-default">Page {{ f.page }}:</span>
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
              <span v-if="f.page > 0" class="tabular-nums text-default">Page {{ f.page }}:</span>
              {{ f.message }}
            </li>
          </ul>
        </div>

        <div v-if="counts.failedCount > 0" class="space-y-2">
          <h4 class="text-sm font-medium text-highlighted">
            Failed to write ({{ counts.failedCount }})
          </h4>
          <ul class="space-y-1 text-sm text-muted">
            <li
              v-for="failure in exportResult.failures"
              :key="failure.filename"
              class="break-words"
            >
              <span class="font-mono text-default">{{ failure.filename }}</span>
              {{ failure.message }}
            </li>
          </ul>
        </div>
      </div>
    </div>
  </main>
</template>
