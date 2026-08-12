import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AnalysisSummary } from "~/types/features";

export function useAnalysis() {
  const summary = ref<AnalysisSummary | null>(null);
  const running = ref(false);
  const error = ref<string | null>(null);
  const folder = ref<string | null>(null);

  async function analyze(path: string) {
    folder.value = path;
    running.value = true;
    error.value = null;
    try {
      summary.value = await invoke<AnalysisSummary>("analyze_folder", { folder: path });
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

  return { summary, running, error, folder, pickFolderAndAnalyze, retry };
}
