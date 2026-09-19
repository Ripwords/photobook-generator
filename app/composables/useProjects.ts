import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
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

  /** Runs `work`, reporting a failure through `error`. True if it succeeded. */
  async function guard(work: () => Promise<void>): Promise<boolean> {
    busy.value = true;
    error.value = null;
    try {
      await work();
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
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
   * Moves a saved project to the trash and refreshes the list. True if it
   * went, which is when the caller offers Undo (`restoreProject`); the
   * trash is emptied of books deleted over 30 days ago. Exported files on
   * disk and the shared analysis cache are untouched; see the
   * `delete_project` command on the Rust side.
   */
  async function deleteProject(id: number): Promise<boolean> {
    return guard(async () => {
      await invoke<void>("delete_project", { id });
      await refresh();
    });
  }

  /** Takes a deleted project back out of the trash and refreshes the list. */
  async function restoreProject(id: number) {
    await guard(async () => {
      await invoke<void>("restore_project", { id });
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

  /** Stars or unstars a saved project and refreshes the list. True if it took. */
  async function setFavourite(id: number, favourite: boolean): Promise<boolean> {
    return guard(async () => {
      await invoke<void>("set_favourite", { id, favourite });
      await refresh();
    });
  }

  /** Selects `path` in Finder. Changes nothing, so a failure only sets `error`. */
  async function reveal(path: string) {
    await guard(() => invoke<void>("reveal_in_finder", { path }));
  }

  return {
    projects,
    busy,
    error,
    refresh,
    reload,
    deleteProject,
    restoreProject,
    renameProject,
    setFavourite,
    reveal,
  };
}
