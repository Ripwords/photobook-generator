import { invoke } from "@tauri-apps/api/core";
// Imported explicitly rather than left to Nuxt's auto-imports so this file
// can be exercised by `tests/overrides.test.ts` under plain vitest, which has
// no Nuxt runtime. The round trip below is the single mechanism keeping the
// culling rule out of the webview, so it is worth being testable.
import { computed, ref, watch, type Ref } from "vue";
import { withOverride, type AnalyzedPhoto, type PhotoOverride, type PhotoOverrides } from "~/types/features";

/**
 * How long a burst of clicks is allowed to settle before Rust is asked to
 * re-judge the set. Clicking through a row of photos fires one toggle per
 * click, and each one is a whole cull plus a re-render; coalescing them costs
 * a barely perceptible pause and removes the entire pile-up.
 */
export const OVERRIDE_SETTLE_MS = 120;

/**
 * The user's own include/exclude decisions over an analysed folder.
 *
 * **There is exactly one culling authority and this composable does not
 * become a second one.** `book::cull::cull` in Rust decides which photo
 * survives. Phase 2 spent a whole task collapsing a Rust rule and a
 * TypeScript rule into that one, because the contact sheet and the printed
 * book disagreed about which photos survived, and this is exactly where it
 * could happen again: an include/exclude toggle is trivially applicable on
 * the client.
 *
 * So a toggle does NOT apply itself. It goes down to Rust via
 * `apply_photo_overrides`, which re-runs the same `cull` the book is built
 * from and returns THE PATHS THAT SURVIVE. Stamping those onto the records is
 * applying an answer, not deciding one -- the ranking, the cluster rule, the
 * capture-quality tie-break and the override semantics all stay in Rust.
 *
 * The command takes only the override map and the run id: the analysed set
 * is cached in `AppState` on the Rust side. Sending the records instead was
 * ~2.5 KB per photo each way, which is ~20 MB of round trip per click on a
 * 1000-photo folder. The run id is what makes that cache safe to answer
 * from -- Rust refuses to judge a set other than the one this screen shows.
 */
export function usePhotoOverrides(source: Ref<AnalyzedPhoto[]>, runId: Ref<number>) {
  const overrides = ref<PhotoOverrides>({});
  /** The analysed records carrying Rust's latest verdict in `kept`. */
  const photos = ref<AnalyzedPhoto[]>([]) as Ref<AnalyzedPhoto[]>;
  const error = ref<string | null>(null);
  const busy = ref(false);

  /**
   * Bumped once per analysed SET, never per toggle.
   *
   * `photos` is replaced with a new array on every override, so anything
   * watching it fires on every click. `GenerateBook` used to, and its reset
   * branch then wiped the generated book, the chosen output folder, the
   * export report, a name the user had typed and a page length they had
   * chosen -- every single toggle. This is the identity to watch when the
   * question is "is this a different set of photos?".
   */
  const photoSetId = ref(0);

  /**
   * Sequencing for in-flight commands. Two fast clicks are two calls, and
   * without this the older reply can land last and leave a stale verdict on
   * screen. The map stays correct either way, so the BOOK would still be
   * right -- only the contact sheet would lie, which is worse than a visible
   * bug because nothing about it looks wrong.
   */
  let latest = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  watch(
    source,
    (next) => {
      // A different analysed folder means different photos and no decisions
      // about them: a hash-keyed map carried across would silently apply a
      // decision made about one folder's photo to an identical file in
      // another. `immediate` because the first set arrives through this path.
      overrides.value = {};
      photos.value = next;
      error.value = null;
      photoSetId.value += 1;
      // Any reply still in flight describes the previous set.
      latest += 1;
      clearTimeout(timer);
      busy.value = false;
    },
    { immediate: true },
  );

  /** Asks Rust to re-judge the set, and applies its answer. */
  async function refresh() {
    const generation = ++latest;
    busy.value = true;
    error.value = null;
    try {
      const kept = new Set(
        await invoke<string[]>("apply_photo_overrides", {
          runId: runId.value,
          overrides: overrides.value,
        }),
      );
      // A reply from a superseded click, or from a previous folder, must not
      // overwrite a newer verdict.
      if (generation !== latest) return;
      photos.value = photos.value.map((photo) =>
        photo.kept === kept.has(photo.path) ? photo : { ...photo, kept: kept.has(photo.path) },
      );
    } catch (e) {
      if (generation !== latest) return;
      error.value = String(e);
    } finally {
      if (generation === latest) busy.value = false;
    }
  }

  /**
   * Records a decision and schedules the re-judge.
   *
   * The map is updated synchronously and stays updated even if the command
   * fails: it is the user's stated intent, and `generate_book` is what
   * finally acts on it. Only the `kept` stamps -- Rust's answer -- can go
   * stale, and that surfaces in `error` rather than being swallowed.
   *
   * Returns a promise that settles once the verdict has been applied, so a
   * caller (and a test) can await the whole round trip rather than the click.
   */
  function setOverride(hash: string, state: PhotoOverride): Promise<void> {
    overrides.value = withOverride(overrides.value, hash, state);
    busy.value = true;
    clearTimeout(timer);
    return new Promise((resolve) => {
      timer = setTimeout(() => {
        void refresh().then(resolve);
      }, OVERRIDE_SETTLE_MS);
    });
  }

  /**
   * Adopts a whole set of decisions at once -- reopening a saved project and
   * then re-analysing its folder, where the decisions come back from the
   * database rather than from a click.
   *
   * Goes through the same Rust round trip as a single toggle: a restored map
   * must be judged by the same authority as a fresh one, or a reopened
   * project's contact sheet would disagree with a freshly analysed one.
   */
  async function restore(restored: PhotoOverrides) {
    overrides.value = { ...restored };
    clearTimeout(timer);
    await refresh();
  }

  const includedCount = computed(
    () => Object.values(overrides.value).filter((state) => state === "include").length,
  );
  const excludedCount = computed(
    () => Object.values(overrides.value).filter((state) => state === "exclude").length,
  );

  return {
    overrides,
    photos,
    photoSetId,
    error,
    busy,
    includedCount,
    excludedCount,
    setOverride,
    restore,
  };
}
