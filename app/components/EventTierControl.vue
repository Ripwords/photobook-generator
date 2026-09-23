<script setup lang="ts">
import { TIER_LABELS, tierTooltipText, type EventRow } from "~/types/book";
import type { Tier } from "~/types/features";

/**
 * The per-event tier choice, shown in the contact sheet's header row.
 *
 * Monochrome by design (memory: ui-style-monochrome): tiers are told apart
 * by label and by which button is pressed (`solid` vs `outline`), never by
 * colour.
 *
 * Follows the segmented-choice pattern `PrintSizePanel.vue`'s unit toggle
 * already uses (`UFieldGroup` of `UButton`s) rather than `UTabs`, which
 * nothing else in this codebase uses yet.
 */
const { row, title, titleOf } = defineProps<{
  row: EventRow | undefined;
  title: string;
  /**
   * Names another event by id. A `similarTo` reason points at a different
   * event than the one this control is for, so the reason text needs the
   * real lookup rather than always naming this control's own `title`.
   */
  titleOf: (event: number) => string;
}>();
const emit = defineEmits<{ set: [tier: Tier | "auto"] }>();

/** Normal has no button of its own: it is what "Auto" resolves to for most events. */
const CHOICES = ["featured", "auto", "brief", "skipped"] as const satisfies readonly (Tier | "auto")[];

/** What the control shows as pressed: the user's own choice, or Auto while the app is still deciding. */
const current = computed<Tier | "auto">(() => (row?.chosen ? row.tier : "auto"));

function labelFor(choice: (typeof CHOICES)[number]): string {
  if (choice !== "auto") return TIER_LABELS[choice];
  return row && !row.chosen ? `Auto: ${TIER_LABELS[row.tier]}` : "Auto";
}

/**
 * Chosen or auto, per `tierTooltipText` -- never `tierReasonText(row.reason,
 * ...)` alone, which is always the engine's SUGGESTION reason even once the
 * user has overridden the tier (see that function's doc).
 */
const reason = computed(() => (row ? tierTooltipText(row, titleOf) : ""));
</script>

<template>
  <UTooltip :text="reason" :disabled="!row" :content="{ side: 'bottom' }">
    <UFieldGroup size="xs" role="group" :aria-label="`Tier for ${title}`">
      <UButton
        v-for="choice in CHOICES"
        :key="choice"
        color="neutral"
        :variant="current === choice ? 'solid' : 'outline'"
        :aria-pressed="current === choice"
        @click="emit('set', choice)"
      >
        {{ labelFor(choice) }}
      </UButton>
    </UFieldGroup>
  </UTooltip>
</template>
