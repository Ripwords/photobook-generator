<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { EventTiers, PhotoOverrides } from "~/types/features";
import type { Screen } from "~/types/navigation";
import type { BookOptions } from "~/types/book";
import type { PrintSpec } from "~/types/printSpec";
import { shortcutCombo } from "~/types/shortcuts";
import type { AnalysisJob } from "~/composables/useAnalysisJobs";

const {
  projects,
  busy,
  error,
  reload,
  deleteProject,
  restoreProject,
  renameProject,
  setFavourite,
  reveal,
} = useProjects();
const { jobs, find, startJob, jobForProject, onSettled, restoreDrafts, flushDrafts } =
  useAnalysisJobs();

/** The single source of navigational truth -- see `Screen`. */
const screen = ref<Screen>({ kind: "library" });

const { sidebarOpen, toggleSidebar, openSettings } = useShell();
const toast = useToast();

/** The draft on screen, while the select screen is up. */
const selectedJob = computed(() =>
  screen.value.kind === "select" ? find(screen.value.jobId) : undefined,
);

const newBookOpen = ref(false);

function newBook() {
  newBookOpen.value = true;
}

function openJob(jobId: number) {
  screen.value = { kind: "select", jobId };
}

// The draft opens the moment it starts: the contact sheet fills in as the
// analysis streams, and leaving it does not stop the analysis.
function onStart(book: { name: string; folders: string[] }) {
  openJob(startJob(book));
}

function openBook(id: number) {
  screen.value = { kind: "editor", projectId: id };
}

// Deleting asks first (`DeleteBookDialog`), and can still be undone from here.
async function deleteBook(id: number) {
  const name = projects.value.find((project) => project.id === id)?.name ?? "Photobook";
  if (!(await deleteProject(id))) return;
  // The sidebar can delete the book on screen; the editor must not outlive it.
  if (screen.value.kind === "editor" && screen.value.projectId === id) {
    screen.value = { kind: "library" };
  }
  toast.add({
    title: `Deleted “${name}”`,
    icon: "i-lucide-trash-2",
    color: "neutral",
    actions: [
      {
        label: "Undo",
        color: "neutral",
        variant: "outline",
        onClick: () => void restoreProject(id),
      },
    ],
  });
}

// The library shows `error` in place; anywhere else a failed sidebar action
// would otherwise say nothing.
watch(error, (message) => {
  if (!message || screen.value.kind === "library") return;
  toast.add({ title: "That did not work", description: message, icon: "i-lucide-circle-x", color: "neutral" });
});

/** The open book's name as the list has it, so a sidebar rename shows in the editor too. */
const listedName = computed(() => {
  const current = screen.value;
  if (current.kind !== "editor") return undefined;
  return projects.value.find((project) => project.id === current.projectId)?.name;
});

function toLibrary() {
  screen.value = { kind: "library" };
  // A book generated, renamed or exported since we left changed this list.
  void reload();
}

// `usingInput` because a Mac app's ⌘ shortcuts work with focus in a text
// field too; none of these combinations types anything.
defineShortcuts({
  [shortcutCombo("newBook")]: { usingInput: true, handler: newBook },
  [shortcutCombo("sidebar")]: { usingInput: true, handler: toggleSidebar },
  [shortcutCombo("settings")]: { usingInput: true, handler: () => openSettings() },
});

function onGenerated(projectId: number) {
  screen.value = { kind: "editor", projectId };
  // The sidebar lists every book, so the one just made belongs in it now.
  void reload();
}

// Editing a book's photos twice opens the draft already doing it, rather than
// a second one analysing the same folders.
function onEditPhotos(payload: {
  projectId: number;
  name: string;
  sourceFolders: string[];
  overrides: PhotoOverrides;
  tiers: EventTiers;
  spec: PrintSpec | null;
  options: BookOptions | null;
}) {
  const existing = jobForProject(payload.projectId);
  if (existing) {
    openJob(existing.id);
    return;
  }
  openJob(
    startJob({
      name: payload.name,
      folders: payload.sourceFolders,
      replacing: { id: payload.projectId, name: payload.name },
      restoreOverrides: payload.overrides,
      restoreTiers: payload.tiers,
      spec: payload.spec,
      options: payload.options ?? undefined,
    }),
  );
}

/** Says a draft finished, unless the user is already looking at it. */
function announce(job: AnalysisJob) {
  if (selectedJob.value?.id === job.id) return;
  toast.add({
    title: job.error ? `“${job.name}” could not be analysed` : `“${job.name}” is ready`,
    description: job.error ? "Open it to try again." : "Its photos are ranked and ready to choose from.",
    icon: job.error ? "i-lucide-triangle-alert" : "i-lucide-circle-check",
    color: "neutral",
    actions: [
      {
        label: "Open",
        color: "neutral",
        variant: "outline",
        onClick: () => openJob(job.id),
      },
    ],
  });
}

let stopAnnouncing: (() => void) | undefined;
let stopFlushingDrafts: (() => void) | undefined;

onMounted(async () => {
  void reload();
  stopAnnouncing = onSettled(announce);
  void restoreDrafts();
  // Drafts are saved as they change; this waits for the last save to land.
  stopFlushingDrafts = await getCurrentWindow().onCloseRequested(flushDrafts);
});

onBeforeUnmount(() => {
  stopAnnouncing?.();
  stopFlushingDrafts?.();
});
</script>

<template>
  <div class="flex h-screen bg-default text-default">
    <AppSidebar
      v-show="sidebarOpen"
      :projects
      :screen
      :drafts="jobs"
      @new="newBook"
      @library="toLibrary"
      @open="openBook"
      :busy
      @open-draft="openJob"
      @favourite="setFavourite"
      @rename="renameProject"
      @delete="deleteBook"
      @reveal="reveal"
    />
    <div class="flex min-w-0 flex-1 flex-col">
      <!--
        The `:key` on each screen is load-bearing: remounting is what makes
        re-entering one for a different draft or book start from that one.
      -->
      <ProjectLibrary
        v-if="screen.kind === 'library'"
        :projects
        :busy
        :error
        @new="newBook"
        @open="openBook"
        @rename="renameProject"
        @delete="deleteBook"
        @favourite="setFavourite"
        @reveal="reveal"
      />
      <SelectPhotos
        v-else-if="selectedJob"
        :key="selectedJob.id"
        :job="selectedJob"
        @generated="onGenerated"
        @discarded="toLibrary"
      />
      <BookEditor
        v-else-if="screen.kind === 'editor'"
        :key="screen.projectId"
        :project-id="screen.projectId"
        :listed-name
        @edit-photos="onEditPhotos"
        @renamed="reload"
      />
    </div>
    <SettingsModal />
    <NewBookDialog v-model:open="newBookOpen" @start="onStart" />
  </div>
</template>
