<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import type { PreviewGeometry } from "~/types/preview";
import {
  applyState,
  checkSummary,
  fieldsFromSpec,
  formatLength,
  lowResolutionNote,
  proposeSpec,
  refusalField,
  sameSpec,
  shapeNote,
  type CheckSummary,
  type LengthUnit,
  type PrintSizeField,
  type PrintSpec,
} from "~/types/printSpec";

/**
 * The book's print size: page, bleed, safe margin, fold and resolution.
 *
 * Every rule lives in Rust. This panel parses what is typed, asks
 * `check_print_spec` about it, and enables Apply only on a `checked` answer --
 * see `applyState`. Shared by the book editor, where the dry run reports what
 * applying would break, and the draft screen, where there is no book yet.
 */
const {
  spec,
  projectId = null,
  busy = false,
} = defineProps<{
  /** The size in force: the book's, or the draft's. */
  spec: PrintSpec;
  /** The book to dry-run against; `null` on the draft screen. */
  projectId?: number | null;
  busy?: boolean;
}>();

const emit = defineEmits<{
  apply: [spec: PrintSpec];
  /** The guides the typed numbers would draw, or `null` for the book's own. */
  preview: [geometry: PreviewGeometry | null];
}>();

const unit = usePrintUnit();
/** The spec the fields were rendered from, so an untouched field keeps its exact inches. */
const base = ref<PrintSpec>(spec);
const fields = ref(fieldsFromSpec(spec, unit.value));
/** Fields left at least once; a parse error shows only after that. */
const blurred = ref(new Set<PrintSizeField>());
/** The field being edited, lit up in the diagram. */
const active = ref<PrintSizeField | null>(null);
function onBlur(key: PrintSizeField) {
  blurred.value.add(key);
  active.value = null;
}

watch(
  () => spec,
  (next) => {
    base.value = next;
    fields.value = fieldsFromSpec(next, unit.value);
    blurred.value = new Set();
  },
);

const proposal = computed(() => proposeSpec(fields.value, base.value, unit.value));
/** The last spec that parsed, so the diagram holds still while a field is mid-edit. */
const drawn = ref<PrintSpec>(spec);
watchEffect(() => {
  if (proposal.value.ok) drawn.value = proposal.value.spec;
});
const { state } = usePrintSpecCheck(toRef(() => projectId), proposal);
const apply = computed(() => applyState(spec, proposal.value, state.value, unit.value));

/**
 * Re-renders from the numbers the fields describe, never writes the display
 * back: the model stays in exact inches whichever unit is on screen.
 */
function setUnit(next: LengthUnit) {
  if (next === unit.value || !proposal.value.ok) return;
  base.value = proposal.value.spec;
  fields.value = fieldsFromSpec(base.value, next);
  unit.value = next;
}

function fieldError(key: PrintSizeField): string | boolean {
  const p = proposal.value;
  if (!p.ok) return p.field === key && blurred.value.has(key) ? p.message : false;
  const s = state.value;
  return s.status === "done" && s.result.kind === "refused" && refusalField(s.result.error) === key;
}

/** Rust's refusal, or a failed check. A parse error shows under its field instead. */
const alert = computed(() => (proposal.value.ok ? apply.value.alert : null));

/** The last dry run, kept while the next one is in flight so the panel does not flicker. */
const summary = ref<CheckSummary | null>(null);
watch(state, (s) => {
  if (s.status !== "done") return;
  if (s.result.kind === "checked") {
    summary.value = checkSummary(s.result);
    emit("preview", s.result.geometry);
  } else {
    summary.value = null;
  }
});
onBeforeUnmount(() => emit("preview", null));

const notes = computed(() => {
  const p = proposal.value;
  return p.ok ? [shapeNote(p.spec), lowResolutionNote(p.spec)].filter((n): n is string => n !== null) : [];
});

const fullPage = computed(() => {
  const p = proposal.value;
  if (!p.ok) return null;
  return `${formatLength(p.spec.pageWIn, unit.value)} x ${formatLength(p.spec.pageHIn, unit.value)} ${unit.value}`;
});

/** The printer this app was built against. Asked of Rust rather than retyped here. */
const pixajoy = ref<PrintSpec | null>(null);
onMounted(async () => {
  try {
    pixajoy.value = await invoke<PrintSpec>("default_print_spec");
  } catch {
    // Only the reset button needs it.
  }
});
const showReset = computed(() => {
  const p = proposal.value;
  return pixajoy.value !== null && !(p.ok && sameSpec(p.spec, pixajoy.value));
});
function reset() {
  if (!pixajoy.value) return;
  base.value = pixajoy.value;
  fields.value = fieldsFromSpec(pixajoy.value, unit.value);
  blurred.value = new Set();
}

function onApply() {
  const p = proposal.value;
  if (p.ok && apply.value.enabled) emit("apply", p.spec);
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-start justify-between gap-4">
      <p class="text-sm text-muted">New books start at Pixajoy's 11 x 8.5 inch landscape book.</p>
      <UFieldGroup size="xs">
        <UButton
          v-for="option in ['in', 'mm'] as const"
          :key="option"
          color="neutral"
          :variant="unit === option ? 'solid' : 'outline'"
          :disabled="!proposal.ok"
          :aria-pressed="unit === option"
          @click="setUnit(option)"
        >
          {{ option }}
        </UButton>
      </UFieldGroup>
    </div>

    <PrintSizeDiagram
      :spec="drawn"
      :unit
      :active
      class="transition-opacity"
      :class="{ 'opacity-50': !proposal.ok }"
    />

    <section class="space-y-3" aria-labelledby="print-size-page">
      <h3 id="print-size-page" class="text-sm font-medium text-highlighted">Page</h3>
      <div>
        <p class="mb-1.5 text-sm text-default">Book size</p>
        <div class="grid grid-cols-2 gap-3">
          <UFormField label="Width" :error="fieldError('trimW')" size="sm">
            <UInput v-model="fields.trimW" inputmode="decimal" :disabled="busy" class="w-full" @focus="active = 'trimW'" @blur="onBlur('trimW')">
              <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
            </UInput>
          </UFormField>
          <UFormField label="Height" :error="fieldError('trimH')" size="sm">
            <UInput v-model="fields.trimH" inputmode="decimal" :disabled="busy" class="w-full" @focus="active = 'trimH'" @blur="onBlur('trimH')">
              <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
            </UInput>
          </UFormField>
        </div>
        <p class="mt-1.5 text-xs text-muted">The finished page, after trimming.</p>
      </div>
      <UFormField label="Bleed" :error="fieldError('bleed')" size="sm">
        <UInput v-model="fields.bleed" inputmode="decimal" :disabled="busy" class="w-full" @focus="active = 'bleed'" @blur="onBlur('bleed')">
          <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
        </UInput>
        <template #help>
          On the three outer edges, trimmed off after printing.
          <span v-if="fullPage" class="tabular-nums">Full page with bleed: {{ fullPage }}.</span>
        </template>
      </UFormField>
    </section>

    <section class="space-y-3" aria-labelledby="print-size-clear">
      <h3 id="print-size-clear" class="text-sm font-medium text-highlighted">Keep clear</h3>
      <UFormField
        label="Safe margin"
        help="Inside the trim. Faces and text stay clear of it."
        :error="fieldError('safeMargin')"
        size="sm"
      >
        <UInput
          v-model="fields.safeMargin"
          inputmode="decimal"
          :disabled="busy"
          class="w-full"
          @focus="active = 'safeMargin'" @blur="onBlur('safeMargin')"
        >
          <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
        </UInput>
      </UFormField>
      <UFormField
        label="Fold"
        help="Measured in from the binding. No face lands in it."
        :error="fieldError('fold')"
        size="sm"
      >
        <UInput v-model="fields.fold" inputmode="decimal" :disabled="busy" class="w-full" @focus="active = 'fold'" @blur="onBlur('fold')">
          <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
        </UInput>
      </UFormField>
    </section>

    <section class="space-y-3" aria-labelledby="print-size-resolution">
      <h3 id="print-size-resolution" class="text-sm font-medium text-highlighted">Resolution</h3>
      <div class="grid grid-cols-2 gap-3">
        <UFormField label="Lowest" help="Below this, export stops." :error="fieldError('minDpi')" size="sm">
          <UInput v-model="fields.minDpi" inputmode="numeric" :disabled="busy" class="w-full" @focus="active = 'minDpi'" @blur="onBlur('minDpi')">
            <template #trailing><span class="text-xs text-muted">DPI</span></template>
          </UInput>
        </UFormField>
        <UFormField label="Target" help="Below this, export warns." :error="fieldError('warnDpi')" size="sm">
          <UInput v-model="fields.warnDpi" inputmode="numeric" :disabled="busy" class="w-full" @focus="active = 'warnDpi'" @blur="onBlur('warnDpi')">
            <template #trailing><span class="text-xs text-muted">DPI</span></template>
          </UInput>
        </UFormField>
      </div>
    </section>

    <section class="space-y-3" aria-labelledby="print-size-cover">
      <h3 id="print-size-cover" class="text-sm font-medium text-highlighted">Cover</h3>
      <UFormField
        label="Cover wrap"
        help="How far the cover photo folds around the board, on every edge but the spine."
        :error="fieldError('coverWrap')"
        size="sm"
      >
        <UInput v-model="fields.coverWrap" inputmode="decimal" :disabled="busy" class="w-full" @focus="active = 'coverWrap'" @blur="onBlur('coverWrap')">
          <template #trailing><span class="text-xs text-muted">{{ unit }}</span></template>
        </UInput>
      </UFormField>
    </section>

    <ul v-if="notes.length > 0" class="space-y-1 text-xs text-muted">
      <li v-for="note in notes" :key="note" class="flex gap-1.5">
        <UIcon name="i-lucide-info" class="mt-0.5 size-3.5 shrink-0" />
        <span>{{ note }}</span>
      </li>
    </ul>

    <!-- What applying would do to this book, from Rust's dry run. -->
    <div v-if="projectId !== null && summary" class="space-y-3 border-t border-default pt-4" :class="{ 'opacity-60': state.status === 'checking' }">
      <p class="text-sm text-default">{{ summary.text }}</p>
      <p v-if="summary.recrops" class="text-xs text-muted">
        Crops you adjusted by hand are recomputed for the new page shape.
      </p>
      <PreflightFindings :blocking="summary.blocking" :warnings="summary.warnings" />
    </div>

    <UAlert
      v-if="alert"
      icon="i-lucide-triangle-alert"
      color="error"
      variant="subtle"
      :description="alert"
      :ui="{ description: 'break-words' }"
    />

    <div class="flex flex-col gap-2">
      <UButton block color="primary" :loading="busy" :disabled="busy || !apply.enabled" @click="onApply">
        {{ apply.label }}
      </UButton>
      <UButton v-if="showReset" block color="neutral" variant="ghost" :disabled="busy" @click="reset">
        Reset to Pixajoy 11 x 8.5
      </UButton>
    </div>
  </div>
</template>
