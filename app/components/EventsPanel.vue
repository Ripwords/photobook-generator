<script setup lang="ts">
import { eventRowNote, TIER_LABELS, type EventRow } from "~/types/book";

/**
 * One row per event at the chosen length, as spec §7 describes: its tier,
 * who chose it, and either why (a Skipped or empty event) or how much of it
 * landed in the book. Clicking a row scrolls the contact sheet to it.
 */
const { rows, title } = defineProps<{
  rows: EventRow[];
  /** The place name when Places is on, else "Event N" -- the same function the sheet titles chapters with. */
  title: (event: number) => string;
}>();
const emit = defineEmits<{ reveal: [event: number] }>();
</script>

<template>
  <div class="flex flex-col gap-1 text-sm">
    <h3 class="font-semibold text-highlighted">Events</h3>
    <!--
      This inspector column already holds the name, length, print size,
      switches and the Update button below it -- so only the row list itself
      scrolls once it outgrows a handful of rows, rather than pushing the
      Update button an arbitrary distance down the column.
    -->
    <div class="max-h-56 space-y-1 overflow-y-auto">
      <button
        v-for="row in rows"
        :key="row.event"
        type="button"
        class="grid w-full grid-cols-[1fr_auto] gap-x-3 rounded px-2 py-1 text-left hover:bg-elevated"
        @click="emit('reveal', row.event)"
      >
        <span class="min-w-0 truncate" :class="row.tier === 'featured' ? 'font-semibold' : ''">{{
          title(row.event)
        }}</span>
        <span class="text-muted">
          {{ TIER_LABELS[row.tier] }} <span class="text-dimmed">({{ row.chosen ? "you" : "auto" }})</span>
        </span>
        <span class="col-span-2 text-xs text-muted tabular-nums">{{ eventRowNote(row, title) }}</span>
      </button>
    </div>
  </div>
</template>
