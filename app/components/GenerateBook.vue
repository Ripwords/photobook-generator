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
  photoFunnel,
  pageOptionLabel,
  recommendedOption,
  risenLabel,
  type BookOptions,
  type PageOption,
} from "~/types/book";
import type { AnalyzedPhoto, EventTiers, PhotoOverrides, Tier } from "~/types/features";
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
  /** An events-panel tier menu was used; `SelectPhotos` sets it like the sheet's own header menu. */
  tier: [event: number, tier: Tier | "auto"];
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
const pageItems = computed(() =>
  (recommendation.value?.options ?? []).map((option) => ({
    label:
      option.pages === recommendation.value?.recommendedPages
        ? `${pageOptionLabel(option)} (recommended)`
        : pageOptionLabel(option),
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
/** Every photo, split by where it lands at the chosen length -- see `photoFunnel`. */
const funnel = computed(() =>
  recommendation.value && chosenOption.value ? photoFunnel(photos.length, recommendation.value, chosenOption.value) : null,
);
const count = (n: number) => n.toLocaleString();
/** A segment's width in the funnel bar. Shares are of the whole set, so the unfilled rest is what culling dropped. */
const share = (n: number) => (funnel.value && funnel.value.total > 0 ? `${(n / funnel.value.total) * 100}%` : "0%");

/**
 * The panel's two tabs. Book holds what the book IS (its folders, name,
 * length, size and where every photo went); Events holds how it is split
 * (one row per event, with the tier knobs under it). They were one column,
 * and a user found it "so cramped and confusing" -- eight equal blocks in a
 * 320px strip, with the tier steppers far from the events they size.
 */
const tab = ref<"book" | "events">("book");
const eventCount = computed(() => chosenOption.value?.events.length ?? 0);
const tabItems = computed(() => [
  { label: "Book", value: "book" as const },
  { label: "Events", value: "events" as const, badge: { label: String(eventCount.value), color: "neutral" as const, variant: "subtle" as const } },
]);
/** Book is the only tab when there are no events to list. */
const showing = computed(() => (eventCount.value > 0 ? tab.value : "book"));

/** Why the chosen length cannot be generated, or `null` when it can. */
const overflowMessage = computed(() =>
  chosenOption.value ? includeOverflowLabel(chosenOption.value) : null,
);

/** The tier floors and Includes cannot all fit this length -- `PageOption.tierOverflow`. */
const tierOverflowMessage = computed(() => {
  const option = chosenOption.value;
  const overflow = option?.tierOverflow;
  if (!option || !overflow) return null;
  return `Your tiers and included photos need ${overflow.needed} photos but ${option.pages} pages hold ${overflow.capacity}; every event you haven't skipped still gets at least one.`;
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
  <section class="space-y-6">
    <!--
      The select screen's inspector: the tabs, then the action that commits,
      sticky under both. The draft's folders and the sheet's help are the
      caller's, passed in as slots so they sit in the Book tab with the rest.
    -->
    <template v-if="canGenerate">
      <!--
        Sticky like the action bar, and for the same reason: `-top-5` reaches
        the aside's real edge, since a sticky offset is measured inside the
        scroller's padding. `-mx-5 px-5` paints it across that padding.
      -->
      <div v-if="eventCount > 0" class="sticky -top-5 z-10 -mx-5 bg-default px-5 pt-1 pb-2">
        <UTabs v-model="tab" :items="tabItems" :content="false" color="neutral" class="w-full" />
      </div>

      <div v-show="showing === 'book'" class="space-y-8">
        <slot name="source" />

        <div class="space-y-5">
          <UFormField label="Book name">
            <UInput v-model="name" :disabled="busy" placeholder="Untitled photobook" class="w-full" />
          </UFormField>

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

          <div class="flex items-center justify-between gap-3 text-sm">
            <div class="min-w-0">
              <p class="font-medium text-default">Print size</p>
              <p class="text-muted tabular-nums">{{ shownSpec ? sizeLabel(shownSpec, unit) : "…" }}</p>
            </div>
            <UButton color="neutral" variant="outline" :disabled="busy || !shownSpec" @click="printSizeOpen = true">
              Change…
            </UButton>
          </div>

          <USwitch
            v-model="places"
            label="Split chapters by place"
            :description="
              located === 0 ? 'None of these photos records where it was taken.' : 'A chapter also ends in each new town.'
            "
            color="neutral"
            :disabled="busy || located === 0"
          />
        </div>

        <!--
          What the chosen length costs, stated before the user commits: a page
          length is a purchase decision. One bar of EVERY photo the user has --
          not just the keepers, whose count a user with 1,043 photos read as
          "460, so it lost half my trip" -- split by where each one ends up.
          The heading carries the whole; the key carries the parts, once each.
        -->
        <section v-if="funnel" aria-labelledby="book-funnel" class="space-y-3 text-sm">
          <h3 id="book-funnel" class="flex items-baseline justify-between gap-2 font-semibold text-highlighted">
            Your photos
            <span class="font-normal text-muted tabular-nums">{{ count(funnel.total) }}</span>
          </h3>
          <div class="flex h-3 overflow-hidden rounded-full bg-elevated ring ring-default" aria-hidden="true">
            <div class="bg-inverted" :style="{ width: share(funnel.placed) }" />
            <div class="border-l border-default bg-accented" :style="{ width: share(funnel.noRoom) }" />
          </div>
          <dl class="space-y-2 tabular-nums">
            <div class="flex items-center gap-2">
              <span class="size-3 shrink-0 rounded-sm bg-inverted" />
              <dt class="min-w-0 flex-1 font-medium text-highlighted">In the book</dt>
              <dd class="font-semibold text-highlighted">{{ count(funnel.placed) }}</dd>
            </div>
            <div v-if="funnel.yours > 0" class="flex items-center gap-2 pl-5 text-muted">
              <UIcon name="i-lucide-circle-plus" class="size-4 shrink-0" />
              <dt class="min-w-0 flex-1">You marked +</dt>
              <dd>{{ count(funnel.yours) }}</dd>
            </div>
            <div class="flex items-center gap-2">
              <span class="size-3 shrink-0 rounded-sm bg-accented ring ring-default" />
              <dt class="flex min-w-0 flex-1 items-center gap-1 text-default">
                Left out at {{ chosenOption?.pages }} pages
                <UTooltip text="Good photos this length has no space for. A longer book places more of them.">
                  <UIcon name="i-lucide-info" class="size-4 shrink-0" aria-label="Why photos are left out" />
                </UTooltip>
              </dt>
              <dd class="text-default">{{ count(funnel.noRoom) }}</dd>
            </div>
            <div class="flex items-center gap-2">
              <span class="size-3 shrink-0 rounded-sm bg-elevated ring ring-default" />
              <dt class="flex min-w-0 flex-1 items-center gap-1 text-muted">
                Repeats and screenshots
                <UTooltip text="Screenshots and documents, and the extra shots of a near-identical set: the app keeps the best one.">
                  <UIcon name="i-lucide-info" class="size-4 shrink-0" aria-label="What counts as a repeat" />
                </UTooltip>
              </dt>
              <dd class="text-muted">{{ count(funnel.culled) }}</dd>
            </div>
          </dl>
          <p v-if="risenMessage" class="text-muted">{{ risenMessage }}</p>
        </section>

        <slot name="footer" />
      </div>

      <div v-if="eventCount > 0 && chosenOption" v-show="showing === 'events'" class="space-y-6">
        <EventsPanel
          :rows="chosenOption.events"
          :title="titleOf"
          @reveal="emit('reveal', $event)"
          @tier="(event, tier) => emit('tier', event, tier)"
        />
        <p v-if="tierOverflowMessage" class="text-sm text-muted">{{ tierOverflowMessage }}</p>

        <!-- The tier knobs, beside the tiers they size: rarely touched, so closed until asked for. -->
        <DisclosureSection label="Tier sizes">
          <div class="space-y-3">
            <UFormField label="Featured events get at least">
              <div class="flex items-center gap-2 text-sm text-muted">
                <UInputNumber
                  v-model="featuredFloor"
                  :min="FEATURED_FLOOR_RANGE.min"
                  :max="FEATURED_FLOOR_RANGE.max"
                  :disabled="busy"
                  class="w-28"
                  aria-label="Featured events get at least this many photos"
                />
                <span>photos</span>
              </div>
            </UFormField>
            <UFormField label="Brief events get at most">
              <div class="flex items-center gap-2 text-sm text-muted">
                <UInputNumber
                  v-model="briefCap"
                  :min="BRIEF_CAP_RANGE.min"
                  :max="BRIEF_CAP_RANGE.max"
                  :disabled="busy"
                  class="w-28"
                  aria-label="Brief events get at most this many photos"
                />
                <span>photos</span>
              </div>
            </UFormField>
          </div>
        </DisclosureSection>
      </div>
    </template>
    <template v-else>
      <slot name="source" />
      <slot name="footer" />
    </template>

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
      The book's own update first: re-editing is usually to change it, not fork it.
      Sticky, so the action that commits stays on screen however long the panel
      above it grows; it settles in place once the panel is scrolled to it.
      `-mx-5 px-5` paints the bar across the aside's own padding, and `-bottom-5`
      pins it to the aside's real edge: a sticky offset is measured inside the
      scroller's padding, so `bottom-0` left 20px of scrolling text showing under it.
    -->
    <div
      v-if="canGenerate"
      class="sticky -bottom-5 z-10 -mx-5 flex flex-col items-start gap-2 border-t border-default bg-default px-5 py-3"
    >
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
