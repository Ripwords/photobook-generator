import { invoke } from "@tauri-apps/api/core";
import type { ProjectListItem } from "~/types/book";

/**
 * The list of saved projects: every book that has ever been generated,
 * newest-updated first. This is the one place `list_projects`,
 * `delete_project` and `rename_project` are called from -- the library screen
 * and `useBook` both read from an instance of this rather than each keeping
 * their own copy of the same invoke calls.
 *
 * `busy`/`error` cover ONLY the calls made here (delete, rename, refresh) --
 * `useBook`'s own `busy`/`error` are a separate flag for generate/open/export,
 * and `useBook.deleteProject`/`renameProject` wrap the functions below in
 * their OWN `guard` too, so the state hazard of deleting or renaming the
 * currently open project is handled there, not here. This composable knows
 * nothing about `activeProject` or `generated` on purpose: it is the CRUD
 * layer, not the one that decides what a delete does to the book on screen.
 */
export function useProjects() {
  const projects = ref<ProjectListItem[]>([]);
  const busy = ref(false);
  const error = ref<string | null>(null);

  async function refresh() {
    projects.value = await invoke<ProjectListItem[]>("list_projects");
  }

  async function guard(work: () => Promise<void>) {
    busy.value = true;
    error.value = null;
    try {
      await work();
    } catch (e) {
      error.value = String(e);
    } finally {
      busy.value = false;
    }
  }

  /**
   * Refreshes the list, reporting a failure through `error` instead of
   * throwing.
   *
   * `refresh` above stays raw on purpose: `useBook` calls it from inside its
   * OWN `guard` after generating, deleting and exporting, and a swallowed
   * exception there would report those as successes. Callers that only want
   * the list, and have nowhere to catch, use this one -- otherwise a failing
   * `list_projects` is an unhandled rejection that leaves a stale list on
   * screen with nothing said.
   */
  async function reload() {
    await guard(refresh);
  }

  /**
   * Deletes a saved project and refreshes the list. Irreversible: the
   * project's layout, include/exclude decisions and export history are gone
   * from the app, though the confirmation the caller shows before this runs
   * is what actually explains that -- this function does not gate on
   * anything itself. Exported files on disk and the shared analysis cache are
   * untouched; see `Db::delete_project`'s doc comment on the Rust side.
   */
  async function deleteProject(id: number) {
    await guard(async () => {
      await invoke<void>("delete_project", { id });
      await refresh();
    });
  }

  /** Renames a saved project and refreshes the list. */
  async function renameProject(id: number, name: string) {
    await guard(async () => {
      await invoke<void>("rename_project", { id, name });
      await refresh();
    });
  }

  return { projects, busy, error, refresh, reload, deleteProject, renameProject };
}
