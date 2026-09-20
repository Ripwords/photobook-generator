import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { editLabel, invoke } from "../dev/tauri-mock/core";
import { mockPhotos } from "../dev/tauri-mock/photos";
import layoutFixture from "./fixtures/wire/book-layout.json";
import type { AnalysisEvent, AnalysisSummary } from "../app/types/features";
import type { BookEdit, BookLayout, HistoryStatus } from "../app/types/preview";
import type { PrintSpec } from "../app/types/printSpec";

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

/** Every `BookEdit`, mirroring `book::history::tests::every_edit`. */
const EVERY_EDIT: BookEdit[] = [
  { kind: "regenerate", opening: 0 },
  { kind: "rejectTemplate", opening: 0 },
  { kind: "setTemplate", opening: 0, templateId: "t" },
  { kind: "setLocked", opening: 0, locked: true },
  { kind: "setLocked", opening: 0, locked: false },
  { kind: "shuffle" },
  { kind: "swapPhotos", a: { page: 1, z: 0 }, b: { page: 2, z: 0 } },
  { kind: "setCrop", placement: { page: 1, z: 0 }, x: 0, y: 0, w: 1 },
  { kind: "setSlot", placement: { page: 1, z: 0 }, rect: { x: 0, y: 0, w: 1, h: 1 } },
  { kind: "replacePhoto", placement: { page: 1, z: 0 }, photo: 3 },
  { kind: "setPrintSpec", spec: layoutFixture.spec as PrintSpec },
  { kind: "setCoverPhoto", side: "front", photo: 1 },
  { kind: "setCoverPhoto", side: "front", photo: null },
  { kind: "setCoverCrop", side: "front", x: 0, y: 0, w: 1 },
  { kind: "setSpineColour", rgb: "#112233" },
];

describe("the harness's undo timeline", () => {
  it("labels every edit exactly the way Rust does", () => {
    const rust = readFileSync(new URL("../src-tauri/src/book/history.rs", import.meta.url), "utf8");
    const body = /pub fn edit_label\(edit: &BookEdit\) -> &'static str \{([\s\S]*?)\n\}/.exec(rust);
    expect(body, "edit_label was renamed or reshaped in history.rs").not.toBeNull();
    const rustLabels = [...body![1]!.matchAll(/=> "([^"]+)"/g)].map((m) => m[1]!);
    expect(rustLabels.length).toBeGreaterThan(0);
    expect(EVERY_EDIT.map(editLabel).toSorted()).toEqual(rustLabels.toSorted());
  });

  it("steps a real edit back and forward, and says what each control would do", async () => {
    // Relative to whatever the harness's book is at, so this does not depend
    // on which tests ran before it.
    const start = await invoke<HistoryStatus>("book_history");
    const before = await invoke<BookLayout>("book_layout");

    const edited = await invoke<BookLayout>("edit_book", {
      edit: { kind: "setSpineColour", rgb: "#123456" },
    });
    expect(edited.cover.spine).toBe("#123456");
    expect(await invoke<HistoryStatus>("book_history")).toEqual({
      undo: "spine colour",
      redo: null,
    });

    const undone = await invoke<BookLayout>("step_book", { step: "undo" });
    expect(undone.cover.spine).toBe(before.cover.spine);
    expect(await invoke<HistoryStatus>("book_history")).toEqual({
      undo: start.undo,
      redo: "spine colour",
    });

    const redone = await invoke<BookLayout>("step_book", { step: "redo" });
    expect(redone.cover.spine).toBe("#123456");
    expect(await invoke<HistoryStatus>("book_history")).toEqual({
      undo: "spine colour",
      redo: null,
    });
  });

  it("leaves a refused edit off the timeline and the book as it was", async () => {
    const before = await invoke<BookLayout>("book_layout");
    const status = await invoke<HistoryStatus>("book_history");

    await expect(
      invoke("edit_book", { edit: { kind: "setSpineColour", rgb: "rather teal" } }),
    ).rejects.toThrow();

    expect(await invoke<HistoryStatus>("book_history")).toEqual(status);
    expect(await invoke<BookLayout>("book_layout")).toEqual(before);
  });

  it("abandons the redo branch when a fresh edit lands after an undo", async () => {
    await invoke("edit_book", { edit: { kind: "setSpineColour", rgb: "#aabbcc" } });
    await invoke("step_book", { step: "undo" });
    expect((await invoke<HistoryStatus>("book_history")).redo).toBe("spine colour");

    await invoke("edit_book", { edit: { kind: "setCoverCrop", side: "front", x: 0, y: 0, w: 1 } });
    expect(await invoke<HistoryStatus>("book_history")).toEqual({
      undo: "cover crop",
      redo: null,
    });

    // And the abandoned state really is gone: redo cannot reach it.
    const here = await invoke<BookLayout>("book_layout");
    expect(await invoke<BookLayout>("step_book", { step: "redo" })).toEqual(here);
  });

  it("does nothing at either end of the timeline", async () => {
    // Back as far as it goes, then one more: the book must not move.
    for (let n = 0; n < 20; n++) await invoke("step_book", { step: "undo" });
    const oldest = await invoke<BookLayout>("book_layout");
    expect(await invoke<BookLayout>("step_book", { step: "undo" })).toEqual(oldest);

    for (let n = 0; n < 20; n++) await invoke("step_book", { step: "redo" });
    const newest = await invoke<BookLayout>("book_layout");
    expect(await invoke<BookLayout>("step_book", { step: "redo" })).toEqual(newest);
  });
});
