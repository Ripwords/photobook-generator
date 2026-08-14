import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  applyExportEvent,
  blockingMessages,
  defaultProjectName,
  generatedLabel,
  initialExportProgress,
  lastExportLabel,
  optionFor,
  projectDetailLabel,
  recommendedOption,
  revealTarget,
  summarizeExport,
  warningMessages,
  type BookRecommendation,
  type ExportEvent,
  type ExportResult,
  type GeneratedBook,
  type ProjectDetail,
  type ProjectListItem,
} from "../app/types/book";

/**
 * The TypeScript half of the Rust<->webview pin.
 *
 * Every fixture below is the SAME file `src-tauri/src/commands.rs`'s
 * `*_serialises_exactly_the_keys_the_webview_reads` tests assert their real
 * serialised structs equal, byte for byte. Neither half can catch a rename on
 * its own: a Rust-to-Rust round trip moves both of its sides together, and a
 * TypeScript test over a literal it wrote itself pins nothing. Reading one
 * committed artefact from both languages is what makes a rename fail
 * somewhere -- and makes "just update the fixture" fail on the other side.
 *
 * These tests deliberately go through the real functions the UI calls, not
 * through `Object.keys`: a key that drifts reads back `undefined`, and the
 * value assertions below are what turn that into a failure.
 */
const fixture = <T>(name: string): T =>
  JSON.parse(
    readFileSync(fileURLToPath(new URL(`./fixtures/wire/${name}`, import.meta.url)), "utf-8"),
  ) as T;

describe("book recommendation wire shape", () => {
  const rec = fixture<BookRecommendation>("book-recommendation.json");

  it("reads the keeper count and the recommended length Rust serialised", () => {
    expect(rec.keeperCount).toBe(26);
    expect(recommendedOption(rec)).toEqual({
      pages: 20,
      capacityPhotos: 24,
      droppedPhotos: 2,
    });
  });

  it("reads the capacity and drop count of an overridden length", () => {
    expect(optionFor(rec, 40)).toEqual({ pages: 40, capacityPhotos: 54, droppedPhotos: 0 });
  });

  it("returns nothing for a length the backend did not offer", () => {
    expect(optionFor(rec, 60)).toBeUndefined();
  });

  it("falls back to the first option if the recommended length is not in the list", () => {
    // Defensive: the UI renders `recommendedOption(...)` directly, so an
    // unmatched recommendation must not put `undefined` on screen.
    const drifted: BookRecommendation = { ...rec, recommendedPages: 99 };
    expect(recommendedOption(drifted)).toEqual(rec.options[0]);
  });
});

describe("generated book wire shape", () => {
  const generated = fixture<GeneratedBook>("generated-book.json");

  it("describes what was generated from the keys Rust serialised", () => {
    expect(generatedLabel(generated)).toBe("20 pages · 24 photos placed · 2 left out");
  });

  it("carries the project id everything downstream is keyed by", () => {
    expect(generated.projectId).toBe(7);
  });

  it("says so plainly when nothing was left out", () => {
    expect(generatedLabel({ ...generated, droppedPhotos: 0 })).toBe(
      "20 pages · 24 photos placed",
    );
  });
});

describe("export result wire shape", () => {
  const exported = fixture<ExportResult>("export-result.json");
  const blocked = fixture<ExportResult>("export-result-blocked.json");

  it("counts what was written, what failed, and what merely warned", () => {
    expect(summarizeExport(exported)).toEqual({
      blocked: false,
      writtenCount: 1,
      failedCount: 1,
      blockingCount: 0,
      warningCount: 1,
    });
  });

  it("reveals the first file actually written, not the directory", () => {
    expect(revealTarget(exported)).toBe(
      "/Users/jj/Desktop/photobook-export/p04-z1-abcd1234.jpg",
    );
  });

  it("falls back to the output directory when nothing was written", () => {
    expect(revealTarget(blocked)).toBe("/Users/jj/Desktop/photobook-export");
  });

  /**
   * The regression this separation exists for: a Block and a Warn are not
   * the same kind of finding, and a UI that lists them together (or picks
   * the wrong one) tells the user an export was fine when it wrote nothing.
   * Asserted in both directions -- the block list must not contain the
   * warning's text and vice versa -- so swapping the two lists fails rather
   * than merely reordering the screen.
   */
  it("keeps blocking findings and warnings apart", () => {
    expect(blockingMessages(blocked)).toEqual([
      "Photo resolves at 120 DPI in this slot, below the 200 DPI floor",
    ]);
    expect(warningMessages(blocked)).toEqual([
      "Salient content falls inside the gutter dead strip",
    ]);
    expect(summarizeExport(blocked)).toEqual({
      blocked: true,
      writtenCount: 0,
      failedCount: 0,
      blockingCount: 1,
      warningCount: 1,
    });
  });

  it("reads the severity spelling Rust actually emits", () => {
    expect(blocked.blocking[0]?.severity).toBe("block");
    expect(blocked.warnings[0]?.severity).toBe("warn");
    expect(blocked.blocking[0]?.photoPath).toBe("/photos/IMG_0042.jpg");
    expect(blocked.blocking[0]?.page).toBe(4);
  });

  it("carries a manifest path only when a manifest was written", () => {
    expect(exported.manifestPath).toBe("/Users/jj/Desktop/photobook-export/manifest.json");
    expect(blocked.manifestPath).toBeNull();
  });
});

describe("export progress", () => {
  const events = fixture<ExportEvent[]>("export-events.json");

  it("reduces the events Rust streams into a determinate progress state", () => {
    const state = events.reduce(applyExportEvent, initialExportProgress);
    expect(state).toEqual({ running: true, completed: 8, total: 24 });
  });

  it("starts idle with nothing to show", () => {
    expect(initialExportProgress).toEqual({ running: false, completed: 0, total: 0 });
  });

  /**
   * `initialExportProgress` is a shared module-level object assigned straight
   * into a ref on every export, exactly like `initialStreamState` -- mutating
   * it in place would leak one export's progress into the next, permanently.
   */
  it("never mutates the state it is given", () => {
    const before = { ...initialExportProgress };
    applyExportEvent(initialExportProgress, { kind: "progress", completed: 3, total: 9 });
    expect(initialExportProgress).toEqual(before);
  });

  it("a progress event before a started event still yields a usable total", () => {
    expect(applyExportEvent(initialExportProgress, { kind: "progress", completed: 2, total: 6 }))
      .toEqual({ running: true, completed: 2, total: 6 });
  });
});

describe("project list wire shape", () => {
  const projects = fixture<ProjectListItem[]>("project-list.json");

  it("summarises the most recent export from the keys Rust serialised", () => {
    expect(projects[0]?.name).toBe("Japan 2026");
    expect(projects[0]?.pageCount).toBe(20);
    expect(lastExportLabel(projects[0]!)).toBe(
      "24 jpg files in /Users/jj/Desktop/photobook-export",
    );
  });

  it("distinguishes a book that has never been exported", () => {
    expect(projects[1]?.lastExport).toBeNull();
    expect(lastExportLabel(projects[1]!)).toBe("Not exported yet");
  });

  it("reads a reopened project's counts and export history", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");
    expect(projectDetailLabel(detail)).toBe("20 pages · 24 photos · 1 export");
    expect(detail.seed).toBe(424242);
    expect(detail.exports[0]?.fileCount).toBe(24);
  });

  it("counts a never-exported project's history as none", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");
    expect(projectDetailLabel({ ...detail, exports: [] })).toBe("20 pages · 24 photos · no exports");
  });
});

describe("defaultProjectName", () => {
  it("names a book after the folder it came from", () => {
    expect(defaultProjectName("/Users/jj/Pictures/Japan 2026")).toBe("Japan 2026");
  });

  it("tolerates a trailing slash", () => {
    expect(defaultProjectName("/Users/jj/Pictures/Japan 2026/")).toBe("Japan 2026");
  });

  it("falls back to a generic name for an empty path", () => {
    expect(defaultProjectName("")).toBe("Untitled photobook");
  });
});
