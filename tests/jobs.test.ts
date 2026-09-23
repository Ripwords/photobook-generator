import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { effectScope, nextTick, toRef } from "vue";
import type { AnalysisEvent, AnalysisSummary, AnalyzedPhoto } from "../app/types/features";
import type { PrintSpec } from "../app/types/printSpec";

/** A print size nobody would get by accident: square, 3 mm bleed. */
const SQUARE: PrintSpec = {
  pageWIn: 8.118,
  pageHIn: 8.236,
  bleedIn: 0.118,
  gutterIn: 0.3,
  safeMarginIn: 0.2,
  minDpi: 180,
  warnDpi: 260,
  coverWrapIn: 0.5,
};

/**
 * Analysis jobs live in a store rather than in the select screen, so leaving
 * the screen neither kills the run's progress nor throws the user's
 * decisions away, and two books can analyse at once.
 *
 * `analyze_folders` is mocked as a hand-driven run: each call records its
 * channel and waits until the test settles it, so a test decides exactly
 * which run's events arrive when.
 */
class FakeChannel<T> {
  onmessage: (message: T) => void = () => {};
}

interface PendingRun {
  folders: string[];
  channel: FakeChannel<AnalysisEvent>;
  finish: (summary: AnalysisSummary) => void;
  fail: (error: string) => void;
}

const runs: PendingRun[] = [];
/** The `drafts` table, as the Rust commands would keep it. */
const savedDrafts = new Map<number, string>();
/** When set, `list_drafts` waits for the test to release it. */
let holdList: Promise<void> | null = null;
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: FakeChannel }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createAnalysisJobs, jobProgress, jobRunId, parseSavedDraft } = await import(
  "../app/composables/useAnalysisJobs"
);
const { usePhotoOverrides } = await import("../app/composables/usePhotoOverrides");

function photo(hash: string): AnalyzedPhoto {
  return {
    status: "ok",
    path: `/p/${hash}.jpg`,
    hash,
    width: 4032,
    height: 3024,
    isUtility: false,
    faceCount: 0,
    sceneTags: [],
    aestheticPct: 50,
    sharpnessPct: 50,
    nearDupCluster: 1,
    eventCluster: 1,
    kept: true,
  };
}

function summary(runId: number, ...hashes: string[]): AnalysisSummary {
  return { runId, total: hashes.length, failed: 0, cached: 0, photos: hashes.map(photo) };
}

/** Lets the store's awaited `invoke` and its `finally` run. */
async function settle() {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
  await nextTick();
}

function forgotten(): number[] {
  return invoke.mock.calls
    .filter(([command]) => command === "forget_run")
    .map(([, args]) => (args as { runId: number }).runId);
}

describe("analysis jobs", () => {
  beforeEach(() => {
    runs.length = 0;
    savedDrafts.clear();
    holdList = null;
    invoke.mockReset();
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "analyze_folders") {
        return new Promise<AnalysisSummary>((resolve, reject) => {
          runs.push({
            folders: args.folders as string[],
            channel: args.onEvent as FakeChannel<AnalysisEvent>,
            finish: resolve,
            fail: (error) => reject(new Error(error)),
          });
        });
      }
      if (command === "apply_photo_overrides") return Promise.resolve(["/p/a.jpg"]);
      if (command === "list_drafts") {
        const rows = [...savedDrafts.values()];
        return (holdList ?? Promise.resolve()).then(() => rows);
      }
      if (command === "save_draft") {
        savedDrafts.set(args.id as number, args.json as string);
      }
      if (command === "delete_draft") savedDrafts.delete(args.id as number);
      return Promise.resolve();
    });
  });

  it("streams two jobs into their own state", async () => {
    const store = createAnalysisJobs();
    const a = store.startJob({ name: "Kyoto", folders: ["/k"] });
    const b = store.startJob({ name: "Bali", folders: ["/b"] });
    expect(runs.map((run) => run.folders)).toEqual([["/k"], ["/b"]]);

    runs[0]!.channel.onmessage({ kind: "scanned", total: 10 });
    runs[1]!.channel.onmessage({ kind: "scanned", total: 3 });
    runs[0]!.channel.onmessage({ kind: "batch", photos: [], analysed: 4, cached: 0, failed: 0 });

    expect(jobProgress(store.find(a)!)).toEqual({ processed: 4, total: 10 });
    expect(jobProgress(store.find(b)!)).toEqual({ processed: 0, total: 3 });

    runs[1]!.finish(summary(2, "b"));
    await settle();
    expect(store.find(b)!.running).toBe(false);
    expect(store.find(a)!.running).toBe(true);
    expect(jobRunId(store.find(b)!)).toBe(2);
    expect(jobRunId(store.find(a)!)).toBe(0);
  });

  it("falls back to the first folder's name when none is given", () => {
    const store = createAnalysisJobs();
    const id = store.startJob({ name: "   ", folders: ["/Pictures/Holiday 2026/"] });
    expect(store.find(id)!.name).toBe("Holiday 2026");
  });

  /**
   * The screen that edits a job's decisions comes and goes; the decisions
   * must not go with it. Mounting the overrides composable again on the same
   * job has to find them, and ask Rust to stamp them onto the set again.
   */
  it("keeps decisions across leaving the screen and coming back", async () => {
    const store = createAnalysisJobs();
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
    runs[0]!.finish(summary(7, "a", "b"));
    await settle();
    const job = store.find(id)!;
    const source = () => toRef(() => job.stream.summary?.photos ?? []);
    const runId = toRef(() => jobRunId(job));

    const first = effectScope();
    const screen = first.run(() => usePhotoOverrides(source(), runId, toRef(job, "overrides")))!;
    await screen.setOverride("b", "exclude");
    first.stop();

    invoke.mockClear();
    const second = effectScope();
    const again = second.run(() => usePhotoOverrides(source(), runId, toRef(job, "overrides")))!;
    await settle();

    expect(again.overrides.value).toEqual({ b: "exclude" });
    expect(invoke).toHaveBeenCalledWith("apply_photo_overrides", {
      runId: 7,
      overrides: { b: "exclude" },
    });
    expect(again.photos.value.map((p) => [p.hash, p.kept])).toEqual([
      ["a", true],
      ["b", false],
    ]);
    second.stop();
  });

  it("restores a saved book's decisions once its analysis is done, not before", async () => {
    const store = createAnalysisJobs();
    const id = store.startJob({
      name: "Kyoto",
      folders: ["/k"],
      replacing: { id: 4, name: "Kyoto" },
      restoreOverrides: { a: "include" },
    });
    expect(store.find(id)!.overrides).toEqual({});

    runs[0]!.finish(summary(1, "a"));
    await settle();
    expect(store.find(id)!.overrides).toEqual({ a: "include" });
  });

  /**
   * Decisions are keyed by content hash, so carrying them into a different
   * set would apply one folder's decision to an identical file in another.
   * The run they were made against is dropped from Rust's cache too.
   */
  it("starts a new run of a job with no decisions and forgets the old run", async () => {
    const store = createAnalysisJobs();
    const id = store.startJob({
      name: "Kyoto",
      folders: ["/k"],
      replacing: { id: 4, name: "Kyoto" },
      restoreOverrides: { a: "include" },
    });
    runs[0]!.finish(summary(1, "a"));
    await settle();

    store.changeFolders(id, ["/elsewhere"]);
    const job = store.find(id)!;
    expect(job.overrides).toEqual({});
    expect(job.replacing).toBeNull();
    expect(forgotten()).toEqual([1]);

    runs[1]!.finish(summary(2, "a"));
    await settle();
    expect(job.overrides).toEqual({}, "a different set does not restore the old book's decisions");
  });

  it("forgets a removed job's run", async () => {
    const store = createAnalysisJobs();
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
    runs[0]!.finish(summary(5, "a"));
    await settle();

    store.remove(id);
    expect(store.find(id)).toBeUndefined();
    expect(forgotten()).toEqual([5]);
  });

  /**
   * Rust cannot be told to stop a gather part-way, so a job discarded while
   * it runs still finishes there. Its result must be forgotten when it
   * arrives, and its late events must not bring it back.
   */
  it("forgets a job removed mid-run once its run finishes", async () => {
    const store = createAnalysisJobs();
    const settled = vi.fn();
    store.onSettled(settled);
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
    store.remove(id);
    expect(forgotten()).toEqual([]);

    runs[0]!.channel.onmessage({ kind: "scanned", total: 9 });
    runs[0]!.finish(summary(6, "a"));
    await settle();

    expect(store.jobs.value).toEqual([]);
    expect(forgotten()).toEqual([6]);
    expect(settled).not.toHaveBeenCalled();
  });

  it("ignores a superseded run of the same job", async () => {
    const store = createAnalysisJobs();
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
    store.retry(id);

    runs[0]!.channel.onmessage({ kind: "scanned", total: 99 });
    runs[0]!.finish(summary(1, "old"));
    await settle();
    const job = store.find(id)!;
    expect(job.running).toBe(true);
    expect(job.stream.scannedTotal).toBe(0);
    expect(forgotten()).toEqual([1]);

    runs[1]!.finish(summary(2, "new"));
    await settle();
    expect(job.running).toBe(false);
    expect(job.stream.summary?.photos.map((p) => p.hash)).toEqual(["new"]);
  });

  it("reports a failed run on its job and tells listeners it settled", async () => {
    const store = createAnalysisJobs();
    const settled = vi.fn();
    store.onSettled(settled);
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });

    runs[0]!.fail("the folder is gone");
    await settle();

    expect(store.find(id)!.error).toContain("the folder is gone");
    expect(store.find(id)!.running).toBe(false);
    expect(settled).toHaveBeenCalledTimes(1);
    expect(settled.mock.calls[0]![0].id).toBe(id);
  });

  it("finds the draft already editing a saved book", () => {
    const store = createAnalysisJobs();
    store.startJob({ name: "New", folders: ["/n"] });
    const id = store.startJob({
      name: "Kyoto",
      folders: ["/k"],
      replacing: { id: 4, name: "Kyoto" },
    });

    expect(store.jobForProject(4)?.id).toBe(id);
    expect(store.jobForProject(5)).toBeUndefined();
  });

  it("renames a job", () => {
    const store = createAnalysisJobs();
    const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
    store.rename(id, "Kyoto 2026");
    expect(store.find(id)!.name).toBe("Kyoto 2026");
  });

  describe("saved across restarts", () => {
    function savedNames(): string[] {
      return [...savedDrafts.values()].map((json) => (JSON.parse(json) as { name: string }).name);
    }

    it("saves a draft when it starts and when it is renamed, and deletes it when removed", async () => {
      const store = createAnalysisJobs();
      await store.restoreDrafts();
      const id = store.startJob({ name: "Kyoto", folders: ["/k"] });
      await settle();
      expect(savedNames()).toEqual(["Kyoto"]);

      store.rename(id, "Kyoto 2026");
      await settle();
      expect(savedNames()).toEqual(["Kyoto 2026"]);

      store.remove(id);
      await settle();
      expect(savedDrafts.size).toBe(0);
    });

    it("brings a draft back after a restart, analyses it again and restores the decisions", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      const id = before.startJob({ name: "Kyoto", folders: ["/k", "/k2"] });
      runs[0]!.finish(summary(1, "a", "b"));
      await settle();
      before.find(id)!.overrides = { b: "exclude" };
      await settle();

      const after = createAnalysisJobs();
      await after.restoreDrafts();
      expect(after.jobs.value.map((job) => [job.name, job.folders])).toEqual([["Kyoto", ["/k", "/k2"]]]);
      expect(runs.at(-1)!.folders).toEqual(["/k", "/k2"]);

      runs.at(-1)!.finish(summary(2, "a", "b"));
      await settle();
      expect(after.jobs.value[0]!.overrides).toEqual({ b: "exclude" });
    });

    it("brings a draft's tier choices back after a restart, the same way as its overrides", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      const id = before.startJob({ name: "Kyoto", folders: ["/k", "/k2"] });
      runs[0]!.finish(summary(1, "a", "b"));
      await settle();
      before.find(id)!.tiers = { h1: "featured" };
      await settle();

      const after = createAnalysisJobs();
      await after.restoreDrafts();
      runs.at(-1)!.finish(summary(2, "a", "b"));
      await settle();
      expect(after.jobs.value[0]!.tiers).toEqual({ h1: "featured" });
    });

    it("keeps a saved book's decisions for a draft quit before its analysis finished", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      before.startJob({
        name: "Kyoto",
        folders: ["/k"],
        replacing: { id: 4, name: "Kyoto" },
        restoreOverrides: { a: "include" },
      });
      await settle();

      const after = createAnalysisJobs();
      await after.restoreDrafts();
      runs.at(-1)!.finish(summary(2, "a"));
      await settle();

      const job = after.jobs.value[0]!;
      expect(job.replacing).toEqual({ id: 4, name: "Kyoto" });
      expect(job.overrides).toEqual({ a: "include" });
    });

    it("does not reuse the id of a draft started while the saved ones were loading", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      before.startJob({ name: "Saved", folders: ["/s"] });
      await settle();

      const held = Promise.withResolvers<void>();
      holdList = held.promise;
      const after = createAnalysisJobs();
      const restoring = after.restoreDrafts();
      after.startJob({ name: "New", folders: ["/n"] });
      held.resolve();
      await restoring;
      await settle();

      const ids = after.jobs.value.map((job) => job.id);
      expect(new Set(ids).size).toBe(2);
      expect(savedNames().toSorted()).toEqual(["New", "Saved"]);
    });

    it("brings a draft's print size back after a restart", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      const id = before.startJob({ name: "Square", folders: ["/s"] });
      before.setSpec(id, SQUARE);
      await settle();

      const after = createAnalysisJobs();
      await after.restoreDrafts();
      expect(after.jobs.value[0]!.spec).toStrictEqual(SQUARE);
    });

    it("keeps a re-edited book's print size on its draft", () => {
      const store = createAnalysisJobs();
      const id = store.startJob({ name: "Kyoto", folders: ["/k"], replacing: { id: 4, name: "Kyoto" }, spec: SQUARE });
      expect(store.find(id)!.spec).toStrictEqual(SQUARE);
    });

    it("reads a draft saved before print sizes existed as the default size, not as unreadable", async () => {
      savedDrafts.set(3, JSON.stringify({ id: 3, name: "Old", folders: ["/f"], replacing: null, overrides: {} }));
      savedDrafts.set(4, JSON.stringify({ id: 4, name: "Torn", folders: ["/f"], replacing: null, overrides: {}, spec: { pageWIn: 6 } }));

      const store = createAnalysisJobs();
      await store.restoreDrafts();

      expect(store.jobs.value.map((job) => [job.name, job.spec])).toEqual([
        ["Old", null],
        ["Torn", null],
      ]);
    });

    it("brings a draft's Places option back after a restart", async () => {
      const before = createAnalysisJobs();
      await before.restoreDrafts();
      const id = before.startJob({ name: "Kansai", folders: ["/k"] });
      before.setOptions(id, { places: true, featuredFloor: 6, briefCap: 2 });
      await settle();

      const after = createAnalysisJobs();
      await after.restoreDrafts();
      expect(after.jobs.value[0]!.options).toStrictEqual({ places: true, featuredFloor: 6, briefCap: 2 });
    });

    it("keeps a re-edited book's options on its draft", () => {
      const store = createAnalysisJobs();
      const id = store.startJob({
        name: "Kansai",
        folders: ["/k"],
        replacing: { id: 4, name: "Kansai" },
        options: { places: true, featuredFloor: 6, briefCap: 2 },
      });
      expect(store.find(id)!.options).toStrictEqual({ places: true, featuredFloor: 6, briefCap: 2 });
    });

    it("reads a draft saved before options existed, or with torn ones, with every option at its default", async () => {
      savedDrafts.set(3, JSON.stringify({ id: 3, name: "Old", folders: ["/f"], replacing: null, overrides: {} }));
      savedDrafts.set(4, JSON.stringify({ id: 4, name: "Torn", folders: ["/f"], replacing: null, overrides: {}, options: { places: "yes" } }));
      savedDrafts.set(5, JSON.stringify({ id: 5, name: "On", folders: ["/f"], replacing: null, overrides: {}, options: { places: true } }));

      const store = createAnalysisJobs();
      await store.restoreDrafts();

      expect(store.jobs.value.map((job) => [job.name, job.options])).toEqual([
        ["Old", { places: false, featuredFloor: 6, briefCap: 2 }],
        ["Torn", { places: false, featuredFloor: 6, briefCap: 2 }],
        ["On", { places: true, featuredFloor: 6, briefCap: 2 }],
      ]);
    });

    /**
     * A spec saved before the cover wrap existed has seven keys. Rust loads
     * it with Pixajoy's wrap; the draft reader must agree, and the shared
     * fixture is what keeps the two defaults the same number.
     */
    it("reads a print size saved before the cover wrap existed with the engine's default wrap", async () => {
      const fixture = JSON.parse(
        readFileSync(fileURLToPath(new URL("./fixtures/wire/legacy-print-spec.json", import.meta.url)), "utf8"),
      ) as { legacy: unknown; loadsAs: PrintSpec };
      savedDrafts.set(5, JSON.stringify({ id: 5, name: "Before covers", folders: ["/f"], replacing: null, overrides: {}, spec: fixture.legacy }));

      const store = createAnalysisJobs();
      await store.restoreDrafts();

      expect(store.jobs.value[0]!.spec).toStrictEqual(fixture.loadsAs);
    });

    it("skips a saved draft it cannot read", async () => {
      savedDrafts.set(1, "not json");
      savedDrafts.set(2, JSON.stringify({ id: 2, name: "No folders" }));
      savedDrafts.set(3, JSON.stringify({ id: 3, name: "Fine", folders: ["/f"], replacing: null, overrides: {} }));

      const store = createAnalysisJobs();
      await store.restoreDrafts();

      expect(store.jobs.value.map((job) => job.name)).toEqual(["Fine"]);
    });
  });
});

describe("parseSavedDraft options", () => {
  const base = { id: 1, name: "Trip", folders: ["/a"] };
  it("gives a draft saved before tiers the default floor and cap", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, options: { places: true } }));
    expect(draft?.options).toEqual({ places: true, featuredFloor: 6, briefCap: 2 });
  });
  it("replaces an out-of-range floor or cap with its default", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, options: { featuredFloor: 40, briefCap: 0 } }));
    expect(draft?.options).toEqual({ places: false, featuredFloor: 6, briefCap: 2 });
  });
});

describe("parseSavedDraft tiers", () => {
  const base = { id: 1, name: "Trip", folders: ["/a"] };
  it("restores the user's tier choices", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, tiers: { h1: "featured", h2: "skipped" } }));
    expect(draft?.tiers).toEqual({ h1: "featured", h2: "skipped" });
  });
  it("gives a draft saved before tiers an empty map", () => {
    expect(parseSavedDraft(JSON.stringify(base))?.tiers).toEqual({});
  });
  it("fails the load on an unknown tier token (spec §6)", () => {
    expect(parseSavedDraft(JSON.stringify({ ...base, tiers: { h1: "hero" } }))).toBeNull();
  });
});
