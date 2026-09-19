/**
 * What the app knows about a newer build of itself, and the one place that
 * knowledge changes.
 *
 * A check is either silent or explicit, and that is the whole reason this is a
 * state machine rather than a handful of refs. The silent check is the one
 * `app.vue` fires on mount. Offline, behind a captive portal, or before any
 * release exists, it must land back on `idle` and say nothing. The explicit
 * check is the one the user pressed a button for, so it owes them an answer
 * either way. An update that was actually found is never suppressed.
 */

export type UpdateInfo = { version: string; notes: string | null };

export type UpdaterState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "uptodate" }
  | { phase: "available"; update: UpdateInfo }
  | { phase: "downloading"; percent: number | null }
  | { phase: "installing" }
  | { phase: "error"; message: string };

export type UpdaterEvent =
  | { type: "check"; silent: boolean }
  | { type: "checked"; update: UpdateInfo | null; silent: boolean }
  | { type: "download" }
  | { type: "progress"; downloaded: number; total: number | null }
  | { type: "installed" }
  | { type: "failed"; message: string; silent: boolean }
  | { type: "dismiss" };

/**
 * `downloaded` is the running total the caller has accumulated, not this
 * chunk's length. A `total` of 0 is as unusable as a missing one, so the same
 * falsy test covers both and there is no divide by zero to guard separately.
 */
function percentOf(downloaded: number, total: number | null): number | null {
  if (!total) return null;
  return Math.min(100, Math.max(0, Math.round((downloaded / total) * 100)));
}

/**
 * A total mapping from event to state. It never reads `state`, which is sound
 * because `useUpdater` is the only thing that dispatches and it emits in
 * order: nothing can arrive that needs the previous phase to interpret it.
 * The parameter stays for the shape every reducer has, and so a later event
 * that does need history has somewhere to read it from.
 */
export function updaterReducer(state: UpdaterState, event: UpdaterEvent): UpdaterState {
  switch (event.type) {
    case "check":
      return { phase: "checking" };
    case "checked":
      if (event.update) return { phase: "available", update: event.update };
      return event.silent ? { phase: "idle" } : { phase: "uptodate" };
    case "download":
      return { phase: "downloading", percent: null };
    case "progress":
      return { phase: "downloading", percent: percentOf(event.downloaded, event.total) };
    case "installed":
      return { phase: "installing" };
    case "failed":
      return event.silent ? { phase: "idle" } : { phase: "error", message: event.message };
    case "dismiss":
      return { phase: "idle" };
  }
}

/** Tauri rejects with a plain string, not an `Error`, so both shapes are read. */
export function updaterMessage(error: unknown, fallback: string): string {
  if (typeof error === "string") return error.trim() || fallback;
  if (error instanceof Error && error.message) return error.message;
  return fallback;
}
