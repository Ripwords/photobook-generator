<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ContextMenuItem, DropdownMenuItem } from "@nuxt/ui";
import { folderListLabel, lastExportedOn, type ProjectListItem } from "~/types/book";

/**
 * One saved photobook in the library, as a cover card or as a list row.
 *
 * Deleting and renaming are emitted up rather than invoked here, because a
 * caller holding book state must route them through `useBook`'s
 * `deleteProject`/`renameProject` so deleting the CURRENTLY OPEN project
 * clears it rather than leaving Export wired to a row that no longer exists.
 */
const {
  project,
  view,
  busy = false,
} = defineProps<{
  project: ProjectListItem;
  view: "grid" | "list";
  /** Disables every action while a delete/rename/open is already in flight. */
  busy?: boolean;
}>();

const emit = defineEmits<{
  open: [id: number];
  rename: [id: number, name: string];
  delete: [id: number];
}>();

/**
 * The cover: one photo, two side by side, or four in a grid -- never three,
 * which leaves a hole in a 2x2 and reads as a missing photo.
 */
const covers = computed(() => {
  const all = project.coverThumbnails;
  const count = all.length >= 4 ? 4 : all.length >= 2 ? 2 : all.length;
  return all.slice(0, count).map((path) => convertFileSrc(path));
});
const exportedOn = computed(() => lastExportedOn(project));

const editing = ref(false);
const draftName = ref(project.name);

// The list can refresh under an in-progress edit (another book's rename just
// completed, reloading `projects`) -- only follow the prop while NOT
// editing, so a live rename never gets clobbered by its own list refresh,
// and an edit of a DIFFERENT book's name never leaks into this one.
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
 * A click on the book opens it, but not while the name field is live and not
 * while another action is in flight.
 *
 * `editing` alone is not a sufficient guard. Clicking the book to dismiss the
 * name field blurs it first, and that commits the rename and clears `editing`
 * before the click ever lands, so a click meant to close an editor would
 * navigate away from the library instead. `mousedown` runs before `blur`,
 * which is the only moment the state is still true, and it re-reads on every
 * press so it cannot go stale.
 */
const editingAtPress = ref(false);

function notePress() {
  editingAtPress.value = editing.value;
}

function openFromCard() {
  if (editing.value || editingAtPress.value || busy) return;
  emit("open", project.id);
}

const actions = computed<(DropdownMenuItem & ContextMenuItem)[][]>(() => [
  [
    {
      label: "Open",
      icon: "i-lucide-book-open",
      disabled: busy,
      onSelect: () => emit("open", project.id),
    },
    { label: "Rename", icon: "i-lucide-pencil", disabled: busy, onSelect: startEditing },
  ],
  [
    {
      label: "Delete…",
      icon: "i-lucide-trash-2",
      color: "error",
      disabled: busy,
      onSelect: () => (confirmOpen.value = true),
    },
  ],
]);

/**
 * A menu hands focus back to whatever opened it as it closes. After "Rename"
 * that would be the menu button, stealing focus from the name field that just
 * appeared -- and its blur commits the rename before a key is pressed.
 */
const keepFocus = { onCloseAutoFocus: (event: Event) => event.preventDefault() };
</script>

<template>
  <li>
    <UContextMenu :items="actions" :content="keepFocus">
      <article
        class="group relative"
        :class="
          view === 'grid'
            ? 'flex flex-col gap-2.5'
            : 'flex items-center gap-4 rounded-md px-2 py-2 hover:bg-elevated/60'
        "
      >
        <!--
          The whole card opens the book: one real button stretched under
          everything else, so the menu and the name field above it stay
          separate controls rather than buttons nested inside a button.
        -->
        <button
          type="button"
          class="absolute inset-0 z-0 cursor-pointer rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          :aria-label="`Open ${project.name}`"
          :disabled="busy"
          @mousedown="notePress"
          @click="openFromCard"
        />

        <div
          class="pointer-events-none relative grid shrink-0 overflow-hidden bg-elevated ring ring-default transition-shadow group-hover:ring-accented"
          :class="[
            view === 'grid' ? 'aspect-[11/8.5] w-full rounded-lg shadow-sm' : 'aspect-[11/8.5] w-20 rounded',
            covers.length === 4 ? 'grid-cols-2 grid-rows-2 gap-px' : covers.length === 2 ? 'grid-cols-2 gap-px' : '',
          ]"
          aria-hidden="true"
        >
          <img
            v-for="src in covers"
            :key="src"
            :src
            alt=""
            loading="lazy"
            class="size-full object-cover"
          />
          <div v-if="covers.length === 0" class="flex items-center justify-center text-muted">
            <UIcon name="i-lucide-book-image" :class="view === 'grid' ? 'size-8' : 'size-5'" />
          </div>
        </div>

        <div class="pointer-events-none flex min-w-0 flex-1 items-start justify-between gap-2">
          <div class="min-w-0 flex-1 space-y-0.5">
            <UInput
              v-if="editing"
              v-model="draftName"
              size="sm"
              autofocus
              :disabled="busy"
              aria-label="Photobook name"
              class="pointer-events-auto relative z-10 w-full max-w-72"
              @keyup.enter="commitEditing"
              @keyup.escape="cancelEditing"
              @blur="commitEditing"
            />
            <p v-else class="truncate text-sm font-medium text-highlighted">{{ project.name }}</p>
            <p class="truncate text-xs text-muted tabular-nums">
              {{ project.pageCount }} pages, {{ project.photoCount }} photos
            </p>
            <p
              v-if="view === 'list'"
              class="truncate text-xs text-muted"
              :title="project.sourceFolders.join('\n')"
            >
              From {{ folderListLabel(project.sourceFolders) }}
            </p>
          </div>
          <p
            v-if="view === 'list'"
            class="shrink-0 self-center text-xs text-muted tabular-nums"
          >
            {{ exportedOn ? `Exported ${exportedOn}` : "Not exported yet" }}
          </p>
          <UDropdownMenu :items="actions" :content="{ ...keepFocus, align: 'end' }">
            <UButton
              icon="i-lucide-ellipsis"
              color="neutral"
              variant="ghost"
              size="xs"
              :disabled="busy"
              :aria-label="`Actions for ${project.name}`"
              class="pointer-events-auto relative z-10 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
              :class="view === 'list' && 'self-center'"
            />
          </UDropdownMenu>
        </div>
        <p
          v-if="view === 'grid'"
          class="pointer-events-none -mt-2 truncate text-xs text-muted tabular-nums"
          :title="project.sourceFolders.join('\n')"
        >
          {{ exportedOn ? `Exported ${exportedOn}` : "Not exported yet" }}
        </p>
      </article>
    </UContextMenu>

    <UModal
      v-model:open="confirmOpen"
      :title="`Delete “${project.name}”?`"
      :ui="{ footer: 'justify-end' }"
    >
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
