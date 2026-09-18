<script setup lang="ts">
import type { ProjectListItem } from "~/types/book";

/**
 * One row in a saved-projects list. The metadata line is a slot rather than
 * baked in here so a caller can show whatever part of the project it cares
 * about; what IS shared is renaming and deleting, which behave identically
 * everywhere a project can be listed.
 *
 * Deleting and renaming are emitted up rather than invoked here, because a
 * caller holding book state must route them through `useBook`'s
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

/**
 * A click anywhere on the row opens it, but not while the name field is live
 * and not while another action is in flight.
 *
 * `editing` alone is not a sufficient guard. Clicking the row to dismiss the
 * name field blurs it first, and that commits the rename and clears `editing`
 * before the click ever lands, so a click meant to close an editor would
 * navigate away from the list instead. `mousedown` runs before `blur`, which
 * is the only moment the state is still true, and it re-reads on every press
 * so it cannot go stale.
 */
const editingAtPress = ref(false);

function notePress() {
  editingAtPress.value = editing.value;
}

function openFromRow() {
  if (editing.value || editingAtPress.value || busy) return;
  emit("open", project.id);
}
</script>

<template>
  <li
    class="group flex cursor-pointer flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-3.5 transition-colors hover:bg-elevated/60 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary"
    role="button"
    tabindex="0"
    @mousedown="notePress"
    @click="openFromRow"
    @keydown.enter="openFromRow"
  >
    <div class="min-w-0 flex-1 space-y-1">
      <UInput
        v-if="editing"
        v-model="draftName"
        size="sm"
        autofocus
        :disabled="busy"
        aria-label="Photobook name"
        class="max-w-72"
        @keyup.enter="commitEditing"
        @keyup.escape="cancelEditing"
        @blur="commitEditing"
      />
      <p v-else class="truncate font-serif text-lg leading-snug text-highlighted">
        {{ project.name }}
      </p>
      <p class="text-xs text-muted"><slot name="meta" /></p>
    </div>
    <!--
      Both handlers, not just `@click.stop`. Enter on a focused action button
      bubbles a keydown to the row before the browser turns it into a click, so
      without this Enter on the pencil or the trash opens the project instead of
      renaming or deleting.
    -->
    <p v-if="$slots.status" class="text-xs text-muted"><slot name="status" /></p>
    <div class="flex items-center gap-1" @click.stop @keydown.enter.stop>
      <UButton
        icon="i-lucide-pencil"
        color="neutral"
        variant="ghost"
        size="xs"
        :disabled="busy || editing"
        aria-label="Rename this photobook"
        @click="startEditing"
      />
      <!-- Neutral until pointed at: a red bin on every row shouts about the rarest action. -->
      <UButton
        icon="i-lucide-trash-2"
        color="neutral"
        variant="ghost"
        size="xs"
        class="hover:text-error focus-visible:text-error"
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
    </div>
  </li>
</template>
