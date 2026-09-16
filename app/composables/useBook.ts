import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  applyExportEvent,
  initialBookState,
  initialExportProgress,
  resolveExportProjectId,
  withGeneratedBook,
  withOpenedProject,
  withProjectDeleted,
  withProjectRenamed,
  type BookRecommendation,
  type BookState,
  type ExportEvent,
  type ExportProgress,
  type ExportResult,
  type GeneratedBook,
  type ProjectDetail,
} from "~/types/book";
import type { BookEdit, BookLayout } from "~/types/preview";
import type { AnalyzedPhoto, PhotoOverrides } from "~/types/features";

/**
 * Everything between "the folder is analysed" and "the files are on disk":
 * how long the book should be, generating and saving it, exporting it, and
 * the list of books already saved.
 *
 * `recommend_book` and `generate_book` are passed the analysed photos back
 * rather than re-reading them in Rust: `aestheticPct`, `sharpnessPct`,
 * `nearDupCluster` and `eventCluster` are whole-set derivations computed
 * once by `finalize_photos` and never persisted (the SQLite cache stores
 * per-photo features only), so the webview's copy is the only place they
 * exist.
 *
 * `export_book` is NOT passed them. Generating a book persists the content
 * hash of every photo it was assembled against, and export rebuilds the
 * slice from those -- so exporting works after a restart, and cannot be fed
 * a different photo set than the book was built from.
 */
export function useBook(
  photos: Ref<AnalyzedPhoto[]>,
  folders: Ref<string[]>,
  /**
   * The user's own include/exclude decisions. Sent with BOTH commands:
   * `recommend_book` so the length chooser can say a length cannot hold every
   * photo they asked for, and `generate_book` because that call is what
   * finally gives the decisions somewhere durable to live.
   */
  overrides: Ref<PhotoOverrides>,
  /** The analysis the photos came from -- see `AnalysisSummary.runId`. */
  runId: Ref<number>,
) {
  const recommendation = ref<BookRecommendation | null>(null);
  const generated = ref<GeneratedBook | null>(null);
  /** A saved project opened from disk via `openProject` -- see `BookState`'s doc comment for why this and `generated` are never both non-null. */
  const activeProject = ref<ProjectDetail | null>(null);
  const exportResult = ref<ExportResult | null>(null);
  /**
   * The assembled book, for the read-only preview. Kept beside `BookState`
   * rather than inside it: every `BookState` transition is a pure function
   * over values the webview already holds, and this one is a round trip to
   * SQLite that only two of those transitions should trigger.
   */
  const layout = ref<BookLayout | null>(null);
  const progress = ref<ExportProgress>(initialExportProgress);
  const { projects, refresh: loadProjects } = useProjects();
  const outputDir = ref<string | null>(null);
  const busy = ref(false);
  const error = ref<string | null>(null);

  /** The four ref values above, read as one `BookState` snapshot. */
  function currentBookState(): BookState {
    return {
      generated: generated.value,
      activeProject: activeProject.value,
      exportResult: exportResult.value,
      outputDir: outputDir.value,
    };
  }

  /** The project id "Export" targets -- see `resolveExportProjectId`'s doc comment. */
  const exportProjectId = computed(() => resolveExportProjectId(currentBookState()));

  /** Assigns a `BookState` transition into the refs above, all at once. */
  function applyBookState(next: BookState) {
    // The preview belongs to ONE book, identified the same way Export is (see
    // `resolveExportProjectId`). Cleared only when that identity actually
    // changes: `withProjectRenamed` and a `withProjectDeleted` for some OTHER
    // project both return a state describing the same book, and blanking the
    // preview under either would look like the book had vanished.
    if (resolveExportProjectId(currentBookState()) !== resolveExportProjectId(next)) {
      layout.value = null;
    }
    generated.value = next.generated;
    activeProject.value = next.activeProject;
    exportResult.value = next.exportResult;
    outputDir.value = next.outputDir;
  }

  /**
   * Loads the layout of a SAVED book. Works identically for one generated
   * moments ago (generating saves the project first) and one reopened from
   * disk after a restart -- which is most of the preview's value, since a
   * reopened project carries its full layout and nothing could show it.
   */
  async function loadLayout(projectId: number) {
    layout.value = await invoke<BookLayout>("book_layout", { projectId });
  }

  async function guard<T>(work: () => Promise<T>): Promise<T | null> {
    busy.value = true;
    error.value = null;
    try {
      return await work();
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      busy.value = false;
    }
  }

  async function refreshRecommendation() {
    await guard(async () => {
      // No photos: Rust reads the set cached by `analyze_folder`. This runs
      // on every override toggle, and re-uploading several megabytes of
      // feature records to answer "how many keepers now?" is what made the
      // toggle unusable on a real folder.
      recommendation.value = await invoke<BookRecommendation>("recommend_book", {
        runId: runId.value,
        overrides: overrides.value,
      });
    });
  }

  async function generate(name: string, pages: number) {
    if (folders.value.length === 0) return;
    await guard(async () => {
      // Generation SAVES: the returned id is a row that already exists, so
      // quitting here cannot lose the book.
      const result = await invoke<GeneratedBook>("generate_book", {
        photos: photos.value,
        pages,
        name,
        sourceFolders: folders.value,
        overrides: overrides.value,
      });
      // Supersedes anything previously opened from disk -- see
      // `withGeneratedBook`'s doc comment. The current `outputDir` is
      // threaded through deliberately: it survives a regenerate.
      applyBookState(withGeneratedBook(currentBookState(), result));
      await loadLayout(result.projectId);
      await loadProjects();
    });
  }

  /**
   * Loads a saved project without touching the analyser: no photos are sent
   * and nothing here calls `analyze_folder`, so opening a project never
   * re-runs Vision. Supersedes anything generated in this session -- see
   * `withOpenedProject`'s doc comment.
   */
  async function openProject(id: number) {
    await guard(async () => {
      const project = await invoke<ProjectDetail>("open_project", { id });
      applyBookState(withOpenedProject(project));
      await loadLayout(project.id);
    });
  }

  /**
   * Deletes a saved project from the list -- and, if it is the one the book
   * panel is currently showing (either generated in this session or opened
   * from disk), clears the whole book state through `withProjectDeleted`
   * rather than leaving `activeProject`/`generated` pointing at a row that no
   * longer exists. Deleting any OTHER project leaves the panel untouched.
   *
   * Calls `invoke` directly rather than going through `useProjects`'s own
   * `deleteProject` -- that one swallows its errors into its OWN `error` ref
   * (by design, for the home page's picker, which has no book state to
   * protect). Nesting THIS `guard` around a call that already swallowed its
   * exception would apply `withProjectDeleted` unconditionally, even after a
   * failed delete, and never surface the failure through the `error` this
   * component reads.
   */
  async function deleteProject(id: number) {
    await guard(async () => {
      await invoke<void>("delete_project", { id });
      applyBookState(withProjectDeleted(currentBookState(), id));
      await loadProjects();
    });
  }

  /**
   * Renames a saved project. If it is the one currently open, the displayed
   * name updates through `withProjectRenamed` immediately rather than waiting
   * on the next `list_projects` refresh to notice. Calls `invoke` directly --
   * see `deleteProject`'s doc comment for why this does not go through
   * `useProjects`'s own `renameProject`.
   */
  async function renameProject(id: number, name: string) {
    await guard(async () => {
      await invoke<void>("rename_project", { id, name });
      applyBookState(withProjectRenamed(currentBookState(), id, name));
      await loadProjects();
    });
  }

  async function pickOutputDir() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") outputDir.value = picked;
  }

  async function exportBook() {
    const projectId = exportProjectId.value;
    if (projectId === null || !outputDir.value) return;

    progress.value = initialExportProgress;
    const onEvent = new Channel<ExportEvent>();
    // Tauri's `Channel` has no `addEventListener`, only this settable field.
    // oxlint-disable-next-line unicorn/prefer-add-event-listener
    onEvent.onmessage = (event) => {
      progress.value = applyExportEvent(progress.value, event);
    };

    await guard(async () => {
      // No photos are sent: Rust rebuilds the exact slice the book was
      // assembled against from the content hashes persisted with the
      // project. That is what lets a saved book be exported after a restart,
      // and it removes the hazard of handing back a same-length-but-
      // different array after a re-analysis.
      exportResult.value = await invoke<ExportResult>("export_book", {
        projectId,
        outputDir: outputDir.value,
        onEvent,
      });
      // A blocked export wrote nothing and is not part of the project's
      // history, but the list still reloads: a successful one just changed
      // the "last export" line this list exists to show.
      await loadProjects();
    });
    progress.value = { ...progress.value, running: false };
  }

  /**
   * Applies one spread-level edit -- regenerate, reject, change layout, lock,
   * shuffle, swap -- to the book on screen, and shows exactly the book Rust
   * saved.
   *
   * The reply IS the new layout, and it replaces `layout` wholesale. Nothing
   * is patched locally: a refused edit (a swap that would cut a face, a spread
   * with no other layout to offer) comes back as an error with the reason and
   * the layout is left as Rust last returned it, so the screen and the saved
   * book cannot disagree.
   */
  async function editBook(edit: BookEdit) {
    const projectId = exportProjectId.value;
    if (projectId === null) return;
    await guard(async () => {
      layout.value = await invoke<BookLayout>("edit_book", { projectId, edit });
      // An edit bumps the project's `updated_at`, which orders the saved list.
      await loadProjects();
    });
  }

  /**
   * Clears everything derived from one analysed set OR one opened project.
   * Called when the photos change: a generated book, an opened project, a
   * chosen output folder and an export report all belong to whichever
   * folder or project they were made from, and carrying any of them across
   * to a different folder means offering to export a book against photos
   * that are no longer on screen.
   */
  function reset() {
    recommendation.value = null;
    layout.value = null;
    applyBookState(initialBookState);
    progress.value = initialExportProgress;
    error.value = null;
  }

  async function reveal(path: string) {
    await guard(() => invoke<void>("reveal_in_finder", { path }));
  }

  return {
    recommendation,
    generated,
    activeProject,
    exportResult,
    exportProjectId,
    layout,
    progress,
    projects,
    outputDir,
    busy,
    error,
    refreshRecommendation,
    generate,
    openProject,
    deleteProject,
    renameProject,
    pickOutputDir,
    exportBook,
    editBook,
    loadProjects,
    reset,
    reveal,
  };
}
