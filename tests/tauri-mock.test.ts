import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { editLabel, invoke } from "../dev/tauri-mock/core";
import { mockPhotos } from "../dev/tauri-mock/photos";
import layoutFixture from "./fixtures/wire/book-layout.json";
import type { AnalysisEvent, AnalysisSummary } from "../app/types/features";
import type { BookEdit, BookLayout, HistoryStatus, SlotCandidate } from "../app/types/preview";
import type { BookRecommendation, ImportedPhoto } from "../app/types/book";
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

  // The harness used to hand back the last photo the book already had, which
  // made a broken import look like a working one on screen: the dialog is
  // holding a `BookLayout` one photo short of the index Rust returns, and a
  // harness that never went past the end could not show that. See
  // `withImportedPhotos`.
  it("appends a hand-picked photo past the end of the book's photos, the way Rust does", async () => {
    const before = await invoke<BookLayout>("book_layout", {});
    const added = await invoke<ImportedPhoto>("import_photo", { projectId: 1, path: "/elsewhere/IMG_9999.JPG" });

    expect(added.alreadyKnown).toBe(false);
    expect(added.photoIndex).toBe(before.photos.length);
    expect(added.photo.path).toBe("/elsewhere/IMG_9999.JPG");
    expect(await invoke<SlotCandidate[]>("slot_candidates", { placement: { page: 2, z: 1 } })).toHaveLength(
      added.photoIndex + 1,
    );
  });

  // `isPlaced` (app/types/book.ts) dims the contact sheet by whichever
  // option the user has chosen a length for. If a shorter option's
  // selection were not a prefix of a longer one's, a tile could go from
  // undimmed at 40 pages back to dimmed at 20 -- this pins the property the
  // dimming logic actually depends on, not just that options differ.
  it("recommend_book nests a shorter option's selection inside a longer one's", async () => {
    const recommendation = await invoke<BookRecommendation>("recommend_book", { overrides: {} });
    const [shorter, longer] = recommendation.options.toSorted((a, b) => a.pages - b.pages);

    expect(shorter!.selectedPaths.length).toBeGreaterThan(0);
    expect(longer!.selectedPaths.length).toBeGreaterThan(shorter!.selectedPaths.length);
    // A prefix, in order -- not just a subset -- so a tile undimmed at the
    // shorter length is undimmed at the same position in the longer one too.
    expect(longer!.selectedPaths.slice(0, shorter!.selectedPaths.length)).toEqual(shorter!.selectedPaths);
  });

  it("hands back where a photo it already holds sits, without appending again", async () => {
    const before = await invoke<BookLayout>("book_layout", {});
    const known = before.photos[1]!;
    const added = await invoke<ImportedPhoto>("import_photo", { projectId: 1, path: known.path });

    expect(added).toEqual({ photoIndex: 1, alreadyKnown: true, photo: known });
    expect((await invoke<BookLayout>("book_layout", {})).photos).toHaveLength(before.photos.length);
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

/**
 * Rust's `edit_label` arms, keyed by the variant and by whatever the arm is
 * guarded on: `SetLocked { locked: true, .. } => "lock"` becomes
 * `"SetLocked locked: true" -> "lock"`. Two arms of one variant are the
 * whole difficulty -- lock versus unlock, setting a cover photo versus
 * clearing one -- and a key that dropped the guard would collapse them.
 */
function rustArms(body: string): Map<string, string> {
  const arms = new Map<string, string>();
  for (const [, variant, fields, label] of body.matchAll(
    /BookEdit::(\w+)\s*(?:\{([^}]*)\})?\s*=> "([^"]+)"/g,
  )) {
    const guards = (fields ?? "")
      .split(",")
      .map((field) => field.trim())
      .filter((field) => field !== "" && field !== "..");
    arms.set([variant!, ...guards].join(" "), label!);
  }
  return arms;
}

/** What Rust would call this edit, by walking its arms the way `match` does. */
function rustLabel(arms: Map<string, string>, edit: BookEdit): string | undefined {
  const variant = edit.kind[0]!.toUpperCase() + edit.kind.slice(1);
  const fields: Record<string, unknown> = { ...edit };
  for (const [key, label] of arms) {
    if (key !== variant && !key.startsWith(`${variant} `)) continue;
    const guard = key.slice(variant.length).trim();
    if (guard === "") return label;
    const [field, want] = guard.split(":").map((part) => part.trim());
    const got = fields[field!];
    const matches =
      want === "true" ? got === true
      : want === "false" ? got === false
      : want === "None" ? got === null
      : got !== null && got !== undefined;
    if (matches) return label;
  }
  return undefined;
}

describe("the harness's undo timeline", () => {
  it("labels every edit exactly the way Rust does", () => {
    const rust = readFileSync(new URL("../src-tauri/src/book/history.rs", import.meta.url), "utf8");
    const body =
      /pub fn edit_label\(edit: &BookEdit, before: &Book\) -> &'static str \{([\s\S]*?)\n\}/.exec(
        rust,
      );
    expect(body, "edit_label was renamed or reshaped in history.rs").not.toBeNull();
    const arms = rustArms(body![1]!);
    expect(arms.size).toBeGreaterThan(0);

    // `SetSlot` is the one arm whose label is computed rather than fixed, so
    // `rustArms` cannot read it and it is checked on its own below. Naming it
    // here means a SECOND computed arm fails this test instead of quietly
    // dropping out of both sides of the comparison.
    const named = [...arms.keys()];
    const computed = [...new Set([...body![1]!.matchAll(/BookEdit::(\w+)/g)].map((m) => m[1]!))]
      .filter((variant) => !named.some((key) => key === variant || key.startsWith(`${variant} `)));
    expect(new Set(computed)).toEqual(new Set(["SetSlot"]));

    const fixed = EVERY_EDIT.filter((edit) => edit.kind !== "setSlot");
    // Edit by edit, not two sorted lists: sorted lists agree even when the
    // mock has swapped two labels, and a harness that calls a resize a crop
    // is worse than one that refuses to label anything.
    for (const edit of fixed) {
      expect(editLabel(edit, layoutFixture), `${JSON.stringify(edit)} is labelled differently`).toBe(
        rustLabel(arms, edit),
      );
    }
    // And no arm is left unvisited, so a label Rust grew is a failure here
    // rather than an untested corner of the harness.
    expect(new Set(fixed.map((edit) => editLabel(edit, layoutFixture)))).toEqual(
      new Set(arms.values()),
    );
  });

  /**
   * Both layout-mode gestures emit one `setSlot`, and the harness has to tell
   * them apart the way Rust does -- from the size the slot had, which lives
   * in the book and not in the edit.
   */
  it("calls a slot that only changed place a move, and any other one a resize", () => {
    const rust = readFileSync(new URL("../src-tauri/src/book/history.rs", import.meta.url), "utf8");
    const arm = /BookEdit::SetSlot \{[^}]*\} => \{([\s\S]*?)\n        \}/.exec(rust);
    expect(arm, "the SetSlot arm was reshaped in history.rs").not.toBeNull();
    expect(arm![1]).toContain('"move"');
    expect(arm![1]).toContain('"resize"');

    const slot = layoutFixture.pages[0]!.placements[0]!;
    const at = { page: layoutFixture.pages[0]!.number, z: slot.z };
    const rect = slot.slotRect;

    expect(
      editLabel({ kind: "setSlot", placement: at, rect: { ...rect, x: rect.x / 2 } }, layoutFixture),
    ).toBe("move");
    expect(
      editLabel({ kind: "setSlot", placement: at, rect: { ...rect, w: rect.w / 2 } }, layoutFixture),
    ).toBe("resize");
    // Height alone counts too: checking only the width called a box dragged
    // shorter a move.
    expect(
      editLabel({ kind: "setSlot", placement: at, rect: { ...rect, h: rect.h / 2 } }, layoutFixture),
    ).toBe("resize");
    // A corner drag reshapes AND shifts the box; it is still a resize.
    expect(
      editLabel(
        { kind: "setSlot", placement: at, rect: { x: 0, y: 0, w: rect.w / 2, h: rect.h / 2 } },
        layoutFixture,
      ),
    ).toBe("resize");
    // A slot the book does not have keeps the old wording.
    expect(editLabel({ kind: "setSlot", placement: { page: 999, z: 0 }, rect }, layoutFixture)).toBe(
      "resize",
    );
  });

  /**
   * The label has to be taken BEFORE the edit lands, or the slot already has
   * its new size and every drag reads as a move. Rust cannot be driven from
   * here -- both call sites need an `AppHandle` -- so the two orderings are
   * pinned as source, and the harness, which can be driven, is driven.
   */
  it("reads the label off the book before the edit overwrites it", async () => {
    const rust = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");
    const calls = [...rust.matchAll(/edit_label\(&edit, &project\.book\);([\s\S]*?)commit_book/g)];
    expect(calls.length, "edit_label's call sites moved or changed shape").toBe(2);
    for (const [, between] of calls) {
      expect(between).toMatch(/apply\(&mut project\.book|edit_and_view\(\s*&mut project\.book/);
    }

    const before = await invoke<BookLayout>("book_layout");
    const page = before.pages.find((p) => p.placements.length > 0)!;
    const slot = page.placements[0]!;
    const at = { page: page.number, z: slot.z };
    const shifted = { ...slot.slotRect, x: Math.max(0, slot.slotRect.x - 0.02) };

    await invoke<BookLayout>("edit_book", { edit: { kind: "setSlot", placement: at, rect: shifted } });
    expect((await invoke<HistoryStatus>("book_history")).undo).toBe("move");

    await invoke<BookLayout>("edit_book", {
      edit: { kind: "setSlot", placement: at, rect: { ...shifted, w: shifted.w * 0.9 } },
    });
    expect((await invoke<HistoryStatus>("book_history")).undo).toBe("resize");

    await invoke<BookLayout>("step_book", { step: "undo" });
    await invoke<BookLayout>("step_book", { step: "undo" });
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
