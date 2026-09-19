<script setup lang="ts">
import type { PreflightFinding } from "~/types/book";

/**
 * Pre-flight's two lists. Blocks and warnings are separate arrays, never one
 * filtered list: they are not the same kind of thing. Shared by the export
 * sheet and the Print size panel so a finding reads the same in both.
 */
const { blocking, warnings } = defineProps<{
  blocking: PreflightFinding[];
  warnings: PreflightFinding[];
}>();
</script>

<template>
  <div v-if="blocking.length > 0" class="space-y-2">
    <h4 class="flex items-center gap-2 text-sm font-medium text-highlighted">
      <UIcon name="i-lucide-octagon-x" class="size-4 text-error" />
      Blocking ({{ blocking.length }})
    </h4>
    <ul class="space-y-1 text-sm text-muted">
      <li v-for="(f, i) in blocking" :key="`block-${i}`" class="break-words">
        <span v-if="f.page > 0" class="tabular-nums text-default">Page {{ f.page }}:</span>
        {{ f.message }}
      </li>
    </ul>
  </div>

  <div v-if="warnings.length > 0" class="space-y-2">
    <h4 class="flex items-center gap-2 text-sm font-medium text-highlighted">
      <UIcon name="i-lucide-triangle-alert" class="size-4 text-warning" />
      Warnings ({{ warnings.length }})
    </h4>
    <ul class="space-y-1 text-sm text-muted">
      <li v-for="(f, i) in warnings" :key="`warn-${i}`" class="break-words">
        <span v-if="f.page > 0" class="tabular-nums text-default">Page {{ f.page }}:</span>
        {{ f.message }}
      </li>
    </ul>
  </div>
</template>
