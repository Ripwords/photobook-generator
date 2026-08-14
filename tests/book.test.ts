import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  applyExportEvent,
  blockingMessages,
  defaultProjectName,
  exportOutcome,
  generatedLabel,
  initialBookState,
  initialExportProgress,
  lastExportedOn,
  lastExportLabel,
  optionFor,
  canGenerateAt,
  includeOverflowLabel,
  projectDetailLabel,
  selectionLabel,
  recommendedOption,
  resolveExportProjectId,
  revealTarget,
  summarizeExport,
  warningMessages,
  withGeneratedBook,
  withOpenedProject,
  type BookRecommendation,
  type BookState,
  type ExportEvent,
  type ExportResult,
  type GeneratedBook,
  type ProjectDetail,
  type ProjectListItem,
} from "../app/types/book";
import {
  overrideFor,
  toggledOverride,
  withOverride,
  type PhotoOverrides,
} from "../app/types/features";

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
      includedOverCapacity: 0,
    });
  });

  it("reads the capacity and drop count of an overridden length", () => {
    expect(optionFor(rec, 40)).toEqual({
      pages: 40,
      capacityPhotos: 54,
      droppedPhotos: 0,
      includedOverCapacity: 0,
    });
  });

  /**
   * The two new keys, read through the real functions the length chooser
   * calls. `includedCount` is what the panel shows beside the keeper count;
   * `includedOverCapacity` is what disables a length outright.
   *
   * A drift on either side reads back `undefined` here: `undefined === 0` is
   * false, so `canGenerateAt` would return false for a length that fits and
   * this test fails -- which is the whole point of asserting through the
   * function rather than on `Object.keys`.
   */
  it("reads how many photos the user picked, and where they do not fit", () => {
    expect(rec.includedCount).toBe(3);
    expect(rec.options.every((option) => canGenerateAt(option))).toBe(true);
    expect(includeOverflowLabel(optionFor(rec, 20)!)).toBeNull();

    const tooMany = { ...optionFor(rec, 20)!, includedOverCapacity: 2 };
    expect(canGenerateAt(tooMany)).toBe(false);
    expect(includeOverflowLabel(tooMany)).toBe(
      "2 more photos marked to include than a 20-page book holds",
    );
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

  it("reads the manifest-write failure Rust reports alongside a successful export", () => {
    expect(exported.manifestError).toBeNull();
    const manifestFailed: ExportResult = {
      ...exported,
      manifestPath: null,
      manifestError: "Read-only file system (os error 30)",
    };
    // The files still exist, so this is NOT a failed export -- the outcome
    // must stay a written one, with the manifest problem reported beside it.
    expect(exportOutcome(manifestFailed)).toBe("partial");
    expect(manifestFailed.manifestError).toBe("Read-only file system (os error 30)");
  });

  /**
   * An export where every item failed has `blocked: false` and `written: []`
   * -- it is neither a block nor a success. Rendering the success alert for
   * it read "0 files written" with an empty format, which tells the user
   * their book exported when nothing did.
   */
  it("does not call an export where nothing was written a success", () => {
    expect(exportOutcome({ ...exported, written: [], failures: exported.failures })).toBe(
      "failed",
    );
    expect(exportOutcome({ ...blocked })).toBe("blocked");
    expect(exportOutcome(exported)).toBe("partial");
    expect(exportOutcome({ ...exported, failures: [] })).toBe("written");
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

  /**
   * **Reopening a project restores the user's own decisions.**
   *
   * Without this key, a reopened project comes back with `overrides:
   * undefined` and every decision quietly reverts to "auto" -- and it looks
   * entirely correct, because the engine's own verdict is a perfectly
   * plausible book. Read through `overrideFor`, so a drifted key surfaces as
   * "auto" for a hash the fixture says is "include" rather than as a
   * TypeScript error that does not exist across this boundary.
   *
   * BOTH states plus an untouched hash: a fixture with one state cannot tell
   * a real map from one that answered the same thing to everything.
   */
  it("restores the include and exclude decisions a project was generated with", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");

    expect(overrideFor(detail.overrides, "a1b2c3d4")).toBe("include");
    expect(overrideFor(detail.overrides, "e5f6a7b8")).toBe("exclude");
    expect(overrideFor(detail.overrides, "never-decided")).toBe("auto");
  });

  /**
   * **The restored decisions must be VISIBLE, not merely returned.**
   *
   * `ProjectDetail.overrides` round-tripped correctly and nothing in `app/`
   * read it, so reopening a project showed no sign of the selection at all --
   * the same "persisted but unreachable from the UI" defect that started this
   * line of work. This is the function the project panel renders.
   *
   * Both states are counted, and separately: a label summing them into one
   * number cannot tell "3 included" from "2 included, 1 excluded", which are
   * very different statements about a book.
   */
  it("says what selection a reopened project was generated with", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");

    expect(selectionLabel(detail)).toBe("Your selection: 1 you included, 1 you excluded");
  });

  it("says nothing about selection when the engine chose the whole book", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");

    expect(selectionLabel({ ...detail, overrides: {} })).toBeNull();
  });

  it("names only the state that is actually present", () => {
    const detail = fixture<ProjectDetail>("project-detail.json");

    expect(selectionLabel({ ...detail, overrides: { h1: "include", h2: "include" } })).toBe(
      "Your selection: 2 you included",
    );
    expect(selectionLabel({ ...detail, overrides: { h1: "exclude" } })).toBe(
      "Your selection: 1 you excluded",
    );
  });
});

/**
 * The override map crosses this boundary in both directions -- up on every
 * toggle and on generation, down inside `ProjectDetail` on reopen -- and the
 * hazard is the STATE SPELLING, not the keys (which are content hashes).
 * Rust pins the same file from its side in `commands.rs`.
 */
describe("photo override wire shape", () => {
  const overrides = fixture<PhotoOverrides>("photo-overrides.json");

  it("reads the states Rust writes", () => {
    expect(overrideFor(overrides, "a1b2c3d4")).toBe("include");
    expect(overrideFor(overrides, "e5f6a7b8")).toBe("exclude");
  });

  it("round-trips a decision through the map the webview sends back", () => {
    const next = withOverride(overrides, "c9d0e1f2", "exclude");
    expect(JSON.parse(JSON.stringify(next))).toEqual({
      a1b2c3d4: "include",
      e5f6a7b8: "exclude",
      c9d0e1f2: "exclude",
    });
  });
});

describe("override toggling", () => {
  it("records a decision without mutating the map it was given", () => {
    const before: PhotoOverrides = { a: "include" };
    const after = withOverride(before, "b", "exclude");

    expect(after).toEqual({ a: "include", b: "exclude" });
    expect(before).toEqual({ a: "include" }, "the input map must not be mutated");
  });

  /**
   * "auto" DELETES the key rather than storing it, mirroring Rust's
   * `Overrides::set`. Storing it would make two representations of "no
   * decision" -- and since the map is what gets persisted, one project would
   * come back with an `auto` row and another without, comparing unequal for
   * no reason.
   */
  it("returning a photo to auto erases the decision rather than storing it", () => {
    const after = withOverride({ a: "include", b: "exclude" }, "a", "auto");

    expect(after).toEqual({ b: "exclude" });
    expect("a" in after).toBe(false);
  });

  it("pressing the state a photo is already in returns it to auto", () => {
    expect(toggledOverride("include", "include")).toBe("auto");
    expect(toggledOverride("exclude", "exclude")).toBe("auto");
  });

  it("pressing the other state switches straight to it", () => {
    expect(toggledOverride("include", "exclude")).toBe("exclude");
    expect(toggledOverride("exclude", "include")).toBe("include");
    expect(toggledOverride("auto", "include")).toBe("include");
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

/**
 * `BookState` is what `useBook.ts` assigns into its refs whenever the
 * "current book" changes -- generating one in this session, or opening a
 * saved one from disk. This is the fix for the bug this whole feature exists
 * for: reopening a saved project used to be unreachable from the UI at all,
 * and the naive fix (adding an `open_project` call that sets its own ref
 * alongside the existing `generated` ref) would leave BOTH populated at
 * once, so a screen that later generated a fresh book would still export
 * whichever project id happened to be exported last. These functions are
 * the single place that decides which of the two "wins".
 */
describe("book state transitions", () => {
  const generated = fixture<GeneratedBook>("generated-book.json");
  const project = fixture<ProjectDetail>("project-detail.json");

  it("prefers the generated book over an opened project if a caller somehow leaves both set", () => {
    expect(
      resolveExportProjectId({ generated: { ...generated, projectId: 1 }, activeProject: { ...project, id: 2 } }),
    ).toBe(1);
  });

  it("has nothing to export before anything is generated or opened", () => {
    expect(initialBookState.generated).toBeNull();
    expect(initialBookState.activeProject).toBeNull();
    expect(resolveExportProjectId(initialBookState)).toBeNull();
  });

  it("generating a book targets that book for export", () => {
    const state = withGeneratedBook(initialBookState, generated);
    expect(resolveExportProjectId(state)).toBe(generated.projectId);
  });

  it("opening a project targets that project for export", () => {
    const state = withOpenedProject(project);
    expect(resolveExportProjectId(state)).toBe(project.id);
  });

  /**
   * The regression this whole file exists to prevent: opening a project
   * must not leave a PREVIOUSLY generated book's id (or its export
   * report / output folder) sitting alongside it. A `useBook` that just
   * set `activeProject` on `open_project` success -- without also clearing
   * `generated` -- would pass this project's OWN fixture-derived checks
   * above, since those never generate anything first.
   */
  it("opening a project clears whatever was generated in this session", () => {
    const generatedState = withGeneratedBook(initialBookState, generated);
    const opened = withOpenedProject({ ...project, id: 99 });
    expect(opened.generated).toBeNull();
    expect(opened.exportResult).toBeNull();
    expect(opened.outputDir).toBeNull();
    // Exporting after this transition targets the OPENED project (99), not
    // the generated book's own id.
    expect(resolveExportProjectId(opened)).toBe(99);
    expect(resolveExportProjectId(generatedState)).not.toBe(99);
  });

  it("generating a fresh book clears a previously opened project", () => {
    const openedState = withOpenedProject(project);
    const generated2 = withGeneratedBook(openedState, { ...generated, projectId: 42 });
    expect(generated2.activeProject).toBeNull();
    expect(generated2.exportResult).toBeNull();
    // Exporting after this transition targets the GENERATED book (42), not
    // the opened project's own id.
    expect(resolveExportProjectId(generated2)).toBe(42);
  });

  /**
   * `outputDir` is the one field that deliberately does NOT reset on every
   * `withGeneratedBook` call: regenerating (a different page length for the
   * SAME analysed folder) should not make the user re-pick where to export
   * to. `withOpenedProject`, in contrast, always clears it -- that path is a
   * genuine switch to a different book, so a folder chosen for whatever was
   * showing before must not be silently reused.
   */
  it("carries the output folder across a regenerate, but not across opening a different project", () => {
    const withDir: BookState = { ...initialBookState, outputDir: "/Users/jj/Desktop/export" };

    const regenerated = withGeneratedBook(withDir, { ...generated, projectId: 11 });
    expect(regenerated.outputDir).toBe("/Users/jj/Desktop/export");

    const opened = withOpenedProject(project);
    expect(opened.outputDir).toBeNull();
  });

  /**
   * The exact hazard called out in the task: switching between a loaded
   * project and a fresh analysis, in either order, repeatedly, must never
   * leave one project's fields mixed with another's. Simulated here as the
   * sequence `useBook` would actually produce -- each transition reads the
   * PREVIOUS state, exactly as `currentBookState()` feeds it in `useBook.ts`.
   */
  it("never mixes fields across repeated switches in either order", () => {
    let state = initialBookState;

    state = withGeneratedBook(state, { ...generated, projectId: 1 });
    expect(resolveExportProjectId(state)).toBe(1);

    state = withOpenedProject({ ...project, id: 2 });
    expect(resolveExportProjectId(state)).toBe(2);
    expect(state.generated).toBeNull();

    state = withOpenedProject({ ...project, id: 3 });
    expect(resolveExportProjectId(state)).toBe(3);

    state = withGeneratedBook(state, { ...generated, projectId: 4 });
    expect(resolveExportProjectId(state)).toBe(4);
    expect(state.activeProject).toBeNull();

    state = withGeneratedBook(state, { ...generated, projectId: 5 });
    expect(resolveExportProjectId(state)).toBe(5);
    expect(state.activeProject).toBeNull();
  });
});

describe("lastExportedOn", () => {
  const projects = fixture<ProjectListItem[]>("project-list.json");

  it("reads the date of the most recent export, in UTC so it is stable across machines", () => {
    expect(lastExportedOn(projects[0]!)).toBe("2025-08-13");
  });

  it("returns null for a project that has never been exported", () => {
    expect(lastExportedOn(projects[1]!)).toBeNull();
  });
});
