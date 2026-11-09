<script setup lang="ts">
import { eventBars, eventRowNote, type EventRow } from "~/types/book";
import type { Tier } from "~/types/features";

/**
 * One row per event at the chosen length, as spec §7 describes, built to be
 * used rather than read: a bar shows how big each event is next to the
 * others and how much of it this length places, the tier menu changes it
 * right here (the same control as the sheet's event header), and clicking
 * the name scrolls the contact sheet to it.
 *
 * Monochrome (memory: ui-style-monochrome): placed is the inverted fill, the
 * rest of the event's kept photos the elevated track.
 */
const { rows, title } = defineProps<{
  rows: EventRow[];
  /** The place name when Places is on, else "Event N" -- the same function the sheet titles chapters with. */
  title: (event: number) => string;
}>();
const emit = defineEmits<{ reveal: [event: number]; tier: [event: number, tier: Tier | "auto"] }>();

const bars = computed(() => eventBars(rows));
/** The pin's key only once a pin is on screen. */
const anyChosen = computed(() => rows.some((row) => row.chosen));
const pct = (fraction: number) => `${fraction * 100}%`;
</script>

<template>
  <section aria-label="Events" class="space-y-3 text-sm">
    <!-- A key, not a sentence: the bars are read by the swatches, which hold in both themes. -->
    <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-muted">
      <span class="flex items-center gap-1.5"><span class="size-3 rounded-sm bg-inverted" />In the book</span>
      <span class="flex items-center gap-1.5">
        <span class="size-3 rounded-sm bg-elevated ring ring-default" />Left out
      </span>
      <span v-if="anyChosen" class="flex items-center gap-1.5"><UIcon name="i-lucide-pin" class="size-3.5" />Your choice</span>
    </div>
    <ul class="divide-y divide-default border-y border-default">
      <li v-for="row in rows" :key="row.event" class="space-y-2 py-3">
        <div class="flex items-center gap-3">
          <button
            type="button"
            class="min-w-0 flex-1 truncate rounded-sm text-left hover:underline focus-visible:outline-2 focus-visible:outline-inverted"
            :class="row.tier === 'skipped' ? 'text-muted' : row.tier === 'featured' ? 'font-semibold text-highlighted' : 'text-default'"
            :title="`Show ${title(row.event)} in the sheet`"
            @click="emit('reveal', row.event)"
          >
            {{ title(row.event) }}
          </button>
          <EventTierControl
            :row
            :title="title(row.event)"
            :title-of="title"
            compact
            @set="emit('tier', row.event, $event)"
          />
        </div>
        <!-- Clicking the bar also scrolls the sheet: the name is the keyboard target, so this one is hidden from it. -->
        <div
          v-if="row.tier !== 'skipped' && row.selected > 0"
          class="flex cursor-pointer items-center gap-3"
          aria-hidden="true"
          @click="emit('reveal', row.event)"
        >
          <span class="h-2 min-w-0 flex-1">
            <span
              class="flex h-full overflow-hidden rounded-full bg-elevated ring ring-default"
              :style="{ width: pct(bars.get(row.event)?.kept ?? 0) }"
            >
              <span
                class="bg-inverted"
                :style="{ width: pct((bars.get(row.event)?.placed ?? 0) / (bars.get(row.event)?.kept || 1)) }"
              />
            </span>
          </span>
          <span class="w-20 shrink-0 text-right text-muted tabular-nums">{{ row.selected }} of {{ row.kept }}</span>
        </div>
        <p v-else class="text-muted">{{ eventRowNote(row, title) }}</p>
        <span v-if="row.tier !== 'skipped' && row.selected > 0" class="sr-only">{{ eventRowNote(row, title) }}</span>
      </li>
    </ul>
  </section>
</template>
