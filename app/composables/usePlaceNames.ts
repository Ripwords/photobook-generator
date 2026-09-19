import { invoke } from "@tauri-apps/api/core";
import { ref, watch, type Ref } from "vue";
import type { PlaceNames } from "~/types/book";

/**
 * Town names for run `runId`'s place chapters, while `places` is on.
 *
 * `place_names` sends chapter centres to Apple's geocoder, so the switch is
 * the only thing that may start it: with it off nothing is asked, and names
 * already fetched are dropped, since time chapters are numbered differently
 * and a name would land on the wrong one.
 */
export function usePlaceNames(runId: Readonly<Ref<number>>, places: Readonly<Ref<boolean>>): Ref<PlaceNames> {
  const names = ref<PlaceNames>({});
  watch(
    [runId, places],
    async ([run, on]) => {
      names.value = {};
      if (!on || run === 0) return;
      try {
        const next = await invoke<PlaceNames>("place_names", { runId: run });
        if (runId.value === run && places.value) names.value = next;
      } catch (e) {
        console.warn("could not name the place chapters", e);
      }
    },
    { immediate: true },
  );
  return names;
}
