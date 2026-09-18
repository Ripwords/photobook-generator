<script setup lang="ts">
import type { PhotoOverrides } from "~/types/features";
import { selectKey, type Screen } from "~/types/navigation";
import { shortcutCombo } from "~/types/shortcuts";

const { projects, busy, error, reload, deleteProject, renameProject } = useProjects();
const { pickFolders } = useAnalysis();

/** The single source of navigational truth -- see `Screen`. */
const screen = ref<Screen>({ kind: "library" });

const { sidebarOpen, toggleSidebar, openSettings, leave } = useShell();

onMounted(() => {
  void reload();
});

// The picker resolves long before the Vision pass does, so the select screen
// goes up immediately and runs the analysis itself rather than leaving the
// user on the library with nothing happening.
//
// Folders first, then the guard: cancelling the picker should never have
// asked the user to discard anything.
async function newBook() {
  const folders = await pickFolders();
  if (folders.length === 0) return;
  leave(() => {
    screen.value = { kind: "select", folders, overrides: {}, replacing: null };
  });
}

function openBook(id: number) {
  screen.value = { kind: "editor", projectId: id };
}

function toLibrary() {
  screen.value = { kind: "library" };
  // A book generated, renamed or exported since we left changed this list.
  void reload();
}

// The sidebar navigates from anywhere, so it goes through the current
// screen's guard. The screens' own transitions below do not: generating has
// just saved the decisions the guard protects.
function sidebarLibrary() {
  leave(toLibrary);
}

function sidebarOpenBook(id: number) {
  leave(() => openBook(id));
}

// `usingInput` because a Mac app's ⌘ shortcuts work with focus in a text
// field too; none of these combinations types anything.
defineShortcuts({
  [shortcutCombo("newBook")]: { usingInput: true, handler: () => void newBook() },
  [shortcutCombo("sidebar")]: { usingInput: true, handler: toggleSidebar },
  [shortcutCombo("settings")]: { usingInput: true, handler: () => openSettings() },
});

function onGenerated(projectId: number) {
  screen.value = { kind: "editor", projectId };
  // The sidebar lists every book, so the one just made belongs in it now.
  void reload();
}

function onEditPhotos(payload: {
  projectId: number;
  name: string;
  sourceFolders: string[];
  overrides: PhotoOverrides;
}) {
  screen.value = {
    kind: "select",
    folders: payload.sourceFolders,
    overrides: payload.overrides,
    replacing: { id: payload.projectId, name: payload.name },
  };
}
</script>

<template>
  <div class="flex h-screen bg-default text-default">
    <AppSidebar
      v-show="sidebarOpen"
      :projects
      :screen
      @new="newBook"
      @library="sidebarLibrary"
      @open="sidebarOpenBook"
    />
    <div class="flex min-w-0 flex-1 flex-col">
      <!--
        The `:key` on each screen is load-bearing: both read their props once in
        `onMounted`, so remounting is what makes re-entering one with different
        props actually re-run.
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
        v-else-if="screen.kind === 'select'"
        :key="selectKey(screen.folders)"
        :folders="screen.folders"
        :restore-overrides="screen.overrides"
        :replacing="screen.replacing"
        @generated="onGenerated"
      />
      <BookEditor
        v-else
        :key="screen.projectId"
        :project-id="screen.projectId"
        @edit-photos="onEditPhotos"
        @renamed="reload"
      />
    </div>
    <SettingsModal />
  </div>
</template>
