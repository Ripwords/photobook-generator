<script setup lang="ts">
import { folderListLabel } from "~/types/book";
import {
  burstSizes,
  groupByEvent,
  keepers,
  overrideFor,
  pickHero,
  showsLeftOutByDefault,
  type AnalyzedPhoto,
  type PhotoOverride,
  type PhotoOverrides,
} from "~/types/features";
import { selectStage, type ReplacedProject } from "~/types/navigation";
import type { LeaveGuard } from "~/composables/useShell";

const { folders, restoreOverrides = {}, replacing = null } = defineProps<{
  /** The folders to analyse, first picked first. Read once, on mount. */
  folders: string[];
  /** Decisions to restore once analysis finishes, when re-editing a saved book. */
  restoreOverrides?: PhotoOverrides;
  /** The saved book this selection came from, and can be generated back over. */
  replacing?: ReplacedProject | null;
}>();

const emit = defineEmits<{
  generated: [projectId: number];
}>();

const {
  analyze,
  summary,
  runId,
  running,
  error,
  folders: analysedFolders,
  scannedTotal,
  processed,
  partialPhotos,
  pickFolderAndAnalyze,
  retry,
} = useAnalysis();

// The analysed set, and the user's own decisions over it.
//
// `photos` is NOT `summary.photos`: it is the same records with `kept`
// re-stamped by Rust after each override. The contact sheet reads Rust's
// verdict and never computes one -- see `usePhotoOverrides` for why that
// round trip exists rather than a two-line filter here.
const analysed = computed<AnalyzedPhoto[]>(() => summary.value?.photos ?? []);
const {
  overrides,
  photos,
  photoSetId,
  error: overrideError,
  setOverride,
  restore,
} = usePhotoOverrides(analysed, runId);

const stage = computed(() => selectStage(running.value, error.value, summary.value));

/** The decisions this screen started with, as a comparable key. */
function decisionKey(decisions: PhotoOverrides): string {
  return JSON.stringify(Object.entries(decisions).toSorted());
}

/**
 * The saved book a generate here would replace, dropped the moment the user
 * picks different folders: a book built from a different source is not an
 * update of the old one.
 */
const replacingNow = ref<ReplacedProject | null>(replacing);
/**
 * The decisions already persisted with that book. Leaving with exactly these
 * loses nothing, so the discard confirmation below measures against this
 * rather than against an empty map.
 */
const savedDecisions = ref(decisionKey(restoreOverrides));
watch(analysedFolders, (next) => {
  if (next.join("\n") === folders.join("\n")) return;
  // Different folders means a different photo set, which resets the override
  // map. Neither the book being replaced nor its saved decisions apply.
  replacingNow.value = null;
  savedDecisions.value = decisionKey({});
});

const kept = computed(() => keepers(photos.value));

/**
 * Whether photos the book will leave out are shown.
 *
 * On by default, and that is a deliberate change: the sheet used to render
 * only the keepers, which made a photo the engine dropped invisible -- and a
 * photo you cannot see is one you cannot ask for. The include control has
 * nothing to act on without this.
 *
 * Off above `LEFT_OUT_SHOWN_BY_DEFAULT_UP_TO`, because the grid is not
 * virtualized: a 500-photo folder would render 500 tiles and re-patch all of
 * them on every toggle. Re-evaluated per analysed SET, not per toggle, so it
 * never fights a choice the user just made.
 */
const showLeftOut = ref(true);
watch(photoSetId, () => {
  showLeftOut.value = showsLeftOutByDefault(analysed.value.length);
});
const visiblePhotos = computed(() => (showLeftOut.value ? photos.value : kept.value));
const eventGroups = computed(() => groupByEvent(visiblePhotos.value));
const burstMap = computed<Map<number, number>>(() => burstSizes(photos.value));
// One hero per event group - the outline and star mark exactly this
// photo, so it stays meaningful instead of becoming decoration. Chosen from
// the KEPT photos only: a hero the book does not contain is not a hero.
const heroPaths = computed<Set<string>>(
  () =>
    new Set(
      groupByEvent(kept.value)
        .map((group) => pickHero(group.photos)?.path)
        .filter((path): path is string => path !== undefined),
    ),
);
const leftOutCount = computed(() => photos.value.length - kept.value.length);

function onSetOverride(photo: AnalyzedPhoto, decision: PhotoOverride) {
  void setOverride(photo.hash, decision);
}

const folderLabel = computed(() => folderListLabel(analysedFolders.value));
const folderTitle = computed(() => analysedFolders.value.join("\n"));

/** The contact sheet's smallest tile width, in CSS pixels. The slider in its toolbar sets it. */
const tileSize = ref(160);
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

/**
 * Decisions made here that no photobook holds yet. They live only in webview
 * state until `generate_book` persists them, so leaving throws exactly these
 * away -- which is the only thing on this screen worth a confirmation, since
 * the analysis itself is cached.
 *
 * Measured against what the screen was opened with, not against nothing: a
 * book reopened through "Edit photos" arrives carrying its own saved
 * decisions, and warning that those are about to be lost would be a lie.
 */
const hasUnsavedDecisions = computed(
  () => decisionKey(overrides.value) !== savedDecisions.value,
);
const confirmLeave = ref(false);

/**
 * Leaving is no longer only this screen's own back button: the sidebar can
 * open the library or another book from anywhere. So the shell asks this
 * guard first, and it holds the navigation until the user says to discard.
 */
const { leaveGuard } = useShell();
let proceedAfterDiscard: (() => void) | null = null;
const guard: LeaveGuard = (proceed) => {
  if (!hasUnsavedDecisions.value) {
    proceed();
    return;
  }
  proceedAfterDiscard = proceed;
  confirmLeave.value = true;
};
leaveGuard.value = guard;
onBeforeUnmount(() => {
  // Only our own: the next screen may already have put up its guard.
  if (leaveGuard.value === guard) leaveGuard.value = null;
});

function discardAndLeave() {
  confirmLeave.value = false;
  const proceed = proceedAfterDiscard;
  proceedAfterDiscard = null;
  proceed?.();
}

onMounted(async () => {
  /*
   * Order matters: `analyze` replaces the photo set, and that resets the
   * override map by design (a hash-keyed decision must not survive into a
   * different folder). `restore` therefore runs after it has resolved, never
   * before.
   */
  await analyze(folders);
  if (Object.keys(restoreOverrides).length > 0) await restore(restoreOverrides);
});
</script>

<template>
  <AppHeader
    :title="replacingNow ? `Editing photos for “${replacingNow.name}”` : 'New photobook'"
    :subtitle="folderLabel"
    :subtitle-title="folderTitle"
  >
    <UTooltip text="Analyse other folders instead">
      <UButton
        icon="i-lucide-folder-open"
        color="neutral"
        variant="outline"
        size="sm"
        :loading="running"
        :disabled="running"
        @click="pickFolderAndAnalyze"
      >
        Choose different folders
      </UButton>
    </UTooltip>
  </AppHeader>

  <div class="flex min-h-0 flex-1">
    <main class="min-w-0 flex-1 overflow-y-auto">
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
        <div :class="gridClass" :style="gridStyle">
          <PhotoTile v-for="photo in partialPhotos" :key="photo.path" :photo="photo" />
          <USkeleton
            v-for="n in remainingSkeletonCount"
            :key="`pending-${n}`"
            class="aspect-square w-full"
            aria-hidden="true"
          />
        </div>
      </div>

      <UEmpty
        v-else-if="stage === 'error'"
        icon="i-lucide-triangle-alert"
        title="Analysis failed"
        :description="error ?? undefined"
        :ui="{ description: 'break-words' }"
        :actions="[
          { label: 'Try again', icon: 'i-lucide-rotate-ccw', color: 'primary', onClick: retry },
          {
            label: 'Choose different folders',
            icon: 'i-lucide-folder-open',
            color: 'neutral',
            variant: 'outline',
            onClick: pickFolderAndAnalyze,
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
            onClick: pickFolderAndAnalyze,
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
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <template v-else-if="stage === 'ready' && summary">
        <!-- The sheet's own toolbar, pinned while the photos scroll under it. -->
        <div
          class="sticky top-0 z-20 flex h-11 items-center gap-4 border-b border-default bg-default/95 px-6 backdrop-blur"
        >
          <UFieldGroup v-if="leftOutCount > 0 || !showLeftOut" size="xs">
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
              Keepers only
            </UButton>
          </UFieldGroup>
          <p class="text-xs text-muted tabular-nums">
            <span class="font-medium text-highlighted">{{ kept.length }}</span> keepers,
            {{ leftOutCount }} left out
          </p>
          <div class="ml-auto flex items-center gap-2">
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
                onClick: pickFolderAndAnalyze,
              },
            ]"
            class="mx-auto max-w-lg"
          />

          <section v-for="(group, index) in eventGroups" :key="group.eventCluster" class="space-y-3">
            <h2
              class="sticky top-11 z-10 -mx-6 flex items-baseline gap-2 bg-default/95 px-6 py-2 text-sm font-semibold text-highlighted backdrop-blur"
            >
              Event {{ index + 1 }}
              <span class="font-normal text-muted tabular-nums"
                >{{ group.photos.length }}
                {{ group.photos.length === 1 ? "photo" : "photos" }}</span
              >
            </h2>
            <div :class="gridClass" :style="gridStyle">
              <PhotoTile
                v-for="photo in group.photos"
                :key="photo.path"
                :photo="photo"
                :burst-size="burstMap.get(photo.nearDupCluster) ?? 1"
                :is-hero="heroPaths.has(photo.path)"
                :is-kept="photo.kept"
                :override="overrideFor(overrides, photo.hash)"
                @set-override="onSetOverride(photo, $event)"
              />
            </div>
          </section>
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
      v-if="stage === 'ready' && summary && analysedFolders.length > 0"
      aria-label="Book settings"
      class="w-80 shrink-0 space-y-8 overflow-y-auto border-l border-default bg-default p-5"
    >
      <GenerateBook
        :photos
        :overrides
        :photo-set-id="photoSetId"
        :run-id="runId"
        :folders="analysedFolders"
        :replacing="replacingNow"
        @generated="emit('generated', $event)"
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
    v-model:open="confirmLeave"
    title="Discard this selection?"
    :ui="{ footer: 'justify-end' }"
  >
    <template #body>
      <div class="space-y-3 text-sm">
        <p class="text-default">
          The photos you included and excluded here have not been saved to a photobook yet, and
          leaving throws those choices away.
        </p>
        <p class="text-muted">
          The photo analysis itself is cached, so coming back to these folders is quick and costs
          no new Vision work.
        </p>
      </div>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="confirmLeave = false">
        Keep editing
      </UButton>
      <UButton color="error" @click="discardAndLeave">Discard</UButton>
    </template>
  </UModal>
</template>
