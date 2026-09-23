<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import {
  eventRowNote,
  folderListLabel,
  isPlaced,
  placedCount,
  type EventRow,
  type PageOption,
  type PlaceChapters,
  type PlaceNames,
} from "~/types/book";
import {
  burstSizes,
  eventHashes,
  eventTitle,
  groupByEvent,
  keepers,
  overrideFor,
  pickHero,
  withEventTier,
  type AnalyzedPhoto,
  type PhotoOverride,
  type PhotoOverrides,
  type Tier,
} from "~/types/features";
import { selectStage } from "~/types/navigation";
import { jobProgress, jobRunId, pickFolders, type AnalysisJob } from "~/composables/useAnalysisJobs";

/**
 * A draft's contact sheet. It reads and writes the draft's job and owns
 * nothing that leaving would lose: the analysis keeps streaming into the job
 * while this screen is gone, and the decisions stay on it.
 */
const { job } = defineProps<{
  job: AnalysisJob;
}>();

const emit = defineEmits<{
  generated: [projectId: number];
  discarded: [];
}>();

const { changeFolders, retry: retryJob, rename, setSpec, setOptions, remove } = useAnalysisJobs();

const summary = computed(() => job.stream.summary);
const runId = computed(() => jobRunId(job));
const scannedTotal = computed(() => job.stream.scannedTotal);
const processed = computed(() => jobProgress(job).processed);
/** The photos streamed so far, as the one group the sheet shows while analysing. */
const streamedGroups = computed(() => [{ eventCluster: 0, photos: job.stream.partialPhotos }]);

async function chooseOtherFolders() {
  const folders = await pickFolders();
  if (folders.length > 0) changeFolders(job.id, folders);
}

function retry() {
  retryJob(job.id);
}

// The analysed set, and the user's own decisions over it.
//
// `photos` is NOT `summary.photos`: it is the same records with `kept`
// re-stamped by Rust after each override. The contact sheet reads Rust's
// verdict and never computes one -- see `usePhotoOverrides` for why that
// round trip exists rather than a two-line filter here.
const analysed = computed<AnalyzedPhoto[]>(() => summary.value?.photos ?? []);
// Written straight onto the job, which is store state rather than this
// screen's: that is what lets the decisions outlive it.
const decisions = computed({
  get: () => job.overrides,
  set: (next: PhotoOverrides) => {
    job.overrides = next;
  },
});
/** The job's tier choices, restored on reopen -- no control writes these yet. */
const tiers = computed(() => job.tiers);
/** The option the user has chosen a length for, so the sheet can dim by what it actually places. */
const chosenOption = ref<PageOption>();
const {
  overrides,
  photos,
  photoSetId,
  error: overrideError,
  setOverride,
} = usePhotoOverrides(analysed, runId, decisions);

const stage = computed(() => selectStage(job.running, job.error, summary.value));

const kept = computed(() => keepers(photos.value));
/**
 * Photos the chosen length actually places -- the same membership `isPlaced`
 * checks per tile. Before a recommendation exists this is the cull verdict,
 * same as `kept`; once one exists it is what "In the book" filters to, so
 * the toggle never disagrees with the dimming.
 */
const placed = computed(() => photos.value.filter((photo) => isPlaced(photo, chosenOption.value)));

/**
 * Whether photos the book will leave out are shown. On for every new set of
 * photos: a photo you cannot see is one you cannot ask for, and the include
 * control has nothing to act on without it. Reset per analysed SET, not per
 * toggle, so it never fights a choice the user just made.
 */
const showLeftOut = ref(true);
watch(photoSetId, () => {
  showLeftOut.value = true;
});
const visiblePhotos = computed(() => (showLeftOut.value ? photos.value : placed.value));

/** The analysed run's place chapters, fetched once per run whatever the switch says, so the switch knows whether it can do anything. */
const placeChapters = ref<PlaceChapters | null>(null);
watch(
  runId,
  async (run) => {
    placeChapters.value = null;
    if (run === 0) return;
    try {
      const next = await invoke<PlaceChapters>("place_chapters", { runId: run });
      if (runId.value === run) placeChapters.value = next;
    } catch (e) {
      console.warn("could not read the place chapters", e);
    }
  },
  { immediate: true },
);
const chapterOverride = computed(() => (job.options.places ? (placeChapters.value?.chapters ?? null) : null));
const placeNames = usePlaceNames(runId, toRef(() => job.options.places));

const eventGroups = computed(() => groupByEvent(visiblePhotos.value, chapterOverride.value));

/**
 * Every one of the job's events, grouped once from ALL of `photos` -- never
 * `eventGroups` above, which is the FILTERED list "In the book" thins.
 * `titleOf` and `eventTitles` both read this instead of re-grouping per
 * lookup, so titling every header in the sheet costs one scan of the
 * photos, not one per event.
 */
const allEventGroups = computed(() => groupByEvent(photos.value, chapterOverride.value));

/**
 * The place name when Places is on, else "Event N" numbered among ALL of the
 * job's events, from `allEventGroups` above -- never `eventGroups`, the
 * FILTERED list "In the book" thins. Numbering from a filtered list shifted
 * the number every time the toggle changed and could show "Event 0" for a
 * hidden event; see `eventTitle`. `EventTierControl` and `EventsPanel` call
 * this directly; the sheet's own header title comes from `eventTitles`
 * below, a full map built by calling this once per event, so all three
 * agree -- the sheet's `names` prop used to be the sparse `placeNames`,
 * which left its header numbering the filtered list whenever a chapter had
 * no place name.
 */
function titleOf(event: number): string {
  return eventTitle(allEventGroups.value, placeNames.value, event);
}

/** Every event's title, keyed by cluster id -- see `titleOf`. Passed to `ContactSheet` as `:names` so its own header agrees with `titleOf`'s other callers. */
const eventTitles = computed<PlaceNames>(() => {
  const names: Record<number, string> = {};
  for (const group of allEventGroups.value) names[group.eventCluster] = titleOf(group.eventCluster);
  return names;
});

/**
 * Sets (or clears, for "auto") every photo of one event to a tier, written
 * straight onto the job like `decisions` is. Reads hashes from ALL of the
 * job's photos via `eventHashes`, never `eventGroups` (the FILTERED list) --
 * the engine resolves a tier over an event's whole photo set
 * (`book::events::resolve`), so tiering a filtered subset left photos a
 * filter hid stuck on a stale tier, or made a Skipped event vanish out from
 * under its own control.
 */
function setEventTier(event: number, tier: Tier | "auto") {
  job.tiers = withEventTier(job.tiers, eventHashes(photos.value, chapterOverride.value, event), tier);
}

/** The event's plan at the chosen length, or `undefined` before one exists. */
function eventRowFor(event: number): EventRow | undefined {
  return chosenOption.value?.events.find((row) => row.event === event);
}

/**
 * A short, one-line reason shown right in a Skipped event's header (spec
 * §7: "dimmed with the reason"), not only on the tier control's hover --
 * `null` for every other tier, where the header stays just the title and
 * count.
 */
function skipNoteFor(event: number): string | null {
  const row = eventRowFor(event);
  return row && row.tier === "skipped" ? eventRowNote(row, titleOf) : null;
}

/** Places on, but the chapters they'd split by haven't loaded yet -- nothing to tier against. */
const tierControlReady = computed(() => !(job.options.places && !placeChapters.value));

const contactSheet = useTemplateRef("contactSheet");
/** An events-panel row was clicked: scroll the sheet to that event's header. */
function revealEvent(event: number) {
  contactSheet.value?.revealEvent(event);
}
const burstMap = computed<Map<number, number>>(() => burstSizes(photos.value));
// One hero per event group - the outline and star mark exactly this
// photo, so it stays meaningful instead of becoming decoration. Chosen from
// the KEPT photos only: a hero the book does not contain is not a hero.
const heroPaths = computed<Set<string>>(
  () =>
    new Set(
      groupByEvent(kept.value, chapterOverride.value)
        .map((group) => pickHero(group.photos)?.path)
        .filter((path): path is string => path !== undefined),
    ),
);
/** What the chosen length actually places, out of every analysed photo -- drives the toolbar count so it agrees with the tiles' dimming. */
const placedTotal = computed(() => placedCount(photos.value, chosenOption.value));
const leftOutCount = computed(() => photos.value.length - placedTotal.value);

function onSetOverride(photo: AnalyzedPhoto, decision: PhotoOverride) {
  void setOverride(photo.hash, decision);
}

const folderLabel = computed(() => folderListLabel(job.folders));
const folderTitle = computed(() => job.folders.join("\n"));

/** The contact sheet's smallest tile width, in CSS pixels. The slider in its toolbar sets it. */
const tileSize = ref(160);
const scroller = useTemplateRef("scroller");
const gridClass = "grid gap-3";
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(auto-fill, minmax(${tileSize.value}px, 1fr))`,
}));

// Determinate once the folder has been scanned (the `Scanned` event gives a
// denominator); indeterminate for the brief window before it arrives.
// `UProgress` treats a `null` model value as indeterminate.
const progressValue = computed(() => (scannedTotal.value > 0 ? processed.value : null));

// Placeholder tiles for photos not yet streamed in, so the grid still reads
// as "growing toward a known total" rather than just stopping short. Capped
// so a folder of thousands doesn't render thousands of empty skeleton
// nodes -- it is a "more coming" indicator, not a literal one-per-photo count.
const remainingSkeletonCount = computed(() =>
  Math.min(Math.max(scannedTotal.value - processed.value, 0), 48),
);

const confirmDiscard = ref(false);

function discard() {
  confirmDiscard.value = false;
  emit("discarded");
  remove(job.id);
}

function onGenerated(projectId: number) {
  emit("generated", projectId);
  remove(job.id);
}
</script>

<template>
  <AppHeader
    :title="job.replacing ? `Editing photos for “${job.replacing.name}”` : job.name"
    :subtitle="folderLabel"
    :subtitle-title="folderTitle"
  >
    <UTooltip text="Analyse other folders instead">
      <UButton
        icon="i-lucide-folder-open"
        color="neutral"
        variant="outline"
        size="sm"
        :loading="job.running"
        :disabled="job.running"
        @click="chooseOtherFolders"
      >
        Choose different folders
      </UButton>
    </UTooltip>
    <UTooltip text="Throw this draft away">
      <UButton
        icon="i-lucide-trash-2"
        color="neutral"
        variant="ghost"
        size="sm"
        aria-label="Discard draft"
        @click="confirmDiscard = true"
      />
    </UTooltip>
  </AppHeader>

  <div class="flex min-h-0 flex-1">
    <main ref="scroller" class="min-w-0 flex-1 overflow-y-auto">
      <div v-if="stage === 'running'" class="space-y-6 p-6">
        <div class="max-w-sm space-y-2">
          <UProgress color="primary" size="sm" :model-value="progressValue" :max="scannedTotal" />
          <p class="text-sm text-muted tabular-nums">
            <template v-if="scannedTotal > 0">
              <span class="text-default">{{ processed }}</span> of
              <span class="text-default">{{ scannedTotal }}</span>
              analyzed in <span class="text-default">{{ folderLabel }}</span>
            </template>
            <template v-else>
              Scanning <span class="text-default">{{ folderLabel }}</span
              >&hellip;
            </template>
          </p>
        </div>
        <div>
          <ContactSheet
            :groups="streamedGroups"
            :headers="false"
            :tile-size
            :scroll-element="scroller"
          >
            <template #tile="{ photo }">
              <PhotoTile :photo />
            </template>
          </ContactSheet>
          <div v-if="remainingSkeletonCount > 0" :class="gridClass" class="mt-3" :style="gridStyle">
            <USkeleton
              v-for="n in remainingSkeletonCount"
              :key="`pending-${n}`"
              class="aspect-square w-full"
              aria-hidden="true"
            />
          </div>
        </div>
      </div>

      <UEmpty
        v-else-if="stage === 'error'"
        icon="i-lucide-triangle-alert"
        title="Analysis failed"
        :description="job.error ?? undefined"
        :ui="{ description: 'break-words' }"
        :actions="[
          { label: 'Try again', icon: 'i-lucide-rotate-ccw', color: 'primary', onClick: retry },
          {
            label: 'Choose different folders',
            icon: 'i-lucide-folder-open',
            color: 'neutral',
            variant: 'outline',
            onClick: chooseOtherFolders,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="stage === 'no-images'"
        icon="i-lucide-image-off"
        title="No photos found"
        description="The folders you chose do not contain any supported image files. Choose different folders to continue."
        :actions="[
          {
            label: 'Choose different folders',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: chooseOtherFolders,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="stage === 'no-analyzed' && summary"
        icon="i-lucide-triangle-alert"
        title="None of the photos in this folder could be analyzed"
        :description="`${summary.failed} of ${summary.total} files failed. They may be corrupted or in an unsupported format.`"
        :actions="[
          {
            label: 'Choose different folders',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: chooseOtherFolders,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <template v-else-if="stage === 'ready' && summary">
        <!-- The sheet's own toolbar, pinned while the photos scroll under it. -->
        <div
          class="@container sticky top-0 z-20 flex h-11 items-center gap-3 border-b border-default bg-default/95 px-6 backdrop-blur"
        >
          <UFieldGroup v-if="leftOutCount > 0 || !showLeftOut" size="xs" class="shrink-0">
            <UButton
              color="neutral"
              :variant="showLeftOut ? 'solid' : 'outline'"
              :aria-pressed="showLeftOut"
              @click="showLeftOut = true"
            >
              All photos
            </UButton>
            <UButton
              color="neutral"
              :variant="showLeftOut ? 'outline' : 'solid'"
              :aria-pressed="!showLeftOut"
              @click="showLeftOut = false"
            >
              In the book
            </UButton>
          </UFieldGroup>
          <p class="min-w-0 truncate text-xs text-muted tabular-nums">
            <span class="font-medium text-highlighted">{{ placedTotal }}</span> of {{ photos.length }} in the book
          </p>
          <!--
            Measured, not guessed: the toggle is 164px, the slider group 162px
            and the two gaps 24px, so everything fits once the row's content box
            reaches ~26rem, with the counts giving way first. Below that the
            slider stands down rather than being clipped. The narrowest real
            surface is this sheet inside the editor's Edit photos panel, which
            is 477px at the window's own minWidth of 1100, so the slider is
            there at every size the app can reach.
          -->
          <div class="ml-auto hidden shrink-0 items-center gap-2 @min-[26rem]:flex">
            <UIcon name="i-lucide-image" class="size-3.5 text-muted" />
            <USlider
              v-model="tileSize"
              :min="110"
              :max="300"
              :step="10"
              size="xs"
              color="neutral"
              class="w-28"
              aria-label="Photo size"
            />
            <UIcon name="i-lucide-image" class="size-5 text-muted" />
          </div>
        </div>

        <div class="space-y-8 p-6">
          <UAlert
            v-if="overrideError"
            color="error"
            variant="subtle"
            icon="i-lucide-triangle-alert"
            title="Could not re-check the selection"
            :description="overrideError"
          />

          <UEmpty
            v-if="eventGroups.length === 0"
            icon="i-lucide-image-off"
            title="No photos were kept"
            description="Every analyzed photo was flagged as a screenshot, document, or similar non-photo image, or was excluded by you."
            :actions="[
              {
                label: 'Choose different folders',
                icon: 'i-lucide-folder-open',
                color: 'primary',
                onClick: chooseOtherFolders,
              },
            ]"
            class="mx-auto max-w-lg"
          />

          <ContactSheet
            v-else
            ref="contactSheet"
            :groups="eventGroups"
            :names="eventTitles"
            :tile-size
            :scroll-element="scroller"
            :sticky-top="44"
          >
            <template #tile="{ photo }">
              <PhotoTile
                :photo
                :burst-size="burstMap.get(photo.nearDupCluster) ?? 1"
                :is-hero="heroPaths.has(photo.path)"
                :is-kept="isPlaced(photo, chosenOption)"
                :override="overrideFor(overrides, photo.hash)"
                @set-override="onSetOverride(photo, $event)"
              />
            </template>
            <template #header="{ event }">
              <!--
                Two root nodes, not one wrapping div: each takes its own
                column of ContactSheet's header grid. The note goes in column
                2, which gets only the space the title (column 1) leaves, so
                the note truncates to nothing before the title loses a pixel.
                The control goes in column 3, which is never squeezed.
              -->
              <span
                v-if="skipNoteFor(event)"
                class="col-start-2 ml-2 min-w-0 truncate text-xs font-normal text-muted"
              >
                {{ skipNoteFor(event) }}
              </span>
              <span v-if="tierControlReady" class="col-start-3 ml-2">
                <EventTierControl
                  :row="eventRowFor(event)"
                  :title="titleOf(event)"
                  :title-of="titleOf"
                  @set="setEventTier(event, $event)"
                />
              </span>
            </template>
          </ContactSheet>
        </div>
      </template>
    </main>

    <!--
      Pick a length and generate, which SAVES the book. Operates on the fully
      ranked set -- `photos`, which is `summary.photos` with `kept` re-stamped
      by Rust for the user's own overrides. `overrides` rides along so
      `generate_book` can persist the decisions with the project. In its own
      column, so the primary action is never below a 200-tile grid, however
      far down the user has scrolled choosing photos.
    -->
    <aside
      v-if="stage === 'ready' && summary"
      aria-label="Book settings"
      class="w-80 shrink-0 space-y-8 overflow-y-auto border-l border-default bg-default p-5"
    >
      <GenerateBook
        :photos
        :overrides
        :tiers
        :photo-set-id="photoSetId"
        :run-id="runId"
        :folders="job.folders"
        :replacing="job.replacing"
        :name="job.name"
        @update:name="rename(job.id, $event)"
        :spec="job.spec"
        @update:spec="setSpec(job.id, $event)"
        :options="job.options"
        @update:options="setOptions(job.id, $event)"
        :located="placeChapters?.located ?? null"
        :title-of="titleOf"
        @generated="onGenerated"
        @option="chosenOption = $event"
        @reveal="revealEvent"
      />

      <section class="space-y-2 border-t border-default pt-6" aria-labelledby="select-stats">
        <h2 id="select-stats" class="text-sm font-medium text-highlighted">Selection</h2>
        <dl class="grid grid-cols-2 gap-x-4 gap-y-1.5 text-sm tabular-nums">
          <dt class="text-muted">Keepers</dt>
          <dd class="text-right text-highlighted">{{ kept.length }}</dd>
          <dt class="text-muted">Analyzed</dt>
          <dd class="text-right">{{ summary.photos.length }}</dd>
          <dt class="text-muted">Scanned</dt>
          <dd class="text-right">{{ summary.total }}</dd>
          <template v-if="summary.failed > 0">
            <dt class="text-muted">Failed</dt>
            <dd class="text-right">{{ summary.failed }}</dd>
          </template>
          <template v-if="summary.cached > 0">
            <dt class="text-muted">From cache</dt>
            <dd class="text-right">{{ summary.cached }}</dd>
          </template>
        </dl>
      </section>

      <section class="space-y-2 border-t border-default pt-6" aria-labelledby="select-legend">
        <h2 id="select-legend" class="text-sm font-medium text-highlighted">Reading the sheet</h2>
        <ul class="space-y-2 text-xs text-muted">
          <li class="flex gap-2">
            <UIcon name="i-lucide-star" class="mt-px size-3.5 shrink-0 text-highlighted" />
            The best photo of each event, outlined.
          </li>
          <li class="flex gap-2">
            <UIcon name="i-lucide-circle-plus" class="mt-px size-3.5 shrink-0 text-highlighted" />
            Use + and &minus; on a photo to override what the engine chose.
          </li>
          <li class="flex gap-2">
            <UIcon name="i-lucide-images" class="mt-px size-3.5 shrink-0 text-highlighted" />
            The best of a burst of near-duplicate frames.
          </li>
          <li class="flex gap-2">
            <UIcon name="i-lucide-sparkles" class="mt-px size-3.5 shrink-0 text-highlighted" />
            Percentiles are ranked across everything you chose: sparkle is aesthetic, focus is
            sharpness.
          </li>
        </ul>
      </section>
    </aside>
  </div>

  <UModal
    v-model:open="confirmDiscard"
    :title="`Discard “${job.name}”?`"
    :ui="{ footer: 'justify-end' }"
  >
    <template #body>
      <div class="space-y-3 text-sm">
        <p class="text-default">
          This draft is not a photobook yet. Discarding it throws away its name and the photos
          you included and excluded.
        </p>
        <p class="text-muted">
          The photo analysis itself is cached, so starting again from these folders is quick and
          costs no new Vision work.
        </p>
      </div>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="confirmDiscard = false">Cancel</UButton>
      <UButton color="error" @click="discard">Discard draft</UButton>
    </template>
  </UModal>
</template>
