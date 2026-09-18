<script setup lang="ts">
import { shortcutKbds } from "~/types/shortcuts";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ProjectListItem } from "~/types/book";
import type { Screen } from "~/types/navigation";

const { projects, screen } = defineProps<{
  projects: ProjectListItem[];
  screen: Screen;
}>();

const emit = defineEmits<{
  new: [];
  library: [];
  open: [id: number];
}>();

const { toggleSidebar, openSettings } = useShell();

/** The book on screen, whether open in the editor or back on the contact sheet. */
const activeId = computed(() => {
  if (screen.kind === "editor") return screen.projectId;
  if (screen.kind === "select") return screen.replacing?.id ?? null;
  return null;
});

/** A selection that is not yet a book has no row of its own, so it gets one while it lasts. */
const drafting = computed(() => screen.kind === "select" && screen.replacing === null);

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
      <UTooltip text="Choose photo folders for a new book" :kbds="shortcutKbds('newBook')">
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

    <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      <p class="px-2 pb-1 text-xs font-medium text-muted">Photobooks</p>
      <ul class="space-y-0.5">
        <li v-if="drafting">
          <div :class="itemClass(true)" aria-current="page">
            <span class="flex size-6 shrink-0 items-center justify-center rounded-sm bg-accented">
              <UIcon name="i-lucide-images" class="size-3.5" />
            </span>
            <span class="truncate italic">Untitled selection</span>
          </div>
        </li>
        <li v-for="project in projects" :key="project.id">
          <button
            type="button"
            :class="itemClass(activeId === project.id)"
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
        </li>
      </ul>
      <p v-if="projects.length === 0 && !drafting" class="px-2 py-1 text-xs text-muted">
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
  </nav>
</template>
