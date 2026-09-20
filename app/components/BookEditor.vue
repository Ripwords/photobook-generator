<script setup lang="ts">
import {
  exportOutcome,
  folderListLabel,
  folderNotice,
  projectDetailLabel,
  revealTarget,
  selectionLabel,
  summarizeExport,
} from "~/types/book";
import type { PhotoOverrides } from "~/types/features";
import type { BookOptions } from "~/types/book";
import { sizeLabel, type PrintSpec } from "~/types/printSpec";
import type { PreviewGeometry } from "~/types/preview";
import { shortcutCombo, shortcutKbds } from "~/types/shortcuts";

const { projectId, listedName } = defineProps<{
  projectId: number;
  /** The name the book list has, which a rename from the sidebar changes. */
  listedName?: string;
}>();

const emit = defineEmits<{
  /** The name changed, so any list showing it is stale. */
  renamed: [];
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
      /** Its print size, so generating it again keeps the book it was. */
      spec: PrintSpec | null;
      /** Its options, for the same reason. */
      options: BookOptions | null;
    },
  ];
}>();

const {
  activeProject,
  exportResult,
  folderCheck,
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
  refreshLayout,
  reveal,
} = useBook();

/** The chat panel beside the book. Open by default; the toolbar button folds it away. */
const chatOpen = ref(true);
/** The export sheet: output folder, the export itself, and pre-flight's report. */
const exportOpen = ref(false);
/** The Print size panel. Beside the book, not over it, so the guides it proposes show live. */
const printSizeOpen = ref(false);
/** The guides the panel's typed numbers would draw, while it is open. */
const proposedGeometry = ref<PreviewGeometry | null>(null);
const unit = usePrintUnit();

async function changePrintSize(spec: PrintSpec) {
  await editBook({ kind: "setPrintSpec", spec });
}

/** Whether a drag moves photo boxes rather than crops -- see `BookPreview`. */
const editSlots = ref(false);

const { openSettings } = useShell();

defineShortcuts({
  [shortcutCombo("chat")]: { usingInput: true, handler: () => (chatOpen.value = !chatOpen.value) },
  [shortcutCombo("export")]: { usingInput: true, handler: () => (exportOpen.value = true) },
});

/** How many printed pages come out white -- see `BookPreviewPage`. */
const blankPages = computed(
  () => layout.value?.pages.filter((page) => page.blank).length ?? 0,
);

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
  void renameProject(project.id, trimmed).then(() => emit("renamed"));
}

watch(
  () => listedName,
  (name) => {
    const project = activeProject.value;
    if (name && project && project.name !== name && !renaming.value) {
      activeProject.value = { ...project, name };
    }
  },
);

/**
 * The folder notice, once. Dismissing it is per visit to this book, not
 * saved: the photos really are missing from the book until Edit photos is
 * run, so the notice earns its place again next time the book is opened.
 */
const folderNoticeDismissed = ref(false);
const notice = computed(() =>
  folderNoticeDismissed.value ? null : folderNotice(folderCheck.value),
);

function requestEditPhotos() {
  const project = activeProject.value;
  if (!project) return;
  emit("editPhotos", {
    projectId: project.id,
    name: project.name,
    sourceFolders: project.sourceFolders,
    overrides: project.overrides,
    spec: layout.value?.spec ?? null,
    options: layout.value?.options ?? null,
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
  >
    <!-- The book's name, renamed in place. -->
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
      <div v-else class="flex min-w-0 items-center gap-0.5">
        <h1 class="truncate text-sm font-semibold text-highlighted">
          {{ activeProject?.name ?? "Loading…" }}
        </h1>
        <UTooltip v-if="activeProject" text="Rename">
          <UButton
            icon="i-lucide-pencil"
            color="neutral"
            variant="ghost"
            size="xs"
            :disabled="busy"
            aria-label="Rename this photobook"
            @click="startRenaming"
          />
        </UTooltip>
      </div>
    </template>

    <UTooltip
      v-if="activeProject"
      text="Every change you make here is written to disk as you make it."
    >
      <span class="mr-2 flex items-center gap-1.5 text-xs text-muted">
        <!--
          Keyed by name: Nuxt Icon's CSS mode, reused across a name change,
          writes the new name's rule with the old icon's image, and the check
          came out as a spinner.
        -->
        <UIcon
          :key="saving ? 'saving' : 'saved'"
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
      own folders. Every photo the book already holds is a features-cache hit,
      so that part costs nothing; a photo added to the folder since is new work
      and does run Vision, which is what the folder notice warns about.
    -->
    <UTooltip text="Back to the contact sheet, with this book's choices">
      <UButton
        icon="i-lucide-images"
        color="neutral"
        variant="ghost"
        size="sm"
        :disabled="busy || !activeProject"
        @click="requestEditPhotos"
      >
        Edit photos
      </UButton>
    </UTooltip>

    <UTooltip text="Show or hide the chat" :kbds="shortcutKbds('chat')">
      <UButton
        icon="i-lucide-message-square"
        color="neutral"
        :variant="chatOpen ? 'soft' : 'ghost'"
        size="sm"
        :aria-pressed="chatOpen"
        @click="chatOpen = !chatOpen"
      >
        Chat
      </UButton>
    </UTooltip>

    <UTooltip text="Write the print files" :kbds="shortcutKbds('export')">
      <UButton
        icon="i-lucide-download"
        color="primary"
        size="sm"
        :disabled="!activeProject"
        @click="exportOpen = true"
      >
        Export
      </UButton>
    </UTooltip>
  </AppHeader>

  <div class="flex min-h-0 flex-1">
    <div class="flex min-w-0 flex-1 flex-col">
      <!--
        The desk the book lies on: a toned surface, so the white pages read as
        paper and the controls around them recede.
      -->
      <main class="min-h-0 flex-1 overflow-y-auto bg-desk">
        <div v-if="!layout" class="mx-auto max-w-[1400px] space-y-6 p-6">
          <UAlert
            v-if="error"
            icon="i-lucide-triangle-alert"
            color="error"
            variant="subtle"
            title="Something went wrong"
            :description="error"
            :ui="{ description: 'break-words' }"
          />
          <!-- 2.518:1 is Pixajoy's spread, a placeholder: no layout, so no spec, has loaded yet. -->
          <template v-else>
            <USkeleton v-for="n in 3" :key="n" class="aspect-[2.518] w-full" />
          </template>
        </div>

        <UAlert
          v-if="layout && error"
          icon="i-lucide-triangle-alert"
          color="error"
          variant="subtle"
          title="Something went wrong"
          :description="error"
          :ui="{ description: 'break-words' }"
          class="mx-auto mt-6 max-w-[1352px]"
        />

        <!--
          What the folders have gained since the book was generated. Opening a
          book resolves its photos by hash and never walks the disk, so without
          this the only way to notice is to run Edit photos on spec.
        -->
        <UAlert
          v-if="layout && notice"
          icon="i-lucide-folder-plus"
          color="neutral"
          variant="subtle"
          :title="notice.title"
          :description="notice.description"
          class="mx-auto mt-6 max-w-[1352px]"
          :actions="[{ label: 'Edit photos', color: 'neutral', variant: 'outline', onClick: requestEditPhotos }]"
          close
          @update:open="folderNoticeDismissed = true"
        />

        <!--
          The book itself, spread by spread, as it will print, with the
          spread-level controls: regenerate, reject, change layout, lock, shuffle,
          and swapping two photos. Every control sends one `edit_book` and shows
          what came back -- `useBook.editBook`.
        -->
        <BookPreview
          v-if="layout"
          v-model:edit-slots="editSlots"
          :layout
          :busy
          :geometry="printSizeOpen ? proposedGeometry : null"
          @edit="editBook"
        />
      </main>

      <!-- What the book holds, and what a click or a drag does right now. -->
      <footer
        v-if="layout"
        class="flex h-8 shrink-0 items-center gap-4 border-t border-default bg-default px-4 text-xs whitespace-nowrap text-muted tabular-nums"
      >
        <button
          type="button"
          class="-mx-1 rounded px-1 text-default hover:bg-elevated focus-visible:outline-2 focus-visible:outline-primary"
          title="Change the print size"
          @click="printSizeOpen = true"
        >
          {{ sizeLabel(layout.spec, unit) }}
        </button>
        <span><span class="text-default">{{ layout.pageCount }}</span> pages</span>
        <span><span class="text-default">{{ layout.placedPhotos }}</span> photos placed</span>
        <span v-if="blankPages > 0">
          <span class="text-default">{{ blankPages }}</span>
          {{ blankPages === 1 ? "page prints" : "pages print" }} blank
        </span>
        <span v-if="layout.droppedPhotos > 0">
          <span class="text-default">{{ layout.droppedPhotos }}</span> left out
        </span>
        <span class="ml-auto flex min-w-0 items-center gap-1.5">
          <UIcon :key="editSlots ? 'move' : 'crop'" :name="editSlots ? 'i-lucide-move' : 'i-lucide-crop'" class="size-3.5 shrink-0" />
          <span
            class="truncate"
            :title="
              editSlots
                ? 'Drag a box to move it, drag a corner to resize it; edges snap to the guides'
                : 'Drag a photo to move its crop, ⌘-scroll over it to zoom. Click a photo, then another anywhere in the book, to swap them'
            "
          >
            <template v-if="editSlots">
              Drag a box to move it, drag a corner to resize it; edges snap to the guides
            </template>
            <template v-else>
              Drag a photo to move its crop, ⌘-scroll over it to zoom. Click a photo, then another
              anywhere in the book, to swap them
            </template>
          </span>
        </span>
      </footer>
    </div>

    <!--
      Kept mounted while folded away, so closing the panel does not throw away
      the conversation or a proposal still waiting for an answer.
    -->
    <aside
      v-show="chatOpen"
      class="w-96 shrink-0 border-l border-default bg-default"
      aria-label="Chat about this book"
    >
      <BookChat
        :project-id="projectId"
        :layout
        @book-changed="refreshLayout"
        @open-keys="openSettings('keys')"
      />
    </aside>
  </div>

  <USlideover
    v-model:open="exportOpen"
    title="Export"
    description="Print-ready files at the book's print size, checked by pre-flight before any is written."
    :ui="{ content: 'max-w-md' }"
  >
    <template #body>
      <div v-if="activeProject" class="space-y-6">
        <section class="space-y-1">
          <p class="text-sm text-toned tabular-nums">{{ projectDetailLabel(activeProject) }}</p>
          <p v-if="restoredSelection" class="flex items-center gap-1.5 text-xs text-muted">
            <UIcon name="i-lucide-hand" class="size-3 shrink-0" />
            <span>{{ restoredSelection }}</span>
          </p>
        </section>

        <section class="space-y-2" aria-labelledby="export-folder">
          <h3 id="export-folder" class="text-sm font-medium text-highlighted">Output folder</h3>
          <div class="flex items-center gap-2 rounded-md border border-default px-3 py-2">
            <UIcon name="i-lucide-folder" class="size-4 shrink-0 text-muted" />
            <p v-if="outputDir" class="min-w-0 flex-1 truncate text-sm" :title="outputDir">
              Exporting to {{ outputDir }}
            </p>
            <p v-else class="min-w-0 flex-1 text-sm text-muted">
              Choose where the print files should be written.
            </p>
            <UButton
              color="neutral"
              variant="outline"
              size="xs"
              :disabled="busy"
              @click="pickOutputDir"
            >
              {{ outputDir ? "Change output folder" : "Choose output folder" }}
            </UButton>
          </div>
        </section>

        <UButton
          icon="i-lucide-download"
          color="primary"
          block
          :loading="busy"
          :disabled="busy || !outputDir"
          @click="runExport"
        >
          Export
        </UButton>

        <div v-if="progress.running" class="space-y-2">
          <UProgress color="primary" size="sm" :model-value="progress.completed" :max="progress.total" />
          <p class="text-sm text-muted tabular-nums">
            <span class="text-default">{{ progress.completed }}</span> of
            <span class="text-default">{{ progress.total }}</span> photos exported
            ({{ exportPercent }}%)
          </p>
        </div>

        <!--
          Pre-flight. Blocks and Warns are rendered as two separate lists from
          two separate arrays, never one filtered list: they are not the same
          kind of thing, and the difference between them is whether any file was
          written at all.
        -->
        <div v-if="exportResult && counts" class="space-y-4 border-t border-default pt-6">
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
            :ui="{ title: 'break-words', description: 'break-words' }"
            orientation="vertical"
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

          <PreflightFindings :blocking="exportResult.blocking" :warnings="exportResult.warnings" />

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
    </template>
  </USlideover>

  <USlideover
    v-model:open="printSizeOpen"
    title="Print size"
    :overlay="false"
    :ui="{ content: 'max-w-sm' }"
  >
    <template #body>
      <PrintSizePanel
        v-if="layout"
        :spec="layout.spec"
        :project-id="projectId"
        :busy
        @apply="changePrintSize"
        @preview="proposedGeometry = $event"
      />
    </template>
  </USlideover>
</template>
