<script setup lang="ts">
import type { ProjectListItem } from "~/types/book";
import { LIBRARY_SORTS, libraryView, type LibrarySort } from "~/types/library";
import { shortcutKbds } from "~/types/shortcuts";

const { projects, busy = false, error = null } = defineProps<{
  projects: ProjectListItem[];
  /** Disables every book's actions while a delete/rename is already in flight. */
  busy?: boolean;
  error?: string | null;
}>();

const emit = defineEmits<{
  new: [];
  open: [id: number];
  rename: [id: number, name: string];
  delete: [id: number];
  favourite: [id: number, favourite: boolean];
  reveal: [path: string];
}>();

const query = ref("");
const sort = ref<LibrarySort>("updated");
const view = ref<"grid" | "list">("grid");

const shown = computed(() => libraryView(projects, query.value, sort.value));
const countLabel = computed(() =>
  projects.length === 1 ? "1 photobook" : `${projects.length} photobooks`,
);

const steps = [
  {
    title: "Name it and choose folders",
    text: "Every photo in them and their subfolders is analysed on this Mac, in the background.",
  },
  {
    title: "Review the contact sheet",
    text: "The best of each event is picked for you. Include or leave out any photo.",
  },
  {
    title: "Generate and export",
    text: "Adjust the spreads, then export print-ready files for Pixajoy.",
  },
];
</script>

<template>
  <AppHeader title="Library" :subtitle="projects.length > 0 ? countLabel : null">
    <template v-if="projects.length > 0">
      <UInput
        v-model="query"
        icon="i-lucide-search"
        size="sm"
        placeholder="Search"
        aria-label="Search photobooks by name or folder"
        class="w-56"
      />
      <USelect
        v-model="sort"
        :items="LIBRARY_SORTS"
        value-key="value"
        size="sm"
        aria-label="Sort by"
        class="w-36"
      />
      <UFieldGroup size="sm">
        <UTooltip text="Covers">
          <UButton
            icon="i-lucide-layout-grid"
            color="neutral"
            :variant="view === 'grid' ? 'soft' : 'outline'"
            aria-label="Show as covers"
            :aria-pressed="view === 'grid'"
            @click="view = 'grid'"
          />
        </UTooltip>
        <UTooltip text="List">
          <UButton
            icon="i-lucide-list"
            color="neutral"
            :variant="view === 'list' ? 'soft' : 'outline'"
            aria-label="Show as a list"
            :aria-pressed="view === 'list'"
            @click="view = 'list'"
          />
        </UTooltip>
      </UFieldGroup>
    </template>
  </AppHeader>

  <main class="flex-1 overflow-y-auto">
    <div class="mx-auto max-w-6xl space-y-6 p-8">
      <UAlert
        v-if="error"
        color="error"
        variant="subtle"
        icon="i-lucide-triangle-alert"
        title="Something went wrong"
        :description="error"
        :ui="{ description: 'break-words' }"
      />

      <!-- No books yet: the way in, and what happens after it. -->
      <section
        v-if="projects.length === 0"
        class="mx-auto mt-10 max-w-2xl space-y-8 rounded-xl border border-default p-10"
        aria-labelledby="first-book"
      >
        <div class="space-y-3">
          <h2 id="first-book" class="text-xl font-semibold text-highlighted">
            Create your first photobook
          </h2>
          <p class="max-w-prose text-sm text-muted">
            PhotobookGen analyses the photos you pick on this Mac: sharpness, faces, colour, and
            Apple's aesthetic model. It groups bursts and events, then ranks every photo against
            the others so you can see what's worth printing.
          </p>
          <div class="flex items-center gap-3 pt-2">
            <UButton icon="i-lucide-plus" color="primary" @click="emit('new')">
              New photobook
            </UButton>
            <span class="flex items-center gap-1 text-xs text-muted">
              or press
              <UKbd v-for="key in shortcutKbds('newBook')" :key size="sm" :value="key" />
            </span>
          </div>
        </div>
        <ol class="grid gap-6 border-t border-default pt-8 sm:grid-cols-3">
          <li v-for="(step, i) in steps" :key="step.title" class="space-y-2">
            <div class="flex items-start gap-2 text-sm font-medium text-highlighted">
              <span
                class="flex size-5 shrink-0 items-center justify-center rounded-full bg-inverted text-[11px] text-inverted tabular-nums"
              >
                {{ i + 1 }}
              </span>
              {{ step.title }}
            </div>
            <p class="text-xs text-muted">{{ step.text }}</p>
          </li>
        </ol>
      </section>

      <UEmpty
        v-else-if="shown.length === 0"
        icon="i-lucide-search-x"
        title="No photobooks match"
        :description="`Nothing is named “${query.trim()}” or made from a folder by that name.`"
        :actions="[
          {
            label: 'Clear search',
            color: 'neutral',
            variant: 'outline',
            onClick: () => (query = ''),
          },
        ]"
        class="mt-16"
      />

      <ul
        v-else
        :class="
          view === 'grid'
            ? 'grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-x-6 gap-y-8'
            : 'divide-y divide-default'
        "
      >
        <ProjectItem
          v-for="project in shown"
          :key="project.id"
          :project
          :view
          :busy
          @open="emit('open', $event)"
          @rename="(id, name) => emit('rename', id, name)"
          @delete="emit('delete', $event)"
          @favourite="(id, favourite) => emit('favourite', id, favourite)"
          @reveal="emit('reveal', $event)"
        />
      </ul>
    </div>
  </main>
</template>
