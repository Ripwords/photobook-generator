import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
// Imported explicitly rather than left to Nuxt's auto-imports so this file
// runs under plain vitest -- see `tests/jobs.test.ts`.
import { reactive, ref, watch } from "vue";
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
import type { PrintSpec } from "~/types/printSpec";

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
  /**
   * The print size the book will be generated at. `null` is the app's
   * default, which only Rust knows -- see `default_print_spec`.
   */
  spec: PrintSpec | null;
}

export interface NewJob {
  /** Blank falls back to the first folder's name. */
  name: string;
  folders: string[];
  replacing?: ReplacedProject | null;
  restoreOverrides?: PhotoOverrides;
  spec?: PrintSpec | null;
}

/**
 * What is saved of a draft so it survives a restart. Not its photos: those
 * come back from the analysis cache when the draft is analysed again.
 */
export interface SavedDraft {
  id: number;
  name: string;
  folders: string[];
  replacing: ReplacedProject | null;
  /** The decisions to apply once the draft is analysed again. */
  overrides: PhotoOverrides;
  spec: PrintSpec | null;
}

/**
 * A job as it is saved. Until its analysis is done, its decisions are still
 * the ones waiting to be applied -- those of the book it re-edits, if any.
 */
export function savedDraft(job: AnalysisJob): SavedDraft {
  const analysed = !job.running && job.stream.summary !== null;
  return {
    id: job.id,
    name: job.name,
    folders: [...job.folders],
    replacing: job.replacing ? { ...job.replacing } : null,
    overrides: { ...(analysed ? job.overrides : job.restoreOverrides) },
    spec: job.spec ? { ...job.spec } : null,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const SPEC_KEYS = ["pageWIn", "pageHIn", "bleedIn", "gutterIn", "safeMarginIn", "minDpi", "warnDpi"] as const;

/**
 * A saved print size, or `null` for the default. Shape only: Rust validates
 * it again when the book is generated, and refuses one that makes no sense.
 */
function parseSpec(value: unknown): PrintSpec | null {
  if (!isRecord(value)) return null;
  const spec = {} as PrintSpec;
  for (const key of SPEC_KEYS) {
    const n = value[key];
    if (typeof n !== "number" || !Number.isFinite(n)) return null;
    spec[key] = n;
  }
  return spec;
}

/**
 * Reads a saved draft back, or `null` for one this version cannot use. A
 * draft saved before print sizes existed, or with a torn one, is still a
 * draft: it gets the default size rather than being thrown away.
 */
export function parseSavedDraft(json: string): SavedDraft | null {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return null;
  }
  if (!isRecord(value)) return null;
  const { id, name, folders, replacing, overrides, spec } = value;
  if (typeof id !== "number" || typeof name !== "string") return null;
  if (!Array.isArray(folders) || folders.length === 0) return null;
  if (!folders.every((folder) => typeof folder === "string")) return null;
  const replaced =
    isRecord(replacing) && typeof replacing.id === "number" && typeof replacing.name === "string"
      ? { id: replacing.id, name: replacing.name }
      : null;
  const decisions: PhotoOverrides = {};
  if (isRecord(overrides)) {
    for (const [hash, state] of Object.entries(overrides)) {
      if (state === "include" || state === "exclude") decisions[hash] = state;
    }
  }
  return { id, name, folders, replacing: replaced, overrides: decisions, spec: parseSpec(spec) };
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
    return addJob(options, nextId++);
  }

  function addJob(options: NewJob, id: number): number {
    const job = reactive<AnalysisJob>({
      id,
      name: options.name.trim() || defaultProjectName(options.folders[0] ?? ""),
      folders: [...options.folders],
      replacing: options.replacing ?? null,
      restoreOverrides: { ...options.restoreOverrides },
      stream: initialStreamState,
      running: true,
      error: null,
      overrides: {},
      spec: options.spec ?? null,
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

  function setSpec(id: number, spec: PrintSpec | null) {
    const job = find(id);
    if (job) job.spec = spec ? { ...spec } : null;
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

  /** Each saved draft's JSON, as last written, by id. */
  const saved = new Map<number, string>();
  /** Writes go one after another, so a later save cannot land before an earlier one. */
  let writes: Promise<unknown> = Promise.resolve();

  function write(command: string, args: Record<string, unknown>) {
    writes = writes
      .then(() => invoke(command, args))
      .catch((error: unknown) => console.warn(`${command} failed`, error));
  }

  /** Saves what changed since the last save, and deletes what is gone. */
  function syncSaved() {
    const current = new Map(jobs.value.map((job) => [job.id, JSON.stringify(savedDraft(job))]));
    for (const [id, json] of current) {
      if (saved.get(id) !== json) write("save_draft", { id, json });
    }
    for (const id of saved.keys()) {
      if (!current.has(id)) write("delete_draft", { id });
    }
    saved.clear();
    for (const [id, json] of current) saved.set(id, json);
  }

  /**
   * Brings back the drafts saved when the app last quit, analyses each again,
   * and from then on saves every change. Their photos come from the analysis
   * cache, so a draft whose folders were analysed comes back in moments.
   */
  async function restoreDrafts() {
    let rows: string[] = [];
    try {
      rows = await invoke<string[]>("list_drafts");
    } catch (error) {
      console.warn("could not load saved drafts", error);
    }
    for (const json of rows) {
      const draft = parseSavedDraft(json);
      if (!draft) continue;
      saved.set(draft.id, json);
      // Ids are this session's, so a draft started while these loaded
      // cannot collide with one of them. The old row is deleted and the
      // draft saved again under its new id.
      nextId = Math.max(nextId, draft.id + 1);
      const reused = jobs.value.some((job) => job.id === draft.id);
      addJob(
        {
          name: draft.name,
          folders: draft.folders,
          replacing: draft.replacing,
          restoreOverrides: draft.overrides,
          spec: draft.spec,
        },
        reused ? nextId++ : draft.id,
      );
    }
    watch(() => jobs.value.map((job) => JSON.stringify(savedDraft(job))), syncSaved, {
      immediate: true,
    });
  }

  /** Waits for every save so far to be written. */
  async function flushDrafts() {
    syncSaved();
    await writes;
  }

  return {
    jobs,
    find,
    startJob,
    retry,
    changeFolders,
    rename,
    setSpec,
    remove,
    jobForProject,
    onSettled,
    restoreDrafts,
    flushDrafts,
  };
}

export type AnalysisJobs = ReturnType<typeof createAnalysisJobs>;

let shared: AnalysisJobs | undefined;

/** The app's one job store. The app is a single-window SPA, so module state is the store. */
export function useAnalysisJobs(): AnalysisJobs {
  shared ??= createAnalysisJobs();
  return shared;
}
