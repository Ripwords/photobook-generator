<script setup lang="ts">
import { shortcutKbds } from "~/types/shortcuts";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ProjectListItem } from "~/types/book";
import { bookActions, sidebarSections } from "~/types/library";
import type { Screen } from "~/types/navigation";
import { jobProgress, type AnalysisJob } from "~/composables/useAnalysisJobs";

const {
  projects,
  screen,
  drafts,
  busy = false,
} = defineProps<{
  projects: ProjectListItem[];
  screen: Screen;
  /** Books still being chosen, some of them still analysing. */
  drafts: AnalysisJob[];
  /** A delete, rename or star is already in flight. */
  busy?: boolean;
}>();

const emit = defineEmits<{
  new: [];
  library: [];
  open: [id: number];
  openDraft: [jobId: number];
  favourite: [id: number, favourite: boolean];
  rename: [id: number, name: string];
  delete: [id: number];
  reveal: [path: string];
}>();

const { toggleSidebar, openSettings } = useShell();

/** The book open in the editor. */
const activeId = computed(() => (screen.kind === "editor" ? screen.projectId : null));
/** The draft whose contact sheet is on screen. */
const activeDraftId = computed(() => (screen.kind === "select" ? screen.jobId : null));

/** A draft's state in a few words, beside its name. */
function draftStatus(draft: AnalysisJob): string {
  if (draft.running) {
    const { processed, total } = jobProgress(draft);
    return total > 0 ? `${processed}/${total}` : "Scanning";
  }
  return draft.error ? "Failed" : "Ready";
}

function draftLabel(draft: AnalysisJob): string {
  if (draft.running) return `${draft.name}, analysing, ${draftStatus(draft)} photos`;
  return draft.error ? `${draft.name}, analysis failed` : `${draft.name}, ready to choose photos`;
}

/** Starred books first, each under its own heading; the rest keep the list's order. */
const sections = computed(() => {
  const { favourites, others } = sidebarSections(projects);
  return [
    { id: "sidebar-favourites", title: "Favourites", books: favourites },
    { id: "sidebar-books", title: "Photobooks", books: others },
  ].filter((section) => section.books.length > 0 || (section.id === "sidebar-books" && projects.length === 0));
});

/** The book a dialog was opened for, from its right-click menu. */
const renaming = ref<ProjectListItem | null>(null);
const deleting = ref<ProjectListItem | null>(null);
const renameOpen = ref(false);
const deleteOpen = ref(false);

function actions(project: ProjectListItem) {
  return bookActions(
    project,
    {
      open: (id) => emit("open", id),
      favourite: (id, favourite) => emit("favourite", id, favourite),
      rename: () => {
        renaming.value = project;
        renameOpen.value = true;
      },
      reveal: (path) => emit("reveal", path),
      delete: () => {
        deleting.value = project;
        deleteOpen.value = true;
      },
    },
    busy,
  );
}

function cover(project: ProjectListItem): string | null {
  const first = project.coverThumbnails[0];
  return first ? convertFileSrc(first) : null;
}

const itemClass = (active: boolean) => [
  "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-primary",
  active
    ? "bg-accented/70 font-medium text-highlighted"
    : "text-toned hover:bg-elevated hover:text-highlighted",
];
</script>

<template>
  <nav aria-label="Photobooks" class="flex h-full w-60 shrink-0 flex-col border-r border-default bg-muted">
    <!-- Clear of the traffic lights, and the part of the sidebar the window drags by. -->
    <div data-tauri-drag-region class="flex h-12 shrink-0 items-center justify-end px-2">
      <UTooltip text="Hide sidebar" :kbds="shortcutKbds('sidebar')">
        <UButton
          icon="i-lucide-panel-left"
          color="neutral"
          variant="ghost"
          size="sm"
          aria-label="Hide sidebar"
          @click="toggleSidebar"
        />
      </UTooltip>
    </div>

    <div class="space-y-0.5 px-2 pb-3">
      <UTooltip text="Name a new book and choose its photos" :kbds="shortcutKbds('newBook')">
        <UButton
          icon="i-lucide-plus"
          color="neutral"
          variant="outline"
          block
          class="mb-2 justify-start bg-default"
          @click="emit('new')"
        >
          New photobook
        </UButton>
      </UTooltip>
      <button
        type="button"
        :class="itemClass(screen.kind === 'library')"
        :aria-current="screen.kind === 'library' ? 'page' : undefined"
        @click="emit('library')"
      >
        <UIcon name="i-lucide-library" class="size-4 shrink-0" />
        <span class="flex-1">Library</span>
        <span class="text-xs text-muted tabular-nums">{{ projects.length }}</span>
      </button>
    </div>

    <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-2 pb-2">
      <section v-if="drafts.length > 0" aria-labelledby="sidebar-drafts">
        <p id="sidebar-drafts" class="px-2 pb-1 text-xs font-medium text-muted">Drafts</p>
        <ul class="space-y-0.5">
          <li v-for="draft in drafts" :key="draft.id">
            <button
              type="button"
              :class="itemClass(activeDraftId === draft.id)"
              :aria-current="activeDraftId === draft.id ? 'page' : undefined"
              :aria-busy="draft.running"
              :aria-label="draftLabel(draft)"
              :title="draft.folders.join('\n')"
              @click="emit('openDraft', draft.id)"
            >
              <span class="flex size-6 shrink-0 items-center justify-center rounded-sm bg-accented">
                <UIcon
                  v-if="draft.running"
                  name="i-lucide-loader-circle"
                  class="size-3.5 motion-safe:animate-spin"
                />
                <UIcon v-else-if="draft.error" name="i-lucide-triangle-alert" class="size-3.5" />
                <UIcon v-else name="i-lucide-images" class="size-3.5" />
              </span>
              <span class="flex-1 truncate">{{ draft.name || "Untitled photobook" }}</span>
              <span class="shrink-0 text-xs font-normal text-muted tabular-nums">
                {{ draftStatus(draft) }}
              </span>
            </button>
          </li>
        </ul>
      </section>

      <section v-for="section in sections" :key="section.id" :aria-labelledby="section.id">
        <p :id="section.id" class="px-2 pb-1 text-xs font-medium text-muted">{{ section.title }}</p>
        <ul class="space-y-0.5">
          <li v-for="project in section.books" :key="project.id">
            <UContextMenu :items="actions(project)">
              <button
                type="button"
                :class="itemClass(activeId === project.id)"
                class="data-[state=open]:bg-accented/50 data-[state=open]:text-highlighted"
                :aria-current="activeId === project.id ? 'page' : undefined"
                :title="project.name"
                @click="emit('open', project.id)"
              >
                <img
                  v-if="cover(project)"
                  :src="cover(project) ?? undefined"
                  alt=""
                  class="size-6 shrink-0 rounded-sm object-cover ring ring-default"
                />
                <span v-else class="flex size-6 shrink-0 items-center justify-center rounded-sm bg-accented">
                  <UIcon name="i-lucide-book-image" class="size-3.5" />
                </span>
                <span class="truncate">{{ project.name }}</span>
              </button>
            </UContextMenu>
          </li>
        </ul>
      </section>
      <p v-if="projects.length === 0" class="px-2 py-1 text-xs text-muted">
        Books you make appear here.
      </p>
    </div>

    <div class="border-t border-default p-2">
      <UTooltip text="Settings" :kbds="shortcutKbds('settings')">
        <button type="button" :class="itemClass(false)" @click="openSettings()">
          <UIcon name="i-lucide-settings" class="size-4 shrink-0" />
          Settings
        </button>
      </UTooltip>
    </div>
    <RenameBookDialog
      v-if="renaming"
      v-model:open="renameOpen"
      :name="renaming.name"
      @rename="(name) => renaming && emit('rename', renaming.id, name)"
    />
    <DeleteBookDialog
      v-if="deleting"
      v-model:open="deleteOpen"
      :name="deleting.name"
      @confirm="deleting && emit('delete', deleting.id)"
    />
  </nav>
</template>
