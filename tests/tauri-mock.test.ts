import { describe, expect, it, vi } from "vitest";
import { invoke } from "../dev/tauri-mock/core";
import { mockPhotos } from "../dev/tauri-mock/photos";
import type { AnalysisEvent, AnalysisSummary } from "../app/types/features";

describe("the browser harness's commands", () => {
  it("analyses its folder to the end, with every mock photo", async () => {
    vi.useFakeTimers();
    try {
      const events: AnalysisEvent[] = [];
      const onEvent = { onmessage: (event: AnalysisEvent) => events.push(event) };
      const done = invoke<AnalysisSummary>("analyze_folders", { folders: ["/mock"], onEvent });
      await vi.runAllTimersAsync();
      const summary = await done;

      const paths = mockPhotos().map((photo) => photo.path);
      expect(summary.photos.map((photo) => photo.path)).toEqual(paths);
      expect(events.at(-1)).toEqual({ kind: "done", summary });
    } finally {
      vi.useRealTimers();
    }
  });
});
