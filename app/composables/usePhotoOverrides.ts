import { invoke } from "@tauri-apps/api/core";
// Imported explicitly rather than left to Nuxt's auto-imports so this file
// can be exercised by `tests/overrides.test.ts` under plain vitest, which has
// no Nuxt runtime. The round trip below is the single mechanism keeping the
// culling rule out of the webview, so it is worth being testable.
import { ref, watch, type Ref } from "vue";
import {
  withOverride,
  type AnalyzedPhoto,
  type PhotoOverride,
  type PhotoOverrides,
} from "~/types/features";

/**
 * The user's own include/exclude decisions over an analysed folder.
 *
 * **There is exactly one culling authority and this composable does not
 * become a second one.** `book::cull::cull` in Rust decides which photo
 * survives; `commands::stamp_kept` writes that verdict onto each record as
 * `kept`; `keepers()` in `types/features.ts` is a filter on the flag with no
 * ranking of its own. Phase 2 spent a whole task collapsing a Rust rule and a
 * TypeScript rule into that one, because the contact sheet and the printed
 * book disagreed about which photos survived.
 *
 * So a toggle here does NOT apply itself to the photos on screen. It goes
 * down to Rust via `apply_photo_overrides`, which re-runs the same `cull` the
 * book is built from and hands the records back with `kept` re-stamped. That
 * round trip is the point: the contact sheet is showing Rust's answer, not
 * its own, and it cannot drift from the book.
 *
 * `photos` is a replacement for the analysed array, not an addition to it --
 * same records, same order, only `kept` changed -- so every consumer
 * (`keepers`, `groupByEvent`, `burstSizes`, `GenerateBook`) reads it exactly
 * as it read `summary.photos` before.
 */
export function usePhotoOverrides(source: Ref<AnalyzedPhoto[]>) {
  const overrides = ref<PhotoOverrides>({});
  /** The analysed records with Rust's latest verdict stamped on them. */
  const photos = ref<AnalyzedPhoto[]>([]) as Ref<AnalyzedPhoto[]>;
  const error = ref<string | null>(null);
  const busy = ref(false);

  // A different analysed folder means different photos and no decisions about
  // them: a hash-keyed map carried across would silently apply a decision
  // made about one folder's photo to an identical file in another. `immediate`
  // because the first analysed set arrives through this same path.
  watch(
    source,
    (next) => {
      overrides.value = {};
      photos.value = next;
      error.value = null;
    },
    { immediate: true },
  );

  /**
   * Records a decision and asks Rust to re-judge the whole set.
   *
   * The map is updated first and left updated even if the command fails: it
   * is the user's stated intent, and `generate_book` is what finally acts on
   * it. Only the `kept` stamps -- Rust's answer -- fail to refresh, and that
   * is surfaced in `error` rather than swallowed.
   */
  async function setOverride(hash: string, state: PhotoOverride) {
    overrides.value = withOverride(overrides.value, hash, state);
    busy.value = true;
    error.value = null;
    try {
      photos.value = await invoke<AnalyzedPhoto[]>("apply_photo_overrides", {
        photos: photos.value,
        overrides: overrides.value,
      });
    } catch (e) {
      error.value = String(e);
    } finally {
      busy.value = false;
    }
  }

  return { overrides, photos, error, busy, setOverride };
}
