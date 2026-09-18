import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
// Imported explicitly rather than left to Nuxt's auto-imports so this file
// runs under plain vitest -- see `tests/jobs.test.ts`.
import { reactive, ref } from "vue";
import { defaultProjectName } from "~/types/book";
import {
  applyAnalysisEvent,
  initialStreamState,
  type AnalysisEvent,
  type AnalysisSummary,
  type PhotoOverrides,
  type StreamState,
} from "~/types/features";
import type { ReplacedProject } from "~/types/navigation";

/**
 * One book in the making: the folders being analysed, what the analysis has
 * produced so far, and the user's own decisions over it.
 *
 * It lives in a store, not in the select screen, because the screen comes and
 * goes. A job keeps streaming while the user is in the library or another
 * book, and its decisions are waiting when they come back.
 */
export interface AnalysisJob {
  readonly id: number;
  name: string;
  /** The folders analysed, first picked first. Their union is one set. */
  folders: string[];
  /** The saved book this draft re-edits and can be generated back over. */
  replacing: ReplacedProject | null;
  /** That book's saved decisions, applied once its analysis is done. */
  restoreOverrides: PhotoOverrides;
  /** The streamed run, accumulated by `applyAnalysisEvent`. */
  stream: StreamState;
  running: boolean;
  error: string | null;
  /** The user's include/exclude decisions about the current run's photos. */
  overrides: PhotoOverrides;
}

export interface NewJob {
  /** Blank falls back to the first folder's name. */
  name: string;
  folders: string[];
  replacing?: ReplacedProject | null;
  restoreOverrides?: PhotoOverrides;
}

/** The run a job's photos came from -- see `AnalysisSummary.runId`. `0` until it is done. */
export function jobRunId(job: AnalysisJob): number {
  return job.stream.summary?.runId ?? 0;
}

/** Photos accounted for so far (analysed, cached or failed), out of the scanned total. */
export function jobProgress(job: AnalysisJob): { processed: number; total: number } {
  const { analysed, cached, failed, scannedTotal } = job.stream;
  return { processed: analysed + cached + failed, total: scannedTotal };
}

/** Opens the folder picker and returns what was chosen, without analysing it. */
export async function pickFolders(): Promise<string[]> {
  const picked = await open({ directory: true, multiple: true });
  return typeof picked === "string" ? [picked] : (picked ?? []);
}

/**
 * Rust keeps every finished run's photos so overrides can be judged without
 * sending them back. A run no job will ask about again is dropped. A failure
 * here costs only memory until the app quits, so it is logged and not shown.
 */
function forgetRun(runId: number) {
  if (runId === 0) return;
  invoke("forget_run", { runId }).catch((error: unknown) => {
    console.warn(`could not forget analysis run ${runId}`, error);
  });
}

export function createAnalysisJobs() {
  const jobs = ref<AnalysisJob[]>([]);
  let nextId = 1;
  /** Bumped per run of a job, so a superseded run's late events are ignored. */
  const attempts = new Map<number, number>();
  const settledListeners = new Set<(job: AnalysisJob) => void>();

  function find(id: number): AnalysisJob | undefined {
    return jobs.value.find((job) => job.id === id);
  }

  async function run(job: AnalysisJob) {
    const attempt = (attempts.get(job.id) ?? 0) + 1;
    attempts.set(job.id, attempt);
    const current = () => attempts.get(job.id) === attempt && find(job.id) === job;

    forgetRun(jobRunId(job));
    job.running = true;
    job.error = null;
    job.stream = initialStreamState;
    // A new run is a new set of photos, and a decision keyed by content hash
    // must not carry into it.
    job.overrides = {};

    // A `Channel`, not Tauri's event system, which is JSON-string-only and
    // not built for this volume.
    const onEvent = new Channel<AnalysisEvent>();
    // oxlint-disable-next-line unicorn/prefer-add-event-listener -- not an EventTarget
    onEvent.onmessage = (event) => {
      if (current()) job.stream = applyAnalysisEvent(job.stream, event);
    };

    try {
      // The return value, not the `Done` event, is the authoritative summary:
      // Rust swallows a failed send, and a large `Done` can land after this
      // resolves. The reducer's `done` case is idempotent for that reason.
      const summary = await invoke<AnalysisSummary>("analyze_folders", {
        folders: [...job.folders],
        onEvent,
      });
      if (!current()) {
        forgetRun(summary.runId);
        return;
      }
      job.stream = applyAnalysisEvent(job.stream, { kind: "done", summary });
      if (Object.keys(job.restoreOverrides).length > 0) {
        job.overrides = { ...job.restoreOverrides };
      }
    } catch (error) {
      if (current()) job.error = String(error);
    }
    if (!current()) return;
    job.running = false;
    for (const listener of settledListeners) listener(job);
  }

  /** Adds a job and starts analysing it. Returns its id. */
  function startJob(options: NewJob): number {
    const job = reactive<AnalysisJob>({
      id: nextId++,
      name: options.name.trim() || defaultProjectName(options.folders[0] ?? ""),
      folders: [...options.folders],
      replacing: options.replacing ?? null,
      restoreOverrides: { ...options.restoreOverrides },
      stream: initialStreamState,
      running: true,
      error: null,
      overrides: {},
    }) as AnalysisJob;
    jobs.value.push(job);
    void run(job);
    return job.id;
  }

  function retry(id: number) {
    const job = find(id);
    if (job) void run(job);
  }

  /**
   * Analyses other folders in place of the job's. A book built from other
   * folders is not an update of the one it came from, so it stops replacing
   * it, and that book's decisions no longer apply.
   */
  function changeFolders(id: number, folders: string[]) {
    const job = find(id);
    if (!job || folders.length === 0) return;
    if (folders.join("\n") !== job.folders.join("\n")) {
      job.replacing = null;
      job.restoreOverrides = {};
    }
    job.folders = [...folders];
    void run(job);
  }

  function rename(id: number, name: string) {
    const job = find(id);
    if (job) job.name = name;
  }

  /**
   * Drops a job, discarded or generated. A running job's result is forgotten
   * when it arrives, since Rust cannot be stopped part-way through a gather.
   */
  function remove(id: number) {
    const job = find(id);
    if (!job) return;
    jobs.value = jobs.value.filter((other) => other !== job);
    if (!job.running) forgetRun(jobRunId(job));
  }

  /** The draft already re-editing a saved book, so "Edit photos" twice opens one draft. */
  function jobForProject(projectId: number): AnalysisJob | undefined {
    return jobs.value.find((job) => job.replacing?.id === projectId);
  }

  /** Called once each time a job's run finishes, succeeded or failed. */
  function onSettled(listener: (job: AnalysisJob) => void): () => void {
    settledListeners.add(listener);
    return () => settledListeners.delete(listener);
  }

  return { jobs, find, startJob, retry, changeFolders, rename, remove, jobForProject, onSettled };
}

export type AnalysisJobs = ReturnType<typeof createAnalysisJobs>;

let shared: AnalysisJobs | undefined;

/** The app's one job store. The app is a single-window SPA, so module state is the store. */
export function useAnalysisJobs(): AnalysisJobs {
  shared ??= createAnalysisJobs();
  return shared;
}
