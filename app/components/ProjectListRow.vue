<script setup lang="ts">
import type { ProjectListItem } from "~/types/book";

/**
 * One row in a saved-projects list, shared by `app/pages/index.vue`'s home
 * picker and `GenerateBook.vue`'s "Saved books" panel -- the two places
 * `list_projects` is rendered. Display of the metadata line differs between
 * the two callers (one shows "exported on <date>", the other the full
 * `lastExportLabel`), so that line is a slot rather than baked in here; what
 * IS shared is renaming and deleting, which behave identically everywhere a
 * project can be listed.
 *
 * Deleting and renaming are emitted up rather than invoked here, because the
 * two callers need different state handling on success: `index.vue` has no
 * book state to worry about (its list is only ever shown before a project is
 * open), while `GenerateBook.vue` must route through `useBook`'s
 * `deleteProject`/`renameProject` so deleting the CURRENTLY OPEN project
 * clears it rather than leaving Export wired to a row that no longer exists.
 */
const { project, busy = false } = defineProps<{
  project: ProjectListItem;
  /** Disables every action while a delete/rename/open is already in flight. */
  busy?: boolean;
}>();

const emit = defineEmits<{
  open: [id: number];
  rename: [id: number, name: string];
  delete: [id: number];
}>();

const editing = ref(false);
const draftName = ref(project.name);

// The list can refresh under an in-progress edit (another row's rename just
// completed, reloading `projects`) -- only follow the prop while NOT
// editing, so a live rename never gets clobbered by its own list refresh,
// and an edit of a DIFFERENT row's name never leaks into this one.
watch(
  () => project.name,
  (name) => {
    if (!editing.value) draftName.value = name;
  },
);

function startEditing() {
  draftName.value = project.name;
  editing.value = true;
}

function cancelEditing() {
  editing.value = false;
  draftName.value = project.name;
}

function commitEditing() {
  const trimmed = draftName.value.trim();
  editing.value = false;
  // No-op renames (empty, or unchanged) are silently dropped rather than
  // round-tripped through Rust -- an empty name would be rejected there
  // anyway, and re-sending the same name just to bump `updated_at` would
  // reorder the list for nothing the user asked for.
  if (!trimmed || trimmed === project.name) return;
  emit("rename", project.id, trimmed);
}

const confirmOpen = ref(false);

function confirmDelete() {
  confirmOpen.value = false;
  emit("delete", project.id);
}
</script>

<template>
  <li
    class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 rounded-lg border border-default px-3 py-2"
  >
    <div class="min-w-0 flex-1 space-y-0.5">
      <UInput
        v-if="editing"
        v-model="draftName"
        size="xs"
        autofocus
        :disabled="busy"
        class="max-w-64"
        @keyup.enter="commitEditing"
        @keyup.escape="cancelEditing"
        @blur="commitEditing"
      />
      <p v-else class="truncate text-sm text-default">{{ project.name }}</p>
      <p class="text-xs text-muted"><slot name="meta" /></p>
    </div>
    <div class="flex items-center gap-1">
      <UButton
        icon="i-lucide-pencil"
        color="neutral"
        variant="ghost"
        size="xs"
        :disabled="busy || editing"
        aria-label="Rename this photobook"
        @click="startEditing"
      />
      <UButton
        icon="i-lucide-trash-2"
        color="error"
        variant="ghost"
        size="xs"
        :disabled="busy"
        aria-label="Delete this photobook"
        @click="confirmOpen = true"
      />
      <UButton
        icon="i-lucide-folder-open"
        color="neutral"
        variant="outline"
        size="xs"
        :disabled="busy"
        @click="emit('open', project.id)"
      >
        Open
      </UButton>
    </div>

    <UModal v-model:open="confirmOpen" :title="`Delete “${project.name}”?`" :ui="{ footer: 'justify-end' }">
      <template #body>
        <div class="space-y-3 text-sm">
          <p class="text-default">
            This permanently deletes <span class="font-medium">{{ project.name }}</span
            >'s page layout, your include/exclude decisions, and its export history from
            PhotobookGen. This cannot be undone.
          </p>
          <p class="text-muted">
            Files you already exported to disk are not touched, and the photo analysis cache is
            kept -- reopening this folder later will not re-scan your photos.
          </p>
        </div>
      </template>
      <template #footer>
        <UButton color="neutral" variant="outline" @click="confirmOpen = false">Cancel</UButton>
        <UButton color="error" @click="confirmDelete">Delete photobook</UButton>
      </template>
    </UModal>
  </li>
</template>
</content>
