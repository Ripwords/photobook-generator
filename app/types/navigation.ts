import type { AnalysisSummary } from "~/types/features";

/** The saved book a re-edited selection is on its way back to replace. */
export interface ReplacedProject {
  id: number;
  name: string;
}

/**
 * Which screen the app is showing, and everything that screen needs. One ref
 * in `index.vue` holds this; nothing else derives navigation.
 */
export type Screen =
  | { kind: "library" }
  /** A draft's contact sheet. The draft itself lives in `useAnalysisJobs`. */
  | { kind: "select"; jobId: number }
  | { kind: "editor"; projectId: number };

/** What the select screen is showing, once navigation has already put us there. */
export type SelectStage = "running" | "error" | "no-images" | "no-analyzed" | "ready";

export function selectStage(
  running: boolean,
  error: string | null,
  summary: AnalysisSummary | null,
): SelectStage {
  if (running) return "running";
  if (error) return "error";
  // Reached only after a folder pick, so the gap before the first event is
  // still the analysis starting up, not an empty state.
  if (!summary) return "running";
  if (summary.total === 0) return "no-images";
  if (summary.photos.length === 0) return "no-analyzed";
  return "ready";
}
