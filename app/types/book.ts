/**
 * The webview's half of the Phase 2 command boundary: recommending a book
 * length, generating and saving one, exporting it, and listing what has been
 * saved.
 *
 * Every interface here mirrors a `#[serde(rename_all = "camelCase")]` struct
 * in `src-tauri/src/commands.rs`, and NOTHING checks that they agree: there
 * is no generated schema across this boundary, so a field renamed on one side
 * and not the other is a silent `undefined` at runtime rather than a build
 * error (`docs/PROJECT-STATUS.md` lists this as known debt).
 *
 * What holds the two sides together is `tests/fixtures/wire/`: Rust asserts
 * its real serialised structs equal those files exactly, and
 * `tests/book.test.ts` feeds the same files through the functions below. A
 * rename on either side fails one of the two suites against a fixture that
 * did not move; "fixing" the fixture then fails the other. See that
 * directory's README.
 *
 * The pure functions live here rather than inside `GenerateBook.vue` for the
 * same reason `applyAnalysisEvent` lives in `features.ts`: a component's
 * template is not unit-testable in this project, and every one of these
 * encodes a decision worth pinning.
 */

/** Mirrors `book::preflight::Severity`, which serialises lowercase. */
export type FindingSeverity = "block" | "warn";

/** Mirrors `book::preflight::Finding`. */
export interface PreflightFinding {
  severity: FindingSeverity;
  /** 0 for a whole-book finding (e.g. free disk space) that names no page. */
  page: number;
  photoPath: string;
  message: string;
}

/** One page length the user can choose, and what choosing it costs. */
export interface PageOption {
  pages: number;
  capacityPhotos: number;
  droppedPhotos: number;
}

export interface BookRecommendation {
  /** Survivors of culling, not the raw analysed count. */
  keeperCount: number;
  recommendedPages: number;
  options: PageOption[];
}

export interface GeneratedBook {
  projectId: number;
  pageCount: number;
  placedPhotos: number;
  droppedPhotos: number;
  seed: number;
}

export interface ExportFailure {
  filename: string;
  message: string;
}

export interface ExportResult {
  /** True when pre-flight found a Block and nothing at all was written. */
  blocked: boolean;
  outputDir: string;
  blocking: PreflightFinding[];
  warnings: PreflightFinding[];
  written: string[];
  failures: ExportFailure[];
  manifestPath: string | null;
  /**
   * Why `manifest.json` could not be written, when it could not be. A
   * manifest failure never fails the export -- every file is already on disk
   * by then -- so this is reported beside a result that still names every
   * path written.
   */
  manifestError: string | null;
  format: string;
}

/**
 * What actually happened, as one value the UI can branch on.
 *
 * `blocked: false` and `written: []` is neither a block nor a success: it is
 * an export where every single item failed. Rendering that as a success
 * ("0 files written", empty format) told the user their book exported when
 * nothing did, so the four cases are enumerated here rather than derived at
 * three separate places in a template.
 */
export type ExportOutcome = "blocked" | "failed" | "partial" | "written";

export function exportOutcome(result: ExportResult): ExportOutcome {
  if (result.blocked) return "blocked";
  if (result.written.length === 0) return "failed";
  // A manifest that could not be written is a partial success too: the files
  // are there, but the record of what went where is not.
  if (result.failures.length > 0 || result.manifestError !== null) return "partial";
  return "written";
}

export interface ExportSummary {
  at: number;
  outputDir: string;
  format: string;
  fileCount: number;
}

export interface ProjectListItem {
  id: number;
  name: string;
  sourceFolder: string;
  pageCount: number;
  photoCount: number;
  createdAt: number;
  updatedAt: number;
  lastExport: ExportSummary | null;
}

export interface ProjectDetail {
  id: number;
  name: string;
  sourceFolder: string;
  createdAt: number;
  updatedAt: number;
  pageCount: number;
  photoCount: number;
  droppedPhotos: number;
  seed: number;
  exports: ExportSummary[];
}

/** Mirrors Rust's `ExportEvent`, streamed over a `tauri::ipc::Channel`. */
export interface ExportStartedEvent {
  kind: "started";
  total: number;
}

export interface ExportProgressEvent {
  kind: "progress";
  /** Cumulative as of this event, never a delta. */
  completed: number;
  total: number;
}

export type ExportEvent = ExportStartedEvent | ExportProgressEvent;

export interface ExportProgress {
  running: boolean;
  completed: number;
  total: number;
}

// Assigned straight into a ref at the start of every export, so it must never
// be mutated in place -- the same hazard (and the same `Object.freeze` guard)
// as `initialStreamState` in `features.ts`.
export const initialExportProgress: ExportProgress = Object.freeze({
  running: false,
  completed: 0,
  total: 0,
});

/**
 * Pure reducer over `ExportEvent`s, extracted from the composable so the
 * accumulation is testable without a live Tauri `Channel` -- exactly as
 * `applyAnalysisEvent` is.
 *
 * A `progress` event carries its own `total`, so progress remains determinate
 * even if the `started` event is lost (a `Channel` send failure is logged and
 * swallowed on the Rust side).
 */
export function applyExportEvent(state: ExportProgress, event: ExportEvent): ExportProgress {
  switch (event.kind) {
    case "started":
      return { running: true, completed: 0, total: event.total };
    case "progress":
      return { running: true, completed: event.completed, total: event.total };
  }
}

/** The option for a given page length, or `undefined` if it was not offered. */
export function optionFor(
  recommendation: BookRecommendation,
  pages: number,
): PageOption | undefined {
  return recommendation.options.find((option) => option.pages === pages);
}

/**
 * The option the backend recommends, falling back to the first offered one.
 *
 * The fallback is not decoration: this feeds the screen directly, and a
 * recommendation that named a length not in `options` would otherwise render
 * "undefined photos left out".
 */
export function recommendedOption(recommendation: BookRecommendation): PageOption | undefined {
  return optionFor(recommendation, recommendation.recommendedPages) ?? recommendation.options[0];
}

/** One line describing what generation produced. */
export function generatedLabel(book: GeneratedBook): string {
  const base = `${book.pageCount} pages · ${book.placedPhotos} photos placed`;
  return book.droppedPhotos > 0 ? `${base} · ${book.droppedPhotos} left out` : base;
}

export interface ExportCounts {
  blocked: boolean;
  writtenCount: number;
  failedCount: number;
  blockingCount: number;
  warningCount: number;
}

export function summarizeExport(result: ExportResult): ExportCounts {
  return {
    blocked: result.blocked,
    writtenCount: result.written.length,
    failedCount: result.failures.length,
    blockingCount: result.blocking.length,
    warningCount: result.warnings.length,
  };
}

export function blockingMessages(result: ExportResult): string[] {
  return result.blocking.map((finding) => finding.message);
}

export function warningMessages(result: ExportResult): string[] {
  return result.warnings.map((finding) => finding.message);
}

/**
 * What "Reveal in Finder" should select: the first file actually written, so
 * Finder highlights a real output rather than just opening a folder. Falls
 * back to the output directory when nothing was written (a blocked export),
 * where opening the folder is still the most useful thing available.
 */
export function revealTarget(result: ExportResult): string {
  return result.written[0] ?? result.outputDir;
}

export function lastExportLabel(project: ProjectListItem): string {
  const last = project.lastExport;
  if (!last) return "Not exported yet";
  const files = last.fileCount === 1 ? "file" : "files";
  return `${last.fileCount} ${last.format} ${files} in ${last.outputDir}`;
}

export function projectDetailLabel(project: ProjectDetail): string {
  const exports =
    project.exports.length === 0
      ? "no exports"
      : `${project.exports.length} export${project.exports.length === 1 ? "" : "s"}`;
  return `${project.pageCount} pages · ${project.photoCount} photos · ${exports}`;
}

/**
 * A book's default name: the folder it was built from. The user can change
 * it, but an unnamed project in a list is useless, so there is no empty
 * default.
 */
export function defaultProjectName(folder: string): string {
  const trimmed = folder.replace(/\/+$/, "");
  const segments = trimmed.split("/");
  return segments[segments.length - 1] || "Untitled photobook";
}
