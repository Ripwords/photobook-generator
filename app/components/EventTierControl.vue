<script setup lang="ts">
import type { DropdownMenuItem } from "@nuxt/ui";
import { TIER_LABELS, tierTooltipText, type EventRow } from "~/types/book";
import type { Tier } from "~/types/features";

/**
 * The per-event tier choice, shown in the contact sheet's header row.
 *
 * One button that names the tier in force and who chose it -- "Featured
 * (auto)", "Brief (you)" -- opening a menu of the choices. It replaced a row
 * of four always-visible buttons that repeated on every event header.
 *
 * Monochrome by design (memory: ui-style-monochrome): the chosen item is told
 * apart by its check mark, never by colour.
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

/** Normal has no item of its own: it is what "Auto" resolves to for most events. */
const CHOICES = ["featured", "brief", "skipped"] as const satisfies readonly Tier[];

/** What the menu shows as checked: the user's own choice, or Auto while the app is still deciding. */
const current = computed<Tier | "auto">(() => (row?.chosen ? row.tier : "auto"));

/** The trigger's label, in the events panel's own "(auto)" / "(you)" wording. */
const label = computed(() => {
  if (!row) return "Auto";
  return `${TIER_LABELS[row.tier]} (${row.chosen ? "you" : "auto"})`;
});

/**
 * Chosen or auto, per `tierTooltipText` -- never `tierReasonText(row.reason,
 * ...)` alone, which is always the engine's SUGGESTION reason even once the
 * user has overridden the tier (see that function's doc).
 */
const reason = computed(() => (row ? tierTooltipText(row, titleOf) : ""));

const items = computed<DropdownMenuItem[][]>(() => [
  ...(reason.value ? [[{ type: "label" as const, label: reason.value }]] : []),
  [
    {
      type: "checkbox",
      label: row && !row.chosen ? `Auto: ${TIER_LABELS[row.tier]}` : "Auto",
      checked: current.value === "auto",
      onSelect: () => emit("set", "auto"),
    },
  ],
  CHOICES.map((choice) => ({
    type: "checkbox" as const,
    label: TIER_LABELS[choice],
    checked: current.value === choice,
    onSelect: () => emit("set", choice),
  })),
]);
</script>

<template>
  <UDropdownMenu
    :items
    :content="{ align: 'end' }"
    :ui="{ content: 'max-w-72', label: 'font-normal text-muted' }"
  >
    <UTooltip :text="reason" :disabled="!row" :content="{ side: 'bottom' }">
      <UButton
        color="neutral"
        variant="outline"
        trailing-icon="i-lucide-chevron-down"
        :aria-label="`Tier for ${title}: ${label}`"
      >
        {{ label }}
      </UButton>
    </UTooltip>
  </UDropdownMenu>
</template>
