import { beforeEach, describe, expect, it, vi } from "vitest";
import { effectScope, nextTick, toRef } from "vue";
import type { AnalysisEvent, AnalysisSummary, AnalyzedPhoto } from "../app/types/features";

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
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: FakeChannel }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createAnalysisJobs, jobProgress, jobRunId } = await import(
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
});
