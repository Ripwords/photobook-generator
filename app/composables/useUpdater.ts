import type { Update } from "@tauri-apps/plugin-updater";
import { type UpdaterEvent, type UpdaterState, updaterMessage, updaterReducer } from "~/types/updater";

/**
 * Finding, downloading and installing a newer build of the app.
 *
 * Every phase change goes through `updaterReducer`; nothing here decides one.
 * A check carries whether it was silent, which is the only subtle part: the
 * one `app.vue` fires on mount must end back at `idle` when it fails or finds
 * nothing, so being offline never nags, while a check the user pressed for
 * owes them an answer either way.
 */

// Module scope so the startup check and the Settings panel share one handle:
// the check that finds the update is not the call that installs it.
let pending: Update | null = null;

export function useUpdater() {
  const state = useState<UpdaterState>("updater", () => ({ phase: "idle" }));

  function dispatch(event: UpdaterEvent) {
    state.value = updaterReducer(state.value, event);
  }

  async function check(silent: boolean) {
    dispatch({ type: "check", silent });
    try {
      const { check: checkForUpdate } = await import("@tauri-apps/plugin-updater");
      pending = await checkForUpdate();
      dispatch({
        type: "checked",
        update: pending ? { version: pending.version, notes: pending.body ?? null } : null,
        silent,
      });
    } catch (e) {
      pending = null;
      dispatch({ type: "failed", message: updaterMessage(e, "Could not check for updates."), silent });
    }
  }

  async function install() {
    if (!pending) return;
    dispatch({ type: "download" });
    let downloaded = 0;
    let total: number | null = null;
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? null;
        else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          dispatch({ type: "progress", downloaded, total });
        }
      });
      dispatch({ type: "installed" });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      dispatch({
        type: "failed",
        message: updaterMessage(e, "Could not install the update."),
        silent: false,
      });
    }
  }

  function dismiss() {
    dispatch({ type: "dismiss" });
  }

  return { state, check, install, dismiss };
}
