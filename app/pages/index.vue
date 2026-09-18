<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask } from "@tauri-apps/plugin-dialog";
import type { PhotoOverrides } from "~/types/features";
import type { Screen } from "~/types/navigation";
import { shortcutCombo } from "~/types/shortcuts";
import type { AnalysisJob } from "~/composables/useAnalysisJobs";

const { projects, busy, error, reload, deleteProject, renameProject } = useProjects();
const { jobs, find, startJob, jobForProject, onSettled } = useAnalysisJobs();

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

/**
 * Drafts live only in memory, so quitting with one loses it. The window asks
 * first, and only then.
 */
async function confirmQuit(event: { preventDefault: () => void }) {
  if (jobs.value.length === 0) return;
  event.preventDefault();
  const count = jobs.value.length;
  const quit = await ask(
    `${count === 1 ? "A draft has" : `${count} drafts have`} not been generated into a photobook. Quitting discards ${count === 1 ? "it" : "them"}.`,
    { title: "Quit PhotobookGen?", kind: "warning", okLabel: "Quit", cancelLabel: "Keep working" },
  );
  if (quit) await getCurrentWindow().destroy();
}

let stopAnnouncing: (() => void) | undefined;
let stopConfirmingQuit: (() => void) | undefined;

onMounted(async () => {
  void reload();
  stopAnnouncing = onSettled(announce);
  stopConfirmingQuit = await getCurrentWindow().onCloseRequested(confirmQuit);
});

onBeforeUnmount(() => {
  stopAnnouncing?.();
  stopConfirmingQuit?.();
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
      @open-draft="openJob"
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
        @delete="deleteProject"
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
        @edit-photos="onEditPhotos"
        @renamed="reload"
      />
    </div>
    <SettingsModal />
    <NewBookDialog v-model:open="newBookOpen" @start="onStart" />
  </div>
</template>
