<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import {
  BRIEF_CAP_RANGE,
  canGenerateAt,
  clampStepper,
  FEATURED_FLOOR_RANGE,
  includeOverflowLabel,
  optionFor,
  pageOptionDescription,
  pageOptionLabel,
  recommendedOption,
  risenLabel,
  type BookOptions,
  type PageOption,
} from "~/types/book";
import type { AnalyzedPhoto, EventTiers, PhotoOverrides } from "~/types/features";
import type { ReplacedProject } from "~/types/navigation";
import { sizeLabel, type PrintSpec } from "~/types/printSpec";

const {
  photos = [],
  overrides = {},
  tiers = {},
  photoSetId = 0,
  runId = 0,
  folders = [],
  replacing = null,
  located = null,
} = defineProps<{
  /** The analysed photos, exactly as Rust sent them -- see `useBook`. */
  photos?: AnalyzedPhoto[];
  /** The user's own include/exclude decisions, persisted with the project by `generate_book`. */
  overrides?: PhotoOverrides;
  /** The user's per-event tier choices, persisted with the project by `generate_book`. */
  tiers?: EventTiers;
  /**
   * Identity of the analysed SET, bumped once per analysis and never per
   * override -- see `usePhotoOverrides.photoSetId`. Watched instead of
   * `photos`, whose array identity changes on every toggle.
   */
  photoSetId?: number;
  /** The analysis `photos` came from -- see `AnalysisSummary.runId`. */
  runId?: number;
  /** The folders the analysed set was drawn from, first picked first. */
  folders?: string[];
  /** The saved book this selection was re-opened from, and can be generated back over. */
  replacing?: ReplacedProject | null;
  /** How many photos carry a location, or `null` until `place_chapters` answers. */
  located?: number | null;
  /** The events panel's row title: the place name when Places is on, else "Event N" -- the same function the sheet titles chapters with. */
  titleOf: (event: number) => string;
}>();

const emit = defineEmits<{
  generated: [projectId: number];
  option: [option: PageOption | undefined];
  /** An events-panel row was clicked; `SelectPhotos` scrolls the sheet to it. */
  reveal: [event: number];
}>();

// `toRef` rather than passing the props straight through: `useBook` holds
// these across async command calls, and a plain value captured at setup time
// would go stale the moment the user analyses a different folder.
const photosRef = toRef(() => photos);
const foldersRef = toRef(() => folders);
const overridesRef = toRef(() => overrides);
const tiersRef = toRef(() => tiers);
const runIdRef = toRef(() => runId);

const {
  recommendation,
  generated,
  busy,
  error,
  refreshRecommendation,
  generate,
  deleteProject,
  reset,
} = useBook(photosRef, foldersRef, overridesRef, tiersRef, runIdRef);

/**
 * The draft's print size, owned by its job so it survives a restart. `null`
 * is the default, which is Rust's to name -- see `default_print_spec`.
 */
const spec = defineModel<PrintSpec | null>("spec", { default: null });
const defaultSpec = ref<PrintSpec | null>(null);
onMounted(async () => {
  try {
    defaultSpec.value = await invoke<PrintSpec>("default_print_spec");
  } catch (e) {
    console.warn("could not read the default print size", e);
  }
});
const shownSpec = computed(() => spec.value ?? defaultSpec.value);
const unit = usePrintUnit();
const printSizeOpen = ref(false);
function choosePrintSize(next: PrintSpec) {
  spec.value = next;
  printSizeOpen.value = false;
}

/** The draft's switches, owned by its job like `spec`. */
const options = defineModel<BookOptions>("options", { required: true });
const places = computed({
  get: () => options.value.places && located !== 0,
  set: (on: boolean) => {
    options.value = { ...options.value, places: on };
  },
});
const featuredFloor = computed({
  get: () => options.value.featuredFloor,
  // `UInputNumber` emits `undefined` when its field is cleared (backspaced
  // to empty, not yet re-typed): `clampStepper` falls back to the current
  // value rather than storing `undefined` on the job, which `recommend_book`
  // would refuse on the very next call.
  set: (n: number | undefined) => {
    options.value = { ...options.value, featuredFloor: clampStepper(n, FEATURED_FLOOR_RANGE, options.value.featuredFloor) };
  },
});
const briefCap = computed({
  get: () => options.value.briefCap,
  set: (n: number | undefined) => {
    options.value = { ...options.value, briefCap: clampStepper(n, BRIEF_CAP_RANGE, options.value.briefCap) };
  },
});

/** The draft's name, owned by its job so the sidebar shows the same one. */
const name = defineModel<string>("name", { required: true });
/**
 * `null` only before the first recommendation arrives -- the watcher below
 * seeds it with the recommended length, so the control is never rendered
 * empty and the user is always overriding a real default rather than
 * choosing from nothing.
 */
const chosenPages = ref<number | null>(null);

/**
 * The "pick a length and generate" flow needs an analysed photo set to
 * recommend against, so it is hidden entirely rather than rendered disabled
 * when there is nothing to recommend over.
 */
const canGenerate = computed(() => photos.length > 0);

const pages = computed(() => chosenPages.value ?? recommendation.value?.recommendedPages ?? null);
const chosenOption = computed(() => {
  if (!recommendation.value) return undefined;
  return pages.value === null
    ? recommendedOption(recommendation.value)
    : optionFor(recommendation.value, pages.value);
});
/** True while the user is still on the recommended length. */
const isRecommended = computed(
  () => pages.value !== null && pages.value === recommendation.value?.recommendedPages,
);
const pageItems = computed(() =>
  (recommendation.value?.options ?? []).map((option) => ({
    label: pageOptionLabel(option),
    // A second line under the label -- Nuxt UI's `USelect` item `description`
    // -- rather than one combined string: "40 pages · 12 of 19 events, 85
    // photos" truncated at the menu's own width and lost the counts first.
    description: pageOptionDescription(option),
    value: option.pages,
    // Not merely expensive -- unbuildable. The engine refuses to choose which
    // of the user's own picks to discard, so generating at this length fails
    // rather than producing a shorter book.
    disabled: !canGenerateAt(option),
  })),
);
/** Why the chosen length cannot be generated, or `null` when it can. */
const overflowMessage = computed(() =>
  chosenOption.value ? includeOverflowLabel(chosenOption.value) : null,
);

/** The tiers' own floors cannot all fit this length -- `PageOption.tierOverflow`. */
const tierOverflowMessage = computed(() => {
  const option = chosenOption.value;
  const overflow = option?.tierOverflow;
  if (!option || !overflow) return null;
  return `These tiers need ${overflow.needed} photos but ${option.pages} pages hold ${overflow.capacity}; every event gets at least one.`;
});

/**
 * "At 40 pages, 3 more events get their own pages" -- shown for the one
 * render right after the CHOSEN LENGTH changes, comparing the length just
 * left to the one just picked, both read from the SAME `recommendation`
 * (`recommend_book` answers every length at once, so this needs no second
 * call).
 *
 * Cleared whenever `recommendation` itself changes reference -- not only
 * when `pages` does. A tier override or a stepper change refreshes
 * `recommendation` in place (see the watcher below) without moving `pages`
 * at all, so the length-only watch used to leave a stale message on screen
 * describing a comparison against an option that no longer exists. Compared
 * event-by-event by `risenLabel`, not by an aggregate count -- see there.
 */
const risenMessage = ref<string | null>(null);
watch(
  [pages, recommendation],
  ([nextPages, nextRecommendation], [prevPages, prevRecommendation]) => {
    risenMessage.value = null;
    // Only a length change WITHIN the same recommendation earns a message: a
    // fresh recommendation invalidates any before/after comparison even if
    // `pages` happens to have moved in the same tick (e.g. the recommended
    // length itself changed).
    if (!nextRecommendation || nextRecommendation !== prevRecommendation) return;
    if (prevPages === null || nextPages === null || prevPages === nextPages) return;
    const before = optionFor(nextRecommendation, prevPages);
    const after = optionFor(nextRecommendation, nextPages);
    if (!before || !after) return;
    risenMessage.value = risenLabel(before, after);
  },
);

// A DIFFERENT ANALYSED SET -- not a different array.
//
// This used to watch `photos`, which `usePhotoOverrides` replaces on every
// toggle: clicking include on one photo therefore threw away the generated
// book, the opened project, the chosen output folder, the export report, a
// name the user had typed, and a page length they had chosen, silently
// reverting a 40-page book to the recommended 20. `photoSetId` changes once
// per analysis, which is the actual question being asked here.
//
// `immediate` so the first render is covered.
watch(
  () => photoSetId,
  () => {
    // Everything below belongs to the PREVIOUS folder: a generated book, the
    // output directory chosen for it, and its export report. Carrying any of
    // them across would leave an "Export" button wired to a book that is no
    // longer on screen.
    reset();
    chosenPages.value = null;
    if (canGenerate.value) void refreshRecommendation(options.value);
  },
  { immediate: true },
);

// The overrides, tiers and options change what the book contains, so they
// change both the keeper count and whether a length can hold every photo the
// user asked for. This is the ONLY refresh those changes trigger -- the
// watcher above used to fire on the same click, sending the whole analysed
// array twice per click.
//
// Nothing else is touched: no reset, no name, no page length. The user is
// refining a selection, not starting over.
watch(
  [() => overrides, () => tiers, () => options.value],
  () => {
    if (canGenerate.value) void refreshRecommendation(options.value);
  },
);

watch(recommendation, (next) => {
  if (next && chosenPages.value === null) chosenPages.value = next.recommendedPages;
});

// Lets the caller dim the contact sheet by what the chosen length actually
// places, rather than by the cull verdict alone -- see `isPlaced`.
watch(chosenOption, (next) => emit("option", next), { immediate: true });

/**
 * `generate_book` always INSERTs, so regenerating a re-edited selection leaves
 * two rows with the same name unless the old one goes. `deleteProject` here is
 * `useBook`'s, which routes through `withProjectDeleted`; the id being deleted
 * is never the one just generated, so the book state is untouched.
 *
 * A failed delete is swallowed on purpose. We navigate regardless, and this
 * screen unmounts in the same tick, so `useBook.error` never gets a frame to
 * render in -- the user is left with a duplicate row in the library and no
 * explanation. That is the deliberate trade: a duplicate is recoverable by
 * deleting it, and a freshly generated book stranded behind an error alert is
 * not.
 */
async function onGenerate(replace: boolean) {
  if (pages.value === null || overflowMessage.value !== null) return;
  await generate(name.value, pages.value, spec.value, options.value);
  const projectId = generated.value?.projectId;
  if (projectId === undefined) return;
  if (replace && replacing) await deleteProject(replacing.id);
  emit("generated", projectId);
}

/** Open by the Update button; only its own Update click calls `onGenerate(true)`. */
const confirmUpdate = ref(false);
async function confirmedUpdate() {
  confirmUpdate.value = false;
  await onGenerate(true);
}
</script>

<template>
  <section class="space-y-4">
    <!--
      A column, for the select screen's inspector: the fields, then what the
      chosen length costs, then the action that commits to it.
    -->
    <div v-if="canGenerate" class="space-y-4">
      <UFormField label="Book name">
        <UInput v-model="name" :disabled="busy" placeholder="Untitled photobook" class="w-full" />
      </UFormField>

      <div v-if="chosenOption && chosenOption.events.length > 0" class="space-y-1.5">
        <EventsPanel :rows="chosenOption.events" :title="titleOf" @reveal="emit('reveal', $event)" />
        <p v-if="tierOverflowMessage" class="text-xs text-muted">{{ tierOverflowMessage }}</p>
        <p v-if="risenMessage" class="text-xs text-muted">{{ risenMessage }}</p>
      </div>

      <UFormField label="Length">
        <!--
          `.nullable` widens the model's type to admit the `null` that means "no
          length chosen yet". It is runtime-inert here: the modifier only maps a
          nullish value to `null`, and this control only ever emits a `pages`.
        -->
        <USelect
          v-model.nullable="chosenPages"
          :items="pageItems"
          :disabled="busy || pageItems.length === 0"
          value-key="value"
          class="w-full"
        />
      </UFormField>

      <UFormField label="Print size">
        <div class="flex items-center justify-between gap-2 rounded-md border border-default px-3 py-1.5">
          <span class="text-sm tabular-nums">{{ shownSpec ? sizeLabel(shownSpec, unit) : "…" }}</span>
          <UButton
            color="neutral"
            variant="outline"
            size="xs"
            :disabled="busy || !shownSpec"
            @click="printSizeOpen = true"
          >
            Change…
          </UButton>
        </div>
      </UFormField>

      <UFormField
        :help="
          located === 0
            ? 'None of these photos records where it was taken.'
            : 'A new chapter starts when you move to another town, not only after a break in time.'
        "
      >
        <USwitch
          v-model="places"
          label="Split chapters by place"
          color="neutral"
          :disabled="busy || located === 0"
        />
      </UFormField>

      <div class="flex items-center gap-2 text-sm text-muted">
        <span>Featured events get at least</span>
        <UInputNumber
          v-model="featuredFloor"
          :min="FEATURED_FLOOR_RANGE.min"
          :max="FEATURED_FLOOR_RANGE.max"
          :disabled="busy"
          size="xs"
          class="w-24"
          aria-label="Featured events get at least this many photos"
        />
        <span>photos</span>
      </div>
      <div class="flex items-center gap-2 text-sm text-muted">
        <span>Brief events get at most</span>
        <UInputNumber
          v-model="briefCap"
          :min="BRIEF_CAP_RANGE.min"
          :max="BRIEF_CAP_RANGE.max"
          :disabled="busy"
          size="xs"
          class="w-24"
          aria-label="Brief events get at most this many photos"
        />
        <span>photos</span>
      </div>
    </div>

    <UModal v-model:open="printSizeOpen" title="Print size" :ui="{ content: 'max-w-md' }">
      <template #body>
        <PrintSizePanel v-if="shownSpec" :spec="shownSpec" @apply="choosePrintSize" />
      </template>
    </UModal>

    <UModal
      v-if="replacing"
      v-model:open="confirmUpdate"
      :title="`Update “${replacing.name}”?`"
      :ui="{ footer: 'justify-end' }"
    >
      <template #body>
        <p class="text-sm text-default">
          Updating builds the book again from these photos and settings. Changes you made in the
          editor (swapped or replaced photos, crops, layouts, moved boxes, the cover, and photos
          added from disk) are replaced. Its undo history, export history and favourite star
          don&rsquo;t carry over. Files you already exported are not touched.
        </p>
      </template>
      <template #footer>
        <UButton color="neutral" variant="outline" @click="confirmUpdate = false">Cancel</UButton>
        <UButton color="primary" :loading="busy" @click="confirmedUpdate">Update</UButton>
      </template>
    </UModal>

    <!--
      The recommendation and its cost, stated before the user commits: a page
      length is a purchase decision, and "26 keepers, 24 fit" is the only
      thing that makes the two SKUs distinguishable. The count placed is keepers
      minus drops rather than `capacityPhotos`: a burst loses its surplus frames
      even in a book with room for them.
    -->
    <p v-if="canGenerate && recommendation" class="text-sm text-muted tabular-nums">
      <span class="text-default">{{ recommendation.keeperCount }}</span>
      keepers<template v-if="recommendation.includedCount > 0">
        (<span class="text-default">{{ recommendation.includedCount }}</span> you picked)</template
      ><template v-if="chosenOption"
        >; <span class="text-default">{{ recommendation.keeperCount - chosenOption.droppedPhotos }}</span> go in
        <span class="text-default">{{ chosenOption.pages }}</span> pages<template
          v-if="chosenOption.droppedPhotos > 0"
          >, so
          <span class="font-medium text-highlighted"
            >{{ chosenOption.droppedPhotos }} would be left out</span
          ></template
        ><template v-else>, nothing left out</template></template
      >.
      <template v-if="isRecommended">This is the recommended length.</template>
      <template v-else>
        The recommended length is
        <span class="text-default">{{ recommendation.recommendedPages }}</span> pages.
      </template>
      Every length offered is a page count Pixajoy sells.
    </p>
    <p v-if="canGenerate && replacing" class="text-xs text-muted">
      Updating replaces the saved book and its export history. Saving as new keeps both.
    </p>

    <!-- The book's own update first: re-editing is usually to change it, not fork it. -->
    <div v-if="canGenerate" class="flex flex-col gap-2">
      <template v-if="replacing">
        <UButton
          block
          icon="i-lucide-book-open"
          color="primary"
          :loading="busy"
          :disabled="busy || !recommendation || overflowMessage !== null"
          @click="confirmUpdate = true"
        >
          Update &ldquo;{{ replacing.name }}&rdquo;
        </UButton>
        <UButton
          block
          icon="i-lucide-copy-plus"
          color="neutral"
          variant="outline"
          :loading="busy"
          :disabled="busy || !recommendation || overflowMessage !== null"
          @click="onGenerate(false)"
        >
          Save as a new photobook
        </UButton>
      </template>
      <UButton
        v-else
        block
        icon="i-lucide-book-open"
        color="primary"
        :loading="busy"
        :disabled="busy || !recommendation || overflowMessage !== null"
        @click="onGenerate(false)"
      >
        Generate book
      </UButton>
    </div>

    <!--
      A length that cannot hold every photo the user explicitly asked for is
      a refusal, not a cost -- so it is stated separately from
      "N would be left out" and it disables the button. The engine will not
      choose which of their own picks to discard.
    -->
    <UAlert
      v-if="overflowMessage"
      icon="i-lucide-triangle-alert"
      color="warning"
      variant="subtle"
      title="This length cannot hold everything you picked"
      :description="`${overflowMessage}. Exclude some photos, or choose a longer book.`"
    />

    <UAlert
      v-if="error"
      icon="i-lucide-triangle-alert"
      color="error"
      variant="subtle"
      title="Something went wrong"
      :description="error"
      :ui="{ description: 'break-words' }"
    />
  </section>
</template>
