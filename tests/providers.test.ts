import { describe, expect, it } from "vitest";
import { planNote, sendView, type SendEvent } from "../app/types/providers";

describe("sendView", () => {
  it("is idle before the first event", () => {
    expect(sendView(null)).toEqual({ kind: "idle" });
  });

  it("measures uploading against its own total", () => {
    expect(sendView({ kind: "uploading", done: 3, total: 4 })).toEqual({
      kind: "sending",
      label: "Uploading photos, 3 of 4",
      progress: 0.75,
    });
    expect(sendView({ kind: "uploading", done: 1, total: 4 })).toMatchObject({ progress: 0.25 });
  });

  it("measures exporting against its own total", () => {
    expect(sendView({ kind: "exporting", done: 2, total: 8 })).toEqual({
      kind: "sending",
      label: "Exporting photos, 2 of 8",
      progress: 0.25,
    });
  });

  it("is indeterminate for a counted phase with nothing to count", () => {
    expect(sendView({ kind: "uploading", done: 0, total: 0 })).toMatchObject({ progress: null });
  });

  it("is indeterminate while creating and laying out", () => {
    expect(sendView({ kind: "creating" })).toEqual({
      kind: "sending",
      label: "Creating the Pixajoy project",
      progress: null,
    });
    expect(sendView({ kind: "layingOut" })).toEqual({
      kind: "sending",
      label: "Laying out the pages",
      progress: null,
    });
  });

  it("carries the project name and link when done", () => {
    const event: SendEvent = { kind: "done", projectName: "Japan 2026", openUrl: "https://x/p/1" };
    expect(sendView(event)).toEqual({ kind: "done", projectName: "Japan 2026", openUrl: "https://x/p/1" });
  });

  it("warns about a partly filled project only when one was created", () => {
    expect(sendView({ kind: "failed", message: "Upload failed", projectName: "Japan 2026" })).toEqual({
      kind: "failed",
      message: "Upload failed",
      leftover: "A partly filled project named “Japan 2026” may be in My Projects.",
    });
    expect(sendView({ kind: "failed", message: "Sign in again", projectName: null })).toEqual({
      kind: "failed",
      message: "Sign in again",
      leftover: null,
    });
  });
});

describe("planNote", () => {
  it("states the page count with no padding note when nothing is added", () => {
    expect(planNote({ bookPages: 40, pageCount: 40 })).toEqual({
      pages: "40 pages",
      padding: null,
    });
  });

  it("counts the blank pages added to reach the 20-page minimum", () => {
    expect(planNote({ bookPages: 12, pageCount: 20 })).toEqual({
      pages: "20 pages",
      padding: "Your book has 12 pages. Pixajoy needs at least 20, so 8 blank pages are added at the end.",
    });
    expect(planNote({ bookPages: 19, pageCount: 20 }).padding).toBe(
      "Your book has 19 pages. Pixajoy needs at least 20, so 1 blank page is added at the end.",
    );
  });

  it("says why one page is added to an odd count above the minimum", () => {
    expect(planNote({ bookPages: 33, pageCount: 34 })).toEqual({
      pages: "34 pages",
      padding: "Your book has 33 pages. Pixajoy needs an even count, so 1 blank page is added at the end.",
    });
  });
});
