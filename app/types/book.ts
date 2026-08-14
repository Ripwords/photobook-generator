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

import type { PhotoOverrides } from "~/types/features";

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
  /**
   * How many photos the user explicitly marked "include" that this length
   * cannot hold. Non-zero means this length cannot be generated at all --
   * the engine refuses rather than choosing which of the user's own picks to
   * discard (Rust's `book::pack::IncludeOverflow`).
   */
  includedOverCapacity: number;
}

export interface BookRecommendation {
  /** Survivors of culling, not the raw analysed count. */
  keeperCount: number;
  /** How many of the analysed photos the user explicitly marked "include". */
  includedCount: number;
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
  /**
   * The include/exclude decisions this book was generated with. Restored on
   * reopen so a saved project does not quietly revert every one of them to
   * "auto" -- which would look entirely correct while being wrong.
   */
  overrides: PhotoOverrides;
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

/**
 * Whether a page length can be generated at all: a length that cannot hold
 * every photo the user explicitly asked for is not a choice, it is a
 * refusal, and offering it as selectable means the user's only feedback is a
 * failed generation.
 *
 * Deliberately NOT the same question as `droppedPhotos > 0`, which is a cost
 * the user is allowed to accept.
 */
export function canGenerateAt(option: PageOption): boolean {
  return option.includedOverCapacity === 0;
}

/**
 * The sentence shown against a length the included photos overflow. Mirrors
 * the wording of Rust's `IncludeOverflow` message, which is what the user
 * would see if they generated anyway.
 */
export function includeOverflowLabel(option: PageOption): string | null {
  if (option.includedOverCapacity === 0) return null;
  return `${option.includedOverCapacity} more photo${option.includedOverCapacity === 1 ? "" : "s"} marked to include than a ${option.pages}-page book holds`;
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

/**
 * Date-only, formatted in UTC so it reads the same regardless of the
 * machine's local timezone -- unlike `lastExportLabel` above, this is shown
 * on the home page's project picker before any book state exists, where
 * there is no other context (file count, format) to anchor the reader.
 * `null` for a project that has never been exported, so the caller decides
 * how to say so rather than this baking in one wording.
 */
export function lastExportedOn(project: ProjectListItem): string | null {
  return project.lastExport ? new Date(project.lastExport.at * 1000).toISOString().slice(0, 10) : null;
}

/**
 * The book-scoped fields that must always move together: which book was
 * generated in THIS session, which saved project was opened from disk
 * instead, its export report, and the output folder chosen for it.
 *
 * Exactly one of `generated`/`activeProject` is meant to be non-null at a
 * time. This is the fix for the bug the whole "reopen a saved project"
 * feature exists for: the backend has always supported exporting any saved
 * project, but the UI only ever gated the Export button on
 * `generated?.projectId` -- the book generated in the CURRENT session -- so
 * quitting the app made every previously saved project unexportable again
 * without regenerating it. Adding `open_project` naively, by setting its own
 * ref alongside the untouched `generated` ref, would reintroduce the same
 * shape of bug one level down: generate a book, then open a different saved
 * project, and Export would still be wired to the FIRST book's id. The two
 * functions below are the single place that decides which one wins, so
 * `useBook.ts` never has to remember to clear the other one by hand.
 */
export interface BookState {
  generated: GeneratedBook | null;
  activeProject: ProjectDetail | null;
  exportResult: ExportResult | null;
  outputDir: string | null;
}

// Assigned straight into refs on every transition, so -- like
// `initialStreamState` and `initialExportProgress` -- it must never be
// mutated in place.
export const initialBookState: BookState = Object.freeze({
  generated: null,
  activeProject: null,
  exportResult: null,
  outputDir: null,
});

/**
 * A book generated in this session supersedes any project opened from disk,
 * and its stale export report goes with it -- but the chosen output folder
 * is carried over, not cleared. Regenerating (picking a different page
 * length for the SAME analysed folder) is the one path that produces
 * several `GeneratedBook`s in a row without the user ever leaving the
 * "results" screen, and re-asking them to pick the output folder on every
 * regenerate would be a genuine regression, not a safety measure -- unlike
 * `outputDir` surviving a *project switch*, which is exactly the mixing this
 * feature exists to prevent (see `withOpenedProject` below).
 */
export function withGeneratedBook(state: BookState, generated: GeneratedBook): BookState {
  return { generated, activeProject: null, exportResult: null, outputDir: state.outputDir };
}

/**
 * A project opened from disk supersedes anything generated in this session.
 * Unlike `withGeneratedBook`, `outputDir` is cleared too: opening a project
 * is a genuine switch to a different book (never a re-run of the one just
 * displayed), so a folder chosen for whatever was current before should not
 * be silently reused for it.
 */
export function withOpenedProject(project: ProjectDetail): BookState {
  return { generated: null, activeProject: project, exportResult: null, outputDir: null };
}

/**
 * The project id "Export" should target, or `null` if there is nothing to
 * export yet. Reads `generated` first: if a caller somehow leaves both set
 * (which the two functions above are written specifically to prevent), the
 * more recently generated book is the more likely intent.
 */
export function resolveExportProjectId(
  state: Pick<BookState, "generated" | "activeProject">,
): number | null {
  return state.generated?.projectId ?? state.activeProject?.id ?? null;
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
