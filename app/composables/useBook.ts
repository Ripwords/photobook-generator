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
  type FolderCheck,
  type GeneratedBook,
  type ProjectDetail,
} from "~/types/book";
import type { BookEdit, BookLayout, HistoryStatus, HistoryStep } from "~/types/preview";
import type { AnalyzedPhoto, PhotoOverrides } from "~/types/features";
import type { PrintSpec } from "~/types/printSpec";
import type { BookOptions } from "~/types/book";

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
 *
 * All four parameters feed `recommend_book`/`generate_book` only, so the
 * editor screen constructs this with none of them. It opens, previews, edits
 * and exports a saved book, and never generates one.
 */
export function useBook(
  photos: Ref<AnalyzedPhoto[]> = ref([]),
  folders: Ref<string[]> = ref([]),
  /**
   * The user's own include/exclude decisions. Sent with BOTH commands:
   * `recommend_book` so the length chooser can say a length cannot hold every
   * photo they asked for, and `generate_book` because that call is what
   * finally gives the decisions somewhere durable to live.
   */
  overrides: Ref<PhotoOverrides> = ref({}),
  /** The analysis the photos came from -- see `AnalysisSummary.runId`. */
  runId: Ref<number> = ref(0),
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
  /**
   * What the book's folders hold that it does not. Filled by `folder_check`
   * AFTER the editor is already on screen and deliberately outside `guard`:
   * it walks the disk, so a folder on a drive that is not plugged in must
   * never hold up opening the book or show as an error over it.
   */
  const folderCheck = ref<FolderCheck | null>(null);
  /**
   * What Undo and Redo would do to the book on screen. Rust owns the
   * timeline -- see `book::history` for why a `BookLayout` could not carry
   * it -- so this is read back after every write rather than tracked here.
   */
  const history = ref<HistoryStatus>({ undo: null, redo: null });
  const progress = ref<ExportProgress>(initialExportProgress);
  const { projects, refresh: loadProjects } = useProjects();
  const outputDir = ref<string | null>(null);
  const { busy, error, guard } = useBusy();

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

  async function loadHistory(projectId: number) {
    history.value = await invoke<HistoryStatus>("book_history", { projectId });
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

  /** `spec` is `null` for the default print size, which Rust fills in. */
  async function generate(name: string, pages: number, spec: PrintSpec | null, options: BookOptions) {
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
        spec,
        options,
      });
      // Supersedes anything previously opened from disk -- see
      // `withGeneratedBook`'s doc comment.
      //
      // The layout is deliberately NOT loaded here. Generating navigates
      // straight to the editor screen, which mounts a fresh `useBook` and
      // opens the project itself, so fetching it here is a `book_layout` round
      // trip whose result nothing renders before this instance is torn down.
      applyBookState(withGeneratedBook(currentBookState(), result));
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
    folderCheck.value = null;
    await guard(async () => {
      const project = await invoke<ProjectDetail>("open_project", { id });
      applyBookState(withOpenedProject(project));
      await loadLayout(project.id);
      // A book's timeline outlives the app, because every edit here is
      // written to disk as it is made and undo is the only way back.
      await loadHistory(project.id);
    });
    await checkFolders(id);
  }

  /**
   * Re-walks the book's folders and records what they hold that it does not.
   * Failure is silent on purpose: this is a courtesy notice, and a book whose
   * photos live on an external drive still opens and still exports from the
   * cache. `null` simply means the question was not answered.
   */
  async function checkFolders(id: number) {
    try {
      folderCheck.value = await invoke<FolderCheck>("folder_check", { projectId: id });
    } catch (err) {
      console.warn("folder_check failed", err);
      folderCheck.value = null;
    }
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
      await loadHistory(projectId);
      // An edit bumps the project's `updated_at`, which orders the saved list.
      await loadProjects();
    });
  }

  /**
   * Steps the book one edit back or forward along its own timeline.
   *
   * Stepping past either end is a no-op in Rust rather than an error, so a
   * shortcut pressed once too often leaves the book and the screen alone
   * instead of raising a banner. The reply is the whole layout, exactly as
   * `editBook`'s is, so nothing is patched here either.
   */
  async function stepBook(step: HistoryStep) {
    const projectId = exportProjectId.value;
    if (projectId === null) return;
    await guard(async () => {
      layout.value = await invoke<BookLayout>("step_book", { projectId, step });
      await loadHistory(projectId);
      await loadProjects();
    });
  }

  /**
   * Reloads the book on screen after something other than `editBook` changed
   * it: the agent's writes go through `agent_edit`, whose reply is the agent's
   * view of the book rather than a `BookLayout`.
   */
  async function refreshLayout() {
    const projectId = exportProjectId.value;
    if (projectId === null) return;
    await guard(async () => {
      await loadLayout(projectId);
      // The agent's edits go on the same timeline this screen's do, so
      // they are undone with the same control.
      await loadHistory(projectId);
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
    folderCheck.value = null;
    history.value = { undo: null, redo: null };
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
    folderCheck,
    history,
    progress,
    projects,
    outputDir,
    busy,
    error,
    refreshRecommendation,
    generate,
    openProject,
    checkFolders,
    deleteProject,
    renameProject,
    pickOutputDir,
    exportBook,
    editBook,
    stepBook,
    refreshLayout,
    loadProjects,
    reset,
    reveal,
  };
}
