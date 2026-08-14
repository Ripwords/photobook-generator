<script setup lang="ts">
import { lastExportedOn } from "~/types/book";
import {
  basename,
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

const {
  analyze,
  summary,
  running,
  error,
  folder,
  scannedTotal,
  processed,
  partialPhotos,
  pickFolderAndAnalyze,
  retry,
} = useAnalysis();

// Read once here so the home page can offer a saved project without forcing
// a folder pick first -- the whole point of persistence is skipping a
// re-analysis of a folder that was already scanned.
const {
  projects,
  busy: projectsBusy,
  error: projectsError,
  refresh: refreshProjects,
  deleteProject,
  renameProject,
} = useProjects();
onMounted(() => {
  void refreshProjects();
});

/** Which saved project the user picked from the list below, or `null` if none. */
const selectedProjectId = ref<number | null>(null);

// A fresh analysis supersedes whatever was selected: without this, finishing
// analysis after picking a folder from the "entry" screen (while a project
// was still selected) would keep showing that project's panel instead of the
// newly analysed folder's results.
watch(folder, () => {
  selectedProjectId.value = null;
});

type ViewState = "entry" | "running" | "error" | "no-images" | "no-analyzed" | "results" | "project";

const state = computed<ViewState>(() => {
  if (running.value) return "running";
  if (error.value) return "error";
  if (selectedProjectId.value !== null) return "project";
  if (!summary.value) return "entry";
  if (summary.value.total === 0) return "no-images";
  if (summary.value.photos.length === 0) return "no-analyzed";
  return "results";
});

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
} = usePhotoOverrides(analysed);

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

/**
 * Re-analyse a reopened project's own folder and restore the decisions it was
 * saved with, so they become visible and editable on the contact sheet.
 *
 * Order matters: `analyze` replaces the photo set, and that resets the
 * override map by design (a hash-keyed decision must not survive into a
 * different folder). `restore` therefore runs after it has resolved, never
 * before.
 */
async function onEditSelection(payload: { sourceFolder: string; overrides: PhotoOverrides }) {
  selectedProjectId.value = null;
  await analyze(payload.sourceFolder);
  await restore(payload.overrides);
}
const folderLabel = computed(() => (folder.value ? basename(folder.value) : "the selected folder"));

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
</script>

<template>
  <div class="flex h-screen flex-col bg-default text-default">
    <header
      class="flex shrink-0 items-center justify-between gap-4 border-b border-default px-6 py-4"
    >
      <div class="flex items-center gap-2.5">
        <UIcon name="i-lucide-images" class="size-5 text-primary" />
        <h1 class="text-sm font-semibold text-highlighted">PhotobookGen</h1>
      </div>
      <div class="flex items-center gap-2">
        <UButton
          v-if="state !== 'entry'"
          icon="i-lucide-folder-open"
          color="neutral"
          variant="outline"
          size="sm"
          :loading="running"
          :disabled="running"
          @click="pickFolderAndAnalyze"
        >
          Choose a different folder
        </UButton>
        <UColorModeButton size="sm" />
      </div>
    </header>

    <main class="flex-1 overflow-y-auto p-6">
      <div v-if="state === 'entry'" class="mx-auto mt-16 max-w-lg space-y-8">
        <!--
          Saved projects, offered ahead of the folder picker: analysing a
          folder runs Apple Vision over every photo in it, so reopening one
          already analysed in a past session must not force that work again.
        -->
        <div v-if="projects.length > 0" class="space-y-3">
          <h2 class="text-sm font-medium text-highlighted">Saved photobooks</h2>
          <UAlert
            v-if="projectsError"
            color="error"
            variant="subtle"
            icon="i-lucide-triangle-alert"
            title="Something went wrong"
            :description="projectsError"
            :ui="{ description: 'break-words' }"
          />
          <ul class="space-y-2">
            <ProjectListRow
              v-for="project in projects"
              :key="project.id"
              :project="project"
              :busy="projectsBusy"
              @open="selectedProjectId = $event"
              @rename="renameProject"
              @delete="deleteProject"
            >
              <template #meta>
                <span class="font-mono tabular-nums">{{ project.pageCount }}</span> pages ·
                {{ basename(project.sourceFolder) }} ·
                <template v-if="lastExportedOn(project)"
                  >exported {{ lastExportedOn(project) }}</template
                >
                <template v-else>not exported yet</template>
              </template>
            </ProjectListRow>
          </ul>
        </div>

        <UEmpty
          icon="i-lucide-images"
          title="Choose a photo folder to begin"
          description="PhotobookGen analyzes every photo in a folder on this Mac: sharpness, faces, color palette, and Apple's aesthetic model. It groups burst shots and events, then ranks each photo against the rest of the folder so you can see what's worth printing."
          :actions="[
            {
              label: 'Choose photo folder',
              icon: 'i-lucide-folder-open',
              color: 'primary',
              loading: running,
              onClick: pickFolderAndAnalyze,
            },
          ]"
        />
      </div>

      <section v-else-if="state === 'project'" class="mx-auto max-w-3xl">
        <!--
          Keyed by the project id: `openProjectId` is only read once, in
          `GenerateBook`'s `onMounted`, so a `key` guarantees a fresh
          component instance (and therefore a fresh `useBook`) if this ever
          becomes reachable for a second project without an unmount in
          between, rather than silently keeping the first project on screen.
        -->
        <GenerateBook
          :key="selectedProjectId"
          :open-project-id="selectedProjectId"
          @edit-selection="onEditSelection"
        />
      </section>

      <div v-else-if="state === 'running'" class="space-y-6">
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
        v-else-if="state === 'error'"
        icon="i-lucide-triangle-alert"
        title="Analysis failed"
        :description="error ?? undefined"
        :ui="{ description: 'break-words' }"
        :actions="[
          { label: 'Try again', icon: 'i-lucide-rotate-ccw', color: 'primary', onClick: retry },
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'neutral',
            variant: 'outline',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="state === 'no-images'"
        icon="i-lucide-image-off"
        title="No photos found"
        description="This folder does not contain any supported image files. Choose a different folder to continue."
        :actions="[
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <UEmpty
        v-else-if="state === 'no-analyzed' && summary"
        icon="i-lucide-triangle-alert"
        title="None of the photos in this folder could be analyzed"
        :description="`${summary.failed} of ${summary.total} files failed. They may be corrupted or in an unsupported format.`"
        :actions="[
          {
            label: 'Choose a different folder',
            icon: 'i-lucide-folder-open',
            color: 'primary',
            onClick: pickFolderAndAnalyze,
          },
        ]"
        class="mx-auto mt-16 max-w-lg"
      />

      <div v-else-if="state === 'results' && summary" class="space-y-8">
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
            Percentiles are ranked within this folder. Sparkle is aesthetic, focus is sharpness.
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
          Everything from here is Phase 2: pick a length, generate a book
          (which SAVES it), pre-flight it, and export the print files. Mounted
          only in the results state, since all of it operates on the fully
          ranked set -- `photos`, which is `summary.photos` with `kept`
          re-stamped by Rust for the user's own overrides. `overrides` rides
          along so `generate_book` can persist the decisions with the project.
        -->
        <GenerateBook
          v-if="folder"
          :photos
          :overrides
          :photo-set-id="photoSetId"
          :folder
          @edit-selection="onEditSelection"
        />

        <UEmpty
          v-if="eventGroups.length === 0"
          icon="i-lucide-image-off"
          title="No photos were kept"
          description="Every analyzed photo was flagged as a screenshot, document, or similar non-photo image, or was excluded by you."
          :actions="[
            {
              label: 'Choose a different folder',
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
  </div>
</template>
