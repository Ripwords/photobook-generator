import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  applyExportEvent,
  initialExportProgress,
  type BookRecommendation,
  type ExportEvent,
  type ExportProgress,
  type ExportResult,
  type GeneratedBook,
  type ProjectListItem,
} from "~/types/book";
import type { AnalyzedPhoto } from "~/types/features";

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
export function useBook(photos: Ref<AnalyzedPhoto[]>, folder: Ref<string | null>) {
  const recommendation = ref<BookRecommendation | null>(null);
  const generated = ref<GeneratedBook | null>(null);
  const exportResult = ref<ExportResult | null>(null);
  const progress = ref<ExportProgress>(initialExportProgress);
  const projects = ref<ProjectListItem[]>([]);
  const outputDir = ref<string | null>(null);
  const busy = ref(false);
  const error = ref<string | null>(null);

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
      recommendation.value = await invoke<BookRecommendation>("recommend_book", {
        photos: photos.value,
      });
    });
  }

  async function generate(name: string, pages: number) {
    if (!folder.value) return;
    await guard(async () => {
      // Generation SAVES: the returned id is a row that already exists, so
      // quitting here cannot lose the book.
      generated.value = await invoke<GeneratedBook>("generate_book", {
        photos: photos.value,
        pages,
        name,
        sourceFolder: folder.value,
      });
      exportResult.value = null;
      await loadProjects();
    });
  }

  async function pickOutputDir() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") outputDir.value = picked;
  }

  async function exportBook() {
    const projectId = generated.value?.projectId;
    if (projectId === undefined || !outputDir.value) return;

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

  async function loadProjects() {
    projects.value = await invoke<ProjectListItem[]>("list_projects");
  }

  /**
   * Clears everything derived from one analysed set. Called when the photos
   * change: a generated book, a chosen output folder and an export report
   * all belong to the folder they were made from, and carrying any of them
   * across to a different folder means offering to export a book against
   * photos that are no longer on screen.
   */
  function reset() {
    recommendation.value = null;
    generated.value = null;
    exportResult.value = null;
    progress.value = initialExportProgress;
    outputDir.value = null;
    error.value = null;
  }

  async function reveal(path: string) {
    await guard(() => invoke<void>("reveal_in_finder", { path }));
  }

  return {
    recommendation,
    generated,
    exportResult,
    progress,
    projects,
    outputDir,
    busy,
    error,
    refreshRecommendation,
    generate,
    pickOutputDir,
    exportBook,
    loadProjects,
    reset,
    reveal,
  };
}
