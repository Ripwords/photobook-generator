import { invoke } from "@tauri-apps/api/core";
import type { ProjectListItem } from "~/types/book";

/**
 * The list of saved projects: every book that has ever been generated,
 * newest-updated first. This is the one place `list_projects` is called
 * from -- the home page's "open a saved project" picker and `useBook`'s
 * "Saved books" panel both read from an instance of this rather than each
 * keeping their own copy of the same invoke call.
 */
export function useProjects() {
  const projects = ref<ProjectListItem[]>([]);

  async function refresh() {
    projects.value = await invoke<ProjectListItem[]>("list_projects");
  }

  return { projects, refresh };
}
