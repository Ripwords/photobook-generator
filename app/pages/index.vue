<script setup lang="ts">
import type { PhotoOverrides } from "~/types/features";
import { selectKey, type Screen } from "~/types/navigation";

const { projects, busy, error, reload, deleteProject, renameProject } = useProjects();
const { pickFolders } = useAnalysis();

/** The single source of navigational truth -- see `Screen`. */
const screen = ref<Screen>({ kind: "library" });

onMounted(() => {
  void reload();
});

// The picker resolves long before the Vision pass does, so the select screen
// goes up immediately and runs the analysis itself rather than leaving the
// user on the library with nothing happening.
async function newBook() {
  const folders = await pickFolders();
  if (folders.length === 0) return;
  screen.value = { kind: "select", folders, overrides: {}, replacing: null };
}

function openBook(id: number) {
  screen.value = { kind: "editor", projectId: id };
}

function toLibrary() {
  screen.value = { kind: "library" };
  // A book generated, renamed or exported since we left changed this list.
  void reload();
}

function onGenerated(projectId: number) {
  screen.value = { kind: "editor", projectId };
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
  <div class="flex h-screen flex-col bg-default text-default">
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
      @close="toLibrary"
      @generated="onGenerated"
    />
    <BookEditor
      v-else
      :key="screen.projectId"
      :project-id="screen.projectId"
      @close="toLibrary"
      @edit-photos="onEditPhotos"
    />
  </div>
</template>
