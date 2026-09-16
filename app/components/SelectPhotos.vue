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

const { folders, restoreOverrides = {}, replacing = null } = defineProps<{
  /** The folders to analyse, first picked first. Read once, on mount. */
  folders: string[];
  /** Decisions to restore once analysis finishes, when re-editing a saved book. */
  restoreOverrides?: PhotoOverrides;
  /** The saved book this selection came from, and can be generated back over. */
  replacing?: ReplacedProject | null;
}>();

const emit = defineEmits<{
  close: [];
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
// One hero per event group - the pastel-yellow accent marks exactly this
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

const gridClass = "grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-3";

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

function requestClose() {
  if (hasUnsavedDecisions.value) confirmLeave.value = true;
  else emit("close");
}

function discardAndClose() {
  confirmLeave.value = false;
  emit("close");
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
    back="All photobooks"
    @back="requestClose"
  >
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
  </AppHeader>

  <main class="flex-1 overflow-y-auto p-6">
    <div v-if="stage === 'running'" class="space-y-6">
      <div class="max-w-sm space-y-2">
        <UProgress color="primary" size="sm" :model-value="progressValue" :max="scannedTotal" />
        <p class="text-sm text-muted">
          <template v-if="scannedTotal > 0">
            <span class="font-mono tabular-nums text-default">{{ processed }}</span> /
            <span class="font-mono tabular-nums text-default">{{ scannedTotal }}</span>
            processed in <span class="text-default">{{ folderLabel }}</span>
          </template>
          <template v-else>
            Scanning <span class="text-default">{{ folderLabel }}</span
            >&hellip;
          </template>
        </p>
      </div>
      <div :class="gridClass">
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

    <div v-else-if="stage === 'ready' && summary" class="space-y-8">
      <div class="space-y-1">
        <div class="flex flex-wrap items-center gap-x-6 gap-y-1 text-sm text-muted">
          <span
            ><span class="font-mono tabular-nums text-default">{{ summary.total }}</span>
            scanned</span
          >
          <span
            ><span class="font-mono tabular-nums text-default">{{ summary.photos.length }}</span>
            analyzed</span
          >
          <span
            ><span class="font-mono tabular-nums text-default">{{ summary.cached }}</span> from
            cache</span
          >
          <span
            ><span class="font-mono tabular-nums text-default">{{ summary.failed }}</span>
            failed</span
          >
          <span class="font-medium text-highlighted"
            ><span class="font-mono tabular-nums">{{ kept.length }}</span> keepers</span
          >
          <USwitch
            v-if="leftOutCount > 0 || !showLeftOut"
            v-model="showLeftOut"
            size="sm"
            :label="`Show the ${leftOutCount} left out`"
          />
        </div>
        <p class="text-xs text-muted">
          Percentiles are ranked across everything you chose. Sparkle is aesthetic, focus is
          sharpness.
          Use + and &minus; on a photo to override what the engine chose.
        </p>
        <UAlert
          v-if="overrideError"
          color="error"
          variant="subtle"
          icon="i-lucide-triangle-alert"
          title="Could not re-check the selection"
          :description="overrideError"
          class="mt-2"
        />
      </div>

      <!--
        Pick a length and generate, which SAVES the book. Operates on the fully
        ranked set -- `photos`, which is `summary.photos` with `kept` re-stamped
        by Rust for the user's own overrides. `overrides` rides along so
        `generate_book` can persist the decisions with the project. Above the
        contact sheet, so the primary action is not below a 200-tile grid.
      -->
      <div
        v-if="analysedFolders.length > 0"
        class="rounded-lg border border-default p-4"
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
      </div>

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

      <section
        v-for="(group, index) in eventGroups"
        :key="group.eventCluster"
        class="space-y-3 border-t border-lavender-600 pt-6 first:border-t-0 first:pt-0 dark:border-lavender-300"
      >
        <h2 class="text-sm font-medium text-toned">
          Event {{ index + 1 }}
          <span class="text-muted"
            >&middot; {{ group.photos.length }}
            {{ group.photos.length === 1 ? "photo" : "photos" }}</span
          >
        </h2>
        <div :class="gridClass">
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
  </main>

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
      <UButton color="error" @click="discardAndClose">Discard</UButton>
    </template>
  </UModal>
</template>
