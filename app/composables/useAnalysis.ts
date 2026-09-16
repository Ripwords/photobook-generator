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
  /**
   * The folders the set on screen was analysed from, first picked first.
   * Several folders are one population: every photo is ranked against all of
   * them, and photos from two folders shot the same afternoon fall into one
   * event. That is deliberate -- a book is the union of what it draws from.
   */
  const folders = ref<string[]>([]);

  const summary = computed(() => stream.value.summary);
  /** The run the set on screen came from -- see `AnalysisSummary.runId`. `0` before any analysis. */
  const runId = computed(() => stream.value.summary?.runId ?? 0);
  const scannedTotal = computed(() => stream.value.scannedTotal);
  /** Cumulative count of photos accounted for so far (analysed + cached + failed), out of `scannedTotal`. */
  const processed = computed(
    () => stream.value.analysed + stream.value.cached + stream.value.failed,
  );
  /** Partial photos in arrival order, for the growing grid while `running` is true. */
  const partialPhotos = computed(() => stream.value.partialPhotos);

  async function analyze(paths: string[]) {
    if (paths.length === 0) return;
    folders.value = [...paths];
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
      // The `invoke` return value, not the `Done` event, is the
      // authoritative delivery of the final summary. `analyze_folder` also
      // sends a `Done` event carrying the same summary, but that send is
      // logged-and-swallowed on failure on the Rust side (webview reload
      // mid-run, callback torn down, serialization failure), which would
      // otherwise leave `stream.value.summary` stuck at `null` even though
      // the command itself returned success. There is also a benign race: a
      // `Done` payload for a few hundred photos can exceed Tauri's 8 KiB
      // direct-eval threshold and take an async fetch round-trip, landing
      // *after* this `invoke` promise resolves. So both paths call
      // `applyAnalysisEvent` with a `done` event, and that reducer case
      // must be idempotent -- see the test in `tests/features.test.ts`.
      const finalSummary = await invoke<AnalysisSummary>("analyze_folders", {
        folders: paths,
        onEvent,
      });
      stream.value = applyAnalysisEvent(stream.value, { kind: "done", summary: finalSummary });
    } catch (e) {
      error.value = String(e);
    } finally {
      running.value = false;
    }
  }

  /** Opens the picker for one or more folders. Their union is one analysed set. */
  async function pickFolderAndAnalyze() {
    const picked = await open({ directory: true, multiple: true });
    const paths = typeof picked === "string" ? [picked] : (picked ?? []);
    if (paths.length === 0) return;
    await analyze(paths);
  }

  /** Re-runs analysis on the last picked folders, or opens the picker if none yet. */
  async function retry() {
    if (folders.value.length > 0) {
      await analyze(folders.value);
    } else {
      await pickFolderAndAnalyze();
    }
  }

  return {
    // Exposed so a reopened project can re-analyse its OWN folders without the
    // user picking them again -- see `index.vue`'s "Edit the selection". Every
    // photo is a features-cache hit by then, so it costs no Vision work.
    analyze,
    summary,
    runId,
    running,
    error,
    folders,
    scannedTotal,
    processed,
    partialPhotos,
    pickFolderAndAnalyze,
    retry,
  };
}
