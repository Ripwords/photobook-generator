import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AnalysisSummary } from "~/types/features";

export function useAnalysis() {
  const summary = ref<AnalysisSummary | null>(null);
  const running = ref(false);
  const error = ref<string | null>(null);

  async function pickFolderAndAnalyze() {
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== "string") return;

    running.value = true;
    error.value = null;
    try {
      summary.value = await invoke<AnalysisSummary>("analyze_folder", { folder });
    } catch (e) {
      error.value = String(e);
    } finally {
      running.value = false;
    }
  }

  return { summary, running, error, pickFolderAndAnalyze };
}
