<script setup lang="ts">
import { folderListLabel, lastExportedOn, type ProjectListItem } from "~/types/book";

const { projects, busy = false, error = null } = defineProps<{
  projects: ProjectListItem[];
  /** Disables every row action while a delete/rename is already in flight. */
  busy?: boolean;
  error?: string | null;
}>();

const emit = defineEmits<{
  new: [];
  open: [id: number];
  rename: [id: number, name: string];
  delete: [id: number];
}>();
</script>

<template>
  <AppHeader title="PhotobookGen">
    <UButton icon="i-lucide-plus" color="primary" size="sm" @click="emit('new')">
      New photobook
    </UButton>
  </AppHeader>

  <main class="flex-1 overflow-y-auto p-6">
    <UEmpty
      v-if="projects.length === 0"
      icon="i-lucide-images"
      title="Choose one or more photo folders to begin"
      description="PhotobookGen analyzes every photo in the folders you pick, and in their subfolders, on this Mac: sharpness, faces, color palette, and Apple's aesthetic model. It groups burst shots and events, then ranks each photo against all the others so you can see what's worth printing."
      :actions="[
        {
          label: 'Choose photo folders',
          icon: 'i-lucide-folder-open',
          color: 'primary',
          onClick: () => emit('new'),
        },
      ]"
      class="mx-auto mt-16 max-w-lg"
    />

    <div v-else class="mx-auto max-w-3xl space-y-4">
      <h2 class="text-sm font-medium text-highlighted">
        Your photobooks
        <span class="font-mono tabular-nums text-muted">{{ projects.length }}</span>
      </h2>

      <UAlert
        v-if="error"
        color="error"
        variant="subtle"
        icon="i-lucide-triangle-alert"
        title="Something went wrong"
        :description="error"
        :ui="{ description: 'break-words' }"
      />

      <ul class="space-y-2">
        <ProjectListRow
          v-for="project in projects"
          :key="project.id"
          :project
          :busy
          @open="emit('open', $event)"
          @rename="(id, name) => emit('rename', id, name)"
          @delete="emit('delete', $event)"
        >
          <template #meta>
            <span class="font-mono tabular-nums">{{ project.pageCount }}</span> pages ·
            <span class="font-mono tabular-nums">{{ project.photoCount }}</span> photos ·
            <span :title="project.sourceFolders.join('\n')">{{
              folderListLabel(project.sourceFolders)
            }}</span>
            ·
            <template v-if="lastExportedOn(project)"
              >Exported {{ lastExportedOn(project) }}</template
            >
            <template v-else>Not exported yet</template>
          </template>
        </ProjectListRow>
      </ul>
    </div>
  </main>
</template>
