<script setup lang="ts">
import { folderNames } from "~/types/book";

/**
 * The folders a draft's photos come from, at the top of its settings, so it
 * is plain that a book can draw on several: one set, every photo ranked
 * against all of them.
 */
const { folders, busy = false } = defineProps<{
  /** First picked first. */
  folders: string[];
  busy?: boolean;
}>();

const emit = defineEmits<{ add: []; remove: [folder: string] }>();

const names = computed(() => folderNames(folders));
</script>

<template>
  <section aria-labelledby="source-folders" class="space-y-2">
    <h3 id="source-folders" class="text-sm font-semibold text-highlighted">Photos from</h3>
    <ul class="space-y-1.5">
      <li
        v-for="(folder, i) in folders"
        :key="folder"
        class="flex items-center gap-2 rounded-md border border-default py-1 pr-1 pl-2.5 text-sm"
      >
        <UIcon name="i-lucide-folder" class="size-4 shrink-0 text-muted" />
        <span class="min-w-0 flex-1 truncate py-1 text-default" :title="folder">{{ names[i] }}</span>
        <UTooltip v-if="folders.length > 1" text="Take this folder out">
          <UButton
            icon="i-lucide-x"
            color="neutral"
            variant="ghost"
            :disabled="busy"
            :aria-label="`Take ${names[i]} out`"
            @click="emit('remove', folder)"
          />
        </UTooltip>
      </li>
    </ul>
    <UButton
      icon="i-lucide-folder-plus"
      color="neutral"
      variant="outline"
      block
      :disabled="busy"
      @click="emit('add')"
    >
      Add folders…
    </UButton>
    <p class="text-sm text-muted">Photos from every folder here make one book.</p>
  </section>
</template>
