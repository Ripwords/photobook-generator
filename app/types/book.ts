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

import type { EventTiers, PhotoOverrides, Tier } from "~/types/features";
import type { PreviewPhoto } from "~/types/preview";

/**
 * Mirrors `book::pace::BookOptions`: the draft screen's settings for one
 * book. `DEFAULT_BOOK_OPTIONS` is the book the app always built.
 */
export interface BookOptions {
  /** Chapters split where the photos move between towns, not only at a gap in time. */
  places: boolean;
  /** The fewest photos a Featured event places. Rust accepts 2..=12. */
  featuredFloor: number;
  /** The most photos a Brief event places. Rust accepts 1..=3. */
  briefCap: number;
}

/** Mirrors `book::chapter::PlaceChapters`: what "Split chapters by place" would do to a draft. */
export interface PlaceChapters {
  /** How many photos carry a usable location. With none, the option can do nothing. */
  located: number;
  /** Each photo's place chapter by path, numbered chronologically like `eventCluster`. */
  chapters: Record<string, number>;
}

/** What `place_names` answers: a town name by place chapter id, for the chapters that have one. */
export type PlaceNames = Readonly<Record<number, string>>;

export const FEATURED_FLOOR_RANGE = { min: 2, max: 12 } as const;
export const BRIEF_CAP_RANGE = { min: 1, max: 3 } as const;

export const DEFAULT_BOOK_OPTIONS: Readonly<BookOptions> = Object.freeze({
  places: false,
  featuredFloor: 6,
  briefCap: 2,
});

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
/**
 * Mirrors `book::events::Reason`: why `recommend`'s per-event plan landed on
 * the tier it did. Internally tagged (`kind`) to match Rust's
 * `#[serde(tag = "kind", rename_all = "camelCase")]`.
 */
export type TierReason =
  | { kind: "utility"; share: number }
  | { kind: "nothingKept" }
  | { kind: "undated" }
  | { kind: "ranked"; rank: number; of: number }
  | { kind: "standout" }
  | { kind: "outOfRoom"; rank: number; of: number }
  | { kind: "similarTo"; event: number }
  | { kind: "demoted" }
  | { kind: "filled" };

/**
 * Mirrors `commands::EventRow`: one event's plan (Rust's `book::events::EventPlan`,
 * `#[serde(flatten)]`ed) plus how many of its photos this length actually
 * selected.
 */
export interface EventRow {
  event: number;
  /** Effective: the user's choice, else the suggestion, after budgeting. */
  tier: Tier;
  suggested: Tier;
  /** True when this is the user's own choice, not the engine's suggestion. */
  chosen: boolean;
  reason: TierReason;
  merit: number;
  moments: number;
  kept: number;
  photos: number;
  selected: number;
}

/** Mirrors `commands::TierCounts`: how many events at a length landed on each tier. */
export interface TierCounts {
  featured: number;
  normal: number;
  brief: number;
  skipped: number;
}

/** Mirrors `book::events::TierOverflow`: the book cannot fit the floors it owes. */
export interface TierOverflow {
  needed: number;
  capacity: number;
}

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
  /** Every event's plan at this length, sorted by event id. */
  events: EventRow[];
  eventsByTier: TierCounts;
  /** The photos this length would actually place, before layout ever runs. */
  selectedPaths: string[];
  /**
   * Set when the user's own tier floors (or Includes) could not fit this
   * length even after every suggested event was demoted as far as possible.
   * Generating anyway fails with Rust's `BookError::TierFloorNotMet`.
   */
  tierOverflow: TierOverflow | null;
}

export interface BookRecommendation {
  /** Survivors of culling, not the raw analysed count. */
  keeperCount: number;
  /** How many of the analysed photos the user explicitly marked "include". */
  includedCount: number;
  recommendedPages: number;
  options: PageOption[];
}

/**
 * The sentence shown against an event's tier, explaining why `recommend`
 * suggested it (or, for `demoted`/`filled`, why `budget` overrode it).
 * `title` looks up an event's display name so the sentence can name a
 * duplicate ("Similar to <title>") without this function owning chapter
 * naming.
 */
export function tierReasonText(reason: TierReason, title: (event: number) => string): string {
  switch (reason.kind) {
    case "utility":
      return `Mostly screenshots and documents (${Math.round(reason.share * 100)}%)`;
    case "nothingKept":
      return "No photo here survived culling";
    case "undated":
      return "These photos have no date";
    case "ranked":
      return `Ranked ${reason.rank} of ${reason.of}`;
    case "standout":
      return "Stands out from the rest of the trip";
    case "outOfRoom":
      return `Ranked ${reason.rank} of ${reason.of}, beyond what this length has room for`;
    case "similarTo":
      return `Similar to “${title(reason.event)}”`;
    case "demoted":
      return "Lowered so every event's minimum fits this length";
    case "filled":
      return "Raised so the book is not left with empty pages";
  }
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
  /** The first folder; the list's one-line label. */
  sourceFolder: string;
  /** Every folder the book was analysed from, first one first. */
  sourceFolders: string[];
  pageCount: number;
  photoCount: number;
  createdAt: number;
  updatedAt: number;
  /** Starred by the user; the sidebar lists these on their own. */
  favourite: boolean;
  lastExport: ExportSummary | null;
  /** Up to four thumbnail paths for the card cover, first placed photo first. */
  coverThumbnails: string[];
}

export interface ProjectDetail {
  id: number;
  name: string;
  sourceFolder: string;
  /** Every folder the book was analysed from -- what "edit the selection" re-analyses. */
  sourceFolders: string[];
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
  /**
   * The event tier choices this book was generated with. Restored on reopen
   * so a saved project does not quietly revert every one of them to the
   * engine's own suggestion.
   */
  tiers: EventTiers;
  exports: ExportSummary[];
}

/**
 * The extensions `import_photo` will take, mirroring `SUPPORTED` in
 * `commands.rs`. Used to filter the native file picker; Rust re-checks, since
 * a path can also arrive from a drop or a stale recent-files entry.
 */
export const IMPORTABLE = [
  "jpg",
  "jpeg",
  "png",
  "heic",
  "heif",
  "cr2",
  "cr3",
  "nef",
  "arw",
  "dng",
  "raf",
  "orf",
] as const;

/** Mirrors Rust's `ImportedPhoto`: where a hand-picked photo landed. */
export interface ImportedPhoto {
  /** Its index into `BookLayout.photos`, ready for a replace or cover edit. */
  photoIndex: number;
  /** The book already held this exact file, so nothing was appended. */
  alreadyKnown: boolean;
  /**
   * The photo itself, because the dialog cannot look it up.
   *
   * It is holding the `BookLayout` it was opened with, whose `photos` stop one
   * short of `photoIndex`, and nothing re-fetches it before the tiles are
   * drawn. See `withImportedPhotos`.
   */
  photo: PreviewPhoto;
}

/**
 * Mirrors Rust's `FolderCheck`: what a saved book's source folders hold that
 * the book itself does not. Opening a book resolves its photos from content
 * hashes and never walks the disk, so this is the only thing that notices a
 * photo added to the folder afterwards.
 */
export interface FolderCheck {
  newPhotos: number;
  /**
   * Photos in the folders that analysis has already tried and given up on,
   * counted apart from `newPhotos` rather than inside it. Re-running analysis
   * reaches the same verdict, so counting them as new made the notice
   * permanent: Edit photos ran and the identical count came straight back.
   * The mark is keyed by content, so editing or replacing the file clears it
   * and the photo is new again.
   */
  unanalysable: number;
  /**
   * A folder could not be read -- a root, or one nested below it -- so both
   * counts cover only what the walk reached.
   */
  unreadable: boolean;
}

/**
 * The folder check as something to show, or `null` for nothing worth saying.
 * An unreadable folder alone is not news -- the book still opens and still
 * exports from what was analysed -- so it only ever qualifies a count.
 */
export function folderNotice(
  check: FolderCheck | null,
): { title: string; description: string } | null {
  if (!check) return null;
  const floor = check.unreadable
    ? "At least that many: one of the folders could not be read. "
    : "";
  if (check.newPhotos === 0) {
    if (check.unanalysable === 0) return null;
    const them = check.unanalysable === 1 ? "it" : "them";
    const photos = check.unanalysable === 1 ? "1 photo" : `${check.unanalysable} photos`;
    return {
      title: `${photos} in this book's folders could not be analysed`,
      description: `${floor}Whatever stopped analysis will stop it again, so Edit photos leaves ${them} out rather than counting ${them} as new on every open. Repair or replace the file and it counts as new again.`,
    };
  }
  const photos = check.newPhotos === 1 ? "1 new photo" : `${check.newPhotos} new photos`;
  const gaveUp =
    check.unanalysable === 0
      ? ""
      : check.unanalysable === 1
        ? " Another photo could not be analysed at all, and Edit photos leaves it out."
        : ` Another ${check.unanalysable} could not be analysed at all, and Edit photos leaves them out.`;
  return {
    title: `${photos} in this book's folders`,
    description: `${floor}Edit photos analyses them and brings them into the book. A photo the book has never seen is new Vision work, so that run is not instant.${gaveUp}`,
  };
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

/**
 * Whether a photo lands in the book at the chosen length. Before a
 * recommendation exists there is no length to place photos into yet, so this
 * falls back to the cull verdict -- the same thing the contact sheet showed
 * before options existed at all.
 */
export function isPlaced(photo: { path: string; kept: boolean }, option: PageOption | undefined): boolean {
  return option ? option.selectedPaths.includes(photo.path) : photo.kept;
}

/**
 * How many of `photos` land in the book at the chosen length -- the same
 * membership `isPlaced` checks one photo at a time, counted. Drives the
 * contact sheet's toolbar count and its Keepers-only filter, so both agree
 * with what the tiles dim instead of drifting back to the cull verdict once
 * a recommendation exists.
 */
export function placedCount(photos: readonly { path: string; kept: boolean }[], option: PageOption | undefined): number {
  return photos.filter((photo) => isPlaced(photo, option)).length;
}

/**
 * The `pageItems` label for one length: the page count, how many events get
 * their own place at it (Featured and Normal -- Brief and Skipped events
 * share a spread rather than getting one each), out of every event the book
 * has, and how many photos it places.
 */
export function pageOptionLabel(option: PageOption): string {
  const t = option.eventsByTier;
  const own = t.featured + t.normal;
  const all = own + t.brief + t.skipped;
  return `${option.pages} pages · ${own} of ${all} events, ${option.selectedPaths.length} photos`;
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
 * is carried over, not cleared.
 *
 * Carrying it is currently unobservable: generating navigates to the editor
 * screen, which mounts its own `useBook` with no output folder chosen yet, so
 * nothing survives a regenerate any more. It is kept because the alternative
 * -- clearing it -- would be the wrong default the moment a caller does
 * generate twice without remounting, and because `outputDir` surviving a
 * *project switch* is the mixing this whole state machine exists to prevent
 * (see `withOpenedProject` below), which is a different question.
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

/**
 * The state after a project is deleted from the database.
 *
 * If the deleted id is the one "Export" currently targets (see
 * `resolveExportProjectId`), the WHOLE book state is cleared through
 * `initialBookState` -- not just `activeProject` set to `null`. Clearing only
 * `activeProject` would still leave a stale `outputDir` and `exportResult`
 * behind (harmless on their own), but worse, it would leave nothing wrong
 * *looking* wrong: the panel would simply vanish while Export, if it were
 * somehow still reachable, kept the dead id. Deleting a project that is
 * NEITHER the generated book nor the opened one leaves the state completely
 * untouched -- deleting some other saved project must not disturb whatever
 * is currently on screen.
 */
export function withProjectDeleted(state: BookState, deletedId: number): BookState {
  return resolveExportProjectId(state) === deletedId ? initialBookState : state;
}

/**
 * The state after a project is renamed.
 *
 * Only `activeProject` can carry a project's name at all -- a `GeneratedBook`
 * has no `name` field, because generating one does not require this session
 * to have chosen a name for anything yet. So a rename patches `activeProject`
 * in place ONLY when it is the one just renamed; every other case (a
 * different project's id, or nothing opened at all) returns `state`
 * unchanged rather than inventing something to patch.
 */
export function withProjectRenamed(state: BookState, id: number, name: string): BookState {
  if (state.activeProject && state.activeProject.id === id) {
    return { ...state, activeProject: { ...state.activeProject, name } };
  }
  return state;
}

/**
 * How many decisions a saved project carries, as a sentence -- or `null` when
 * the engine chose everything in it.
 *
 * A reopened project used to return `overrides` and show nothing at all, so
 * the decisions were invisible and unchangeable. That is the same defect that
 * started this line of work (persistence that existed but was unreachable
 * from the UI), one level down.
 */
export function selectionLabel(project: ProjectDetail): string | null {
  const states = Object.values(project.overrides);
  const included = states.filter((state) => state === "include").length;
  const excluded = states.filter((state) => state === "exclude").length;
  if (included === 0 && excluded === 0) return null;
  const parts: string[] = [];
  if (included > 0) parts.push(`${included} you included`);
  if (excluded > 0) parts.push(`${excluded} you excluded`);
  return `Your selection: ${parts.join(", ")}`;
}

export function projectDetailLabel(project: ProjectDetail): string {
  const count = project.exports.length;
  const exports =
    count === 0 ? "not exported yet" : count === 1 ? "exported once" : `exported ${count} times`;
  return `${project.pageCount} pages, ${project.photoCount} photos, ${exports}`;
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

/**
 * The folders a book draws from, as one short label: the first folder's
 * name, and how many more there are. Several folders are one population --
 * every photo is ranked against all of them -- so the label says "and 2
 * more", never lists them, which is what the tooltip is for.
 */
export function folderListLabel(folders: readonly string[]): string {
  const [first, ...rest] = folders;
  if (!first) return "the selected folders";
  const name = defaultProjectName(first);
  if (rest.length === 0) return name;
  return `${name} and ${rest.length} more folder${rest.length === 1 ? "" : "s"}`;
}
