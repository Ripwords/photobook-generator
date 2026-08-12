<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { keepers } from "~/types/features";

const { summary, running, error, pickFolderAndAnalyze } = useAnalysis();
const kept = computed(() => (summary.value ? keepers(summary.value.photos) : []));
</script>

<template>
  <main class="p-8 space-y-6">
    <div class="flex items-center gap-4">
      <h1 class="text-2xl font-semibold">Photobook Generator</h1>
      <UButton :loading="running" @click="pickFolderAndAnalyze">
        Choose photo folder
      </UButton>
    </div>

    <UAlert v-if="error" color="error" :title="error" />

    <div v-if="summary" class="space-y-4">
      <div class="flex gap-6 text-sm">
        <span>{{ summary.total }} scanned</span>
        <span>{{ summary.photos.length }} analysed</span>
        <span>{{ summary.cached }} from cache</span>
        <span>{{ summary.failed }} failed</span>
        <span class="font-medium">{{ kept.length }} keepers</span>
      </div>

      <UTable
        :data="kept"
        :columns="[
          { id: 'thumbnail', header: '' },
          { accessorKey: 'path', header: 'Photo' },
          { accessorKey: 'aestheticPct', header: 'Aesthetic' },
          { accessorKey: 'sharpnessPct', header: 'Sharpness' },
          { accessorKey: 'faceCount', header: 'Faces' },
          { accessorKey: 'eventCluster', header: 'Event' },
        ]"
      >
        <template #thumbnail-cell="{ row }">
          <img
            v-if="row.original.thumbnailPath"
            :src="convertFileSrc(row.original.thumbnailPath)"
            alt=""
            width="60"
            height="60"
          />
        </template>
      </UTable>
    </div>
  </main>
</template>
