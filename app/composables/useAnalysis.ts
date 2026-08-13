import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  applyAnalysisEvent,
  initialStreamState,
  type AnalysisEvent,
  type AnalysisSummary,
  type StreamState,
} from "~/types/features";

export function useAnalysis() {
  // The whole streaming accumulation (partial photos, running counts, the
  // final summary) lives in one `StreamState`, updated by the pure
  // `applyAnalysisEvent` reducer -- see `app/types/features.ts` for why that
  // logic is pulled out to a plain function instead of living inline in the
  // `onmessage` handler below.
  const stream = ref<StreamState>(initialStreamState);
  const running = ref(false);
  const error = ref<string | null>(null);
  const folder = ref<string | null>(null);

  const summary = computed(() => stream.value.summary);
  const scannedTotal = computed(() => stream.value.scannedTotal);
  /** Cumulative count of photos accounted for so far (analysed + cached + failed), out of `scannedTotal`. */
  const processed = computed(
    () => stream.value.analysed + stream.value.cached + stream.value.failed,
  );
  /** Partial photos in arrival order, for the growing grid while `running` is true. */
  const partialPhotos = computed(() => stream.value.partialPhotos);

  async function analyze(path: string) {
    folder.value = path;
    running.value = true;
    error.value = null;
    stream.value = initialStreamState;

    // A `Channel`, not Tauri's event system: events are JSON-string-only and
    // explicitly not designed for high-throughput/low-latency streaming per
    // Tauri's own docs, which hundreds of photos with thumbnails very much
    // is. `analyze_folder` streams `Batch` events (partial, per-photo data)
    // as it goes, then one `Done` event carrying the whole-set derivations
    // (percentiles, cluster ids) -- see the doc comment on `AnalysisEvent`
    // in commands.rs for why those two are split.
    const onEvent = new Channel<AnalysisEvent>();
    // Tauri's `Channel` is not a DOM `EventTarget` -- it has no
    // `addEventListener`, only this single settable `onmessage` field, so
    // the lint rule below does not apply to it.
    // oxlint-disable-next-line unicorn/prefer-add-event-listener
    onEvent.onmessage = (event) => {
      stream.value = applyAnalysisEvent(stream.value, event);
    };

    try {
      await invoke<AnalysisSummary>("analyze_folder", { folder: path, onEvent });
    } catch (e) {
      error.value = String(e);
    } finally {
      running.value = false;
    }
  }

  async function pickFolderAndAnalyze() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    await analyze(picked);
  }

  /** Re-runs analysis on the last picked folder, or opens the picker if none yet. */
  async function retry() {
    if (folder.value) {
      await analyze(folder.value);
    } else {
      await pickFolderAndAnalyze();
    }
  }

  return {
    summary,
    running,
    error,
    folder,
    scannedTotal,
    processed,
    partialPhotos,
    pickFolderAndAnalyze,
    retry,
  };
}
