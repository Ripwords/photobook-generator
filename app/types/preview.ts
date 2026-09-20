/**
 * The read-only spread preview: turning the layout Rust already computed into
 * the handful of numbers the DOM needs to draw it.
 *
 * **This file recomputes nothing about the book.** Every rect here arrives
 * from `src-tauri/src/preview.rs`, which copies it straight off the `Book`
 * that `pace::assemble` produced and `project.rs` persisted, and every guide
 * is derived in Rust from THAT BOOK'S OWN `PrintSpec` and shipped on the wire
 * rather than restated here. That is deliberate: a preview whose job is to
 * reveal defects -- a crop that cuts a face, salient content sliding into the
 * gutter, a page that prints blank -- has to be showing the same geometry
 * that was validated, or it flatters the engine and the user is wrong at the
 * printer. Now that the geometry is per book, a guide restated here would not
 * merely drift; it would be wrong for every book that is not 11 x 8.5.
 *
 * **Positioned DOM elements with CSS transforms, not a canvas.** Photos are
 * `<img>` with a transform for the crop; guides are absolutely positioned
 * overlays. Hit-testing, focus and accessibility come free and the geometry
 * is inspectable in devtools. Rendering is at roughly 1400 x 556 per spread;
 * print resolution for the default 11 x 8.5 book is 6718 x 2668, which is
 * 71.7 MB of canvas per spread and well past what WKWebView will do -- and a
 * book with a larger page or a higher target resolution is worse still.
 *
 * The functions live here rather than inside `BookPreview.vue` for the same
 * reason `applyAnalysisEvent` lives in `features.ts`: a component's template
 * is not unit-testable in this project, and the geometry is exactly where the
 * bugs would be. `tests/preview.test.ts` pins all of it against the same
 * `tests/fixtures/wire/book-layout.json` that Rust asserts itself against.
 */

import type { BookOptions } from "./book";
import type { PrintSpec } from "./printSpec";

/** Mirrors `geometry::Rect`, which serialises its fields unrenamed. */
export interface PreviewRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Mirrors `geometry::Side`, which serialises lowercase. */
export type PageSide = "left" | "right";

/** Mirrors `preview::PreviewPhoto`. */
export interface PreviewPhoto {
  path: string;
  hash: string;
  /** Oriented pixel dimensions -- the frame `crop` is normalised against. */
  width: number;
  height: number;
  /**
   * The sidecar's 400px contact-sheet JPEG, or `null` when writing it failed.
   * `null` renders as an empty box, never as a missing placement.
   */
  thumbnailPath: string | null;
  /** The analysis's quality percentile, 0-100. */
  aestheticPct: number;
  /** Capture time in Unix seconds, or `null` when the photo carries none. */
  capturedAt: number | null;
}

/** Mirrors `preview::PreviewPlacement`. */
export interface PreviewPlacement {
  /** Indexes `BookLayout.photos`, NOT the position of this placement. */
  photoIndex: number;
  /** Page-normalised destination rect. */
  slotRect: PreviewRect;
  /** Crop window in the photo's own oriented, normalised frame. */
  crop: PreviewRect;
  z: number;
  /**
   * The basename the exporter writes this placement under, extensionless, so
   * something odd on screen can be traced to the file it produced and checked
   * against `manifest.json`. Built by `export::output_filename` in Rust --
   * never re-derived here, or the preview would name files the exporter does
   * not write. `null` only when the photo index is out of range.
   */
  filename: string | null;
}

/** Mirrors `preview::PreviewPage`. */
export interface PreviewPage {
  number: number;
  side: PageSide;
  templateId: string;
  /** True when this page prints with nothing on it -- see `preview.rs`. */
  blank: boolean;
  placements: PreviewPlacement[];
}

/**
 * Mirrors `preview::PageGuides`: one page's three guide rects, page-normalised
 * and finished. `safe` is a strict subset of `trim` on every edge but the
 * fold; `gutter` is a full-height strip AGAINST the fold, overlapping both.
 */
export interface PageGuides {
  trim: PreviewRect;
  safe: PreviewRect;
  gutter: PreviewRect;
}

/**
 * Mirrors `preview::PreviewGeometry`: the guides one book's spec implies,
 * one set per side, ready to position.
 *
 * Derived, never authored -- the editable measurements are `BookLayout.spec`.
 * Indexed by `PageSide`, so a page reads `geometry[side].trim` and never has
 * to know which of its edges is the fold. That asymmetry lives in Rust alone.
 */
export interface PreviewGeometry {
  pageWIn: number;
  pageHIn: number;
  left: PageGuides;
  right: PageGuides;
}

/** Mirrors `preview::BookLayout`, returned by the `book_layout` command. */
export interface BookLayout {
  projectId: number;
  seed: number;
  pageCount: number;
  placedPhotos: number;
  droppedPhotos: number;
  /**
   * The book's own print geometry, in the inches the user typed. This is what
   * the Print size panel edits and hands back to `setPrintSpec`; `geometry`
   * below is what the preview draws, derived from it in Rust.
   */
  spec: PrintSpec;
  /** The book's own options, carried into its draft by "Edit photos" like `spec`. */
  options: BookOptions;
  geometry: PreviewGeometry;
  /** Every analysed photo, in the order `photoIndex` indexes. */
  photos: PreviewPhoto[];
  pages: PreviewPage[];
  /** One entry per opening, in `toSpreads` order -- see `openingFor`. */
  openings: PreviewOpening[];
  cover: PreviewCover;
}

/**
 * Mirrors `preview::PreviewCover`. `aspect` and each `visible` rect come from
 * the book's spec in Rust, like the page guides, and are never re-derived here.
 */
export interface PreviewCover {
  /** Panel width over height in inches: the shape a cover crop is cut to. */
  aspect: number;
  /** Lowercase `#rrggbb`. */
  spine: string;
  front: PreviewCoverSide;
  back: PreviewCoverSide;
}

/** Mirrors `preview::PreviewCoverSide`. */
export interface PreviewCoverSide {
  /** Panel-normalised: the finished board. The rest of the panel is wrap, which folds under. */
  board: PreviewRect;
  /** Panel-normalised: what shows on the finished board, less the safe margin. */
  visible: PreviewRect;
  photo: PreviewCoverPhoto | null;
}

/** Mirrors `preview::PreviewCoverPhoto`. */
export interface PreviewCoverPhoto {
  /** Indexes `BookLayout.photos`. */
  photoIndex: number;
  crop: PreviewRect;
  /** Built by `export::cover_filename`; `null` only when the index is out of range. */
  filename: string | null;
}

/**
 * Mirrors `preview::PreviewOpening`: what the user has said about one
 * opening and what the engine could still offer it. An opening is what the
 * reader sees with the book open -- page 1 alone, then each pair of facing
 * pages, then the last page alone -- and it is numbered the way `toSpreads`
 * orders them, so `openings[i]` describes `toSpreads(pages)[i]`.
 */
export interface PreviewOpening {
  index: number;
  /** Skipped by "shuffle"; refuses every other edit until unlocked. */
  locked: boolean;
  /** Templates the user has rejected here; never offered again. */
  rejected: string[];
  /**
   * Templates that could replace the current one. Empty when the opening
   * holds nothing, or when every template for this many photos has been
   * shown or rejected -- which is also when "regenerate" has nothing to do.
   */
  alternatives: string[];
}

/** Mirrors `edit::PlacementRef`: a slot named by printed page number and z. */
export interface PlacementRef {
  page: number;
  z: number;
}

/**
 * Mirrors `edit::BookEdit`, the one argument of the `edit_book` command.
 * `kind` is the serde tag; every field name is the camelCase Rust accepts,
 * pinned by `tests/fixtures/wire/book-edits.json` on both sides.
 */
export type BookEdit =
  | { kind: "regenerate"; opening: number }
  | { kind: "rejectTemplate"; opening: number }
  | { kind: "setTemplate"; opening: number; templateId: string }
  | { kind: "setLocked"; opening: number; locked: boolean }
  | { kind: "shuffle" }
  | { kind: "swapPhotos"; a: PlacementRef; b: PlacementRef }
  | { kind: "setCrop"; placement: PlacementRef; x: number; y: number; w: number }
  | { kind: "setSlot"; placement: PlacementRef; rect: PreviewRect }
  | { kind: "replacePhoto"; placement: PlacementRef; photo: number }
  /**
   * Print this book at a different size. Rust re-cuts every crop and keeps
   * the layout; see `book::reprint`. Unlike every other edit here it is never
   * refused, so the caller shows the findings from `check_print_spec` BEFORE
   * sending it, not a rejection afterwards.
   *
   * Deliberately absent from `app/agent/tools.ts`: the chat agent edits
   * layouts, it does not get to change what book the user is buying.
   */
  | { kind: "setPrintSpec"; spec: PrintSpec }
  /** Put any analysed photo on one side of the cover, or clear it with `null`. */
  | { kind: "setCoverPhoto"; side: CoverSide; photo: number | null }
  /** A hand crop on the cover; Rust derives the height from the cover panel. */
  | { kind: "setCoverCrop"; side: CoverSide; x: number; y: number; w: number }
  /** The plain spine colour, `#rrggbb`. */
  | { kind: "setSpineColour"; rgb: string };

/** Mirrors `geometry::CoverSide`. */
export type CoverSide = "front" | "back";

/**
 * Mirrors `book::history::HistoryStatus`: the edit each control would move
 * over, or `null` for that end of the timeline. Rust holds the timeline
 * itself -- see `book::history` for why a layout could not.
 */
export interface HistoryStatus {
  undo: string | null;
  redo: string | null;
}

/** Mirrors `book::history::Step`, the one argument of `step_book`. */
export type HistoryStep = "undo" | "redo";

/**
 * Whether Undo or Redo can run right now.
 *
 * Read by the buttons AND by the keyboard shortcuts that do the same thing.
 * They used to decide separately, and the shortcut forgot `busy`: holding
 * Cmd-Z during a save sent overlapping `step_book` calls, and the book left
 * on screen was whichever reply landed last rather than where the cursor
 * finished.
 */
export function canStep(step: HistoryStep, status: HistoryStatus, busy: boolean): boolean {
  return !busy && status[step] !== null;
}

/** What the Undo or Redo control reads, e.g. "Undo resize". */
export function stepLabel(step: HistoryStep, status: HistoryStatus): string {
  const named = status[step];
  return named ? `${step === "undo" ? "Undo" : "Redo"} ${named}` : `Nothing to ${step}`;
}

/**
 * What releasing the pointer on a photo does.
 *
 * "save" commits the window on screen as the slot's crop and keeps drawing it
 * until the saved book answers. "select" throws the window away and picks the
 * photo for a swap. Never both: `BookPreview.onCrop` drops the selection on
 * any crop, so a release that saves cannot also leave a photo selected.
 */
export type Release = "save" | "select";

/**
 * Decide it from the three things that differ between one release and another.
 *
 * `moved` is whether the press travelled `DRAG_THRESHOLD_PX` and so counts as
 * a drag. `live` is whether a window is being drawn that the saved book does
 * not have. `owed` is whether a wheel zoom put that window there and has not
 * been saved yet -- the wheel waits out `WHEEL_SETTLE_MS` of quiet so that a
 * roll is one edit, and a press inside that wait takes the pending save over.
 *
 * `owed` is the whole reason this is a function. A click inside the settle
 * window is not a drag, so it fell to the branch that throws the live window
 * away, and the zoom the user had just watched happen snapped back to the old
 * crop -- for over three seconds in a traced run -- before the pending save
 * landed and jumped it forward again. The same handoff keeps zoom-then-drag to
 * one entry in the timeline: the drag starts from the zoomed window, so its
 * crop already carries the zoom and the wheel's own save would be a stale
 * second edit for the same slot.
 */
export function releaseAction(moved: boolean, live: boolean, owed: boolean): Release {
  return live && (moved || owed) ? "save" : "select";
}

/**
 * What a gesture starting now does to a zoom that is still waiting out
 * `WHEEL_SETTLE_MS`.
 *
 * "save" emits it as its own edit, "hold" leaves it pending for the release to
 * decide as `releaseAction`'s `owed`, and "none" means there is nothing to do.
 *
 * Only one zoom can be waiting, and one timer holds it. So a gesture on a
 * DIFFERENT box has to flush it here: overwriting the pending box and
 * re-arming the shared timer dropped the first zoom entirely. Measured on both
 * surfaces -- command-scroll one cover panel then the other inside the settle
 * window and the first panel went 113.475% wide on screen, then back to the
 * 113.475% the book still held, with no edit for it in the timeline.
 *
 * A gesture on the SAME box is not a loss. Another roll of the wheel is
 * computed from the window already on screen and so carries the waiting zoom,
 * which is why it is "none" rather than a stale first edit; a press is held
 * because a drag's crop starts from that same window.
 */
export type Takeover = "save" | "hold" | "none";

export function zoomTakeover(pending: string | null, box: string, press: boolean): Takeover {
  if (pending === null) return "none";
  if (pending !== box) return "save";
  return press ? "hold" : "none";
}

/** Mirrors `score::Rejection`: the hard constraints an edit can break. */
export type Rejection = "faceClipped" | "faceInGutter" | "faceInSafeMargin" | "tooLowResolution";

/** Mirrors `edit::SlotCandidate`: how one photo would sit in one slot. */
export interface SlotCandidate {
  crop: PreviewRect;
  refused: Rejection | null;
}

/**
 * One printed opening: two facing pages with the fold between them, or ONE
 * page facing an inside cover.
 *
 * A book is 2 single pages + (N-2)/2 spreads. Page 1 is a right-hand page
 * facing the inside front cover; the last page is a left-hand page facing the
 * inside back cover. Neither has a partner, and rendering either as half of a
 * spread would show a fold where the book has a cover.
 */
export interface PreviewSpread {
  key: string;
  left: PreviewPage | null;
  right: PreviewPage | null;
  label: string;
}

/**
 * CSS for an absolutely positioned box inside the page.
 *
 * A type alias, not an interface, and that is load-bearing. Vue's
 * `CSSProperties` carries a `` `--${string}` `` index signature for custom
 * properties, and TypeScript grants an implicit index signature to an object
 * type alias but never to an interface. As an interface this is not assignable
 * to `:style` at all, which is how it went unchecked until `bun run typecheck`
 * existed.
 */
export type BoxStyle = {
  left: string;
  top: string;
  width: string;
  height: string;
};

/** CSS for the `<img>` inside a slot. A type alias for the reason `BoxStyle` is. */
export type CropStyle = {
  width: string;
  height: string;
  /**
   * Always `"none"`. Tailwind's preflight sets `img { max-width: 100% }`,
   * which would clamp an enlarged image back to the slot width and silently
   * show MORE of the photo than the engine cropped -- the one regression this
   * whole file exists to prevent, arriving through CSS rather than through
   * arithmetic. It lives in the style object so a template cannot forget it.
   */
  maxWidth: string;
  transform: string;
};

/** One drawable slot: which photo, where on the page, and which part of it. */
export interface PreviewSlot {
  /** Unique within the book: page number and stacking order, never the hash. */
  key: string;
  z: number;
  /** `null` when the payload carries no photo at that index. */
  photo: PreviewPhoto | null;
  slot: BoxStyle;
  crop: CropStyle;
  /** The exporter's own basename for this placement -- see `PreviewPlacement`. */
  filename: string | null;
}

/**
 * Percentages, rounded to 4 decimal places.
 *
 * Rounding is not cosmetic here: `0.62 * 100` is `62.00000000000001` in
 * IEEE-754, and a style string carrying that reads as a bug in devtools. Four
 * places is roughly 0.03 px on a 700 px page -- far below one device pixel,
 * so nothing about where a box lands changes.
 */
function pct(value: number): string {
  return `${Math.round(value * 10_000) / 10_000}`;
}

/**
 * A positive extent, or 1 as a fallback.
 *
 * A crop with a non-positive extent cannot happen -- `book::crop::choose_crop`
 * never produces one -- but dividing by it would yield `Infinity` and a slot
 * that renders as nothing at all, which reads on screen as "the engine placed
 * no photo here". Falling back to the full frame is visibly wrong instead of
 * invisibly wrong.
 */
function extent(value: number): number {
  return value > 0 ? value : 1;
}

/**
 * The CSS that draws the engine's crop, and the most important few numbers in
 * the preview.
 *
 * The cropped window has to fill the slot, so the whole image is drawn at
 * `1/crop.w` by `1/crop.h` of the slot's size and then slid so the window's
 * top-left corner sits at the slot's. `transform: translate()` resolves its
 * percentages against the ELEMENT's own border box -- which, after the sizing
 * above, is the full image -- so the offsets are exactly the normalised
 * `crop.x` and `crop.y`, with no second conversion to get wrong.
 *
 * Undistorted precisely when the crop's real-world aspect equals the slot's,
 * which is what `choose_crop` guarantees (it only ever moves the window, never
 * reshapes it). A visible stretch here would therefore be a real disagreement
 * between the crop and the slot, not a rendering artefact -- which is the
 * behaviour worth having.
 */
export function cropStyle(crop: PreviewRect): CropStyle {
  return {
    width: `${pct(100 / extent(crop.w))}%`,
    height: `${pct(100 / extent(crop.h))}%`,
    maxWidth: "none",
    transform: `translate(-${pct(crop.x * 100)}%, -${pct(crop.y * 100)}%)`,
  };
}

/** Any page-normalised rect as an absolutely positioned box. */
export function rectStyle(rect: PreviewRect): BoxStyle {
  return {
    left: `${pct(rect.x * 100)}%`,
    top: `${pct(rect.y * 100)}%`,
    width: `${pct(rect.w * 100)}%`,
    height: `${pct(rect.h * 100)}%`,
  };
}

function spread(left: PreviewPage | null, right: PreviewPage | null): PreviewSpread {
  const numbers = [left?.number, right?.number].filter((n) => n !== undefined);
  return {
    key: `spread-${numbers.join("-")}`,
    left,
    right,
    label: numbers.length > 1 ? `Pages ${numbers[0]}–${numbers[1]}` : `Page ${numbers[0]}`,
  };
}

/**
 * The book's pages grouped into printed openings.
 *
 * Page 1 alone, then the middle in facing pairs, then the last page alone --
 * matching `pace::assemble`, which builds exactly that and gives page 1
 * `Side::Right` and the last page `Side::Left`.
 *
 * **Every page appears exactly once**, blank ones included: ten of the 36
 * templates leave a full page empty (mostly text-zone layouts nothing renders
 * yet), so skipping them would show a tidier book than the one that prints. An
 * odd-length middle -- not a real SKU, but reachable through a degenerate page
 * count -- leaves its last page unpartnered rather than dropping it.
 */
export function toSpreads(pages: PreviewPage[]): PreviewSpread[] {
  if (pages.length === 0) return [];

  const spreads: PreviewSpread[] = [];
  const first = pages[0];
  if (first) spreads.push(spread(null, first));

  const middle = pages.slice(1, -1);
  for (let i = 0; i < middle.length; i += 2) {
    spreads.push(spread(middle[i] ?? null, middle[i + 1] ?? null));
  }

  if (pages.length > 1) {
    const last = pages[pages.length - 1];
    if (last) spreads.push(spread(last, null));
  }
  return spreads;
}

/**
 * The photo at an index of the layout's own photo list, or `null`.
 *
 * `photoIndex` indexes the WHOLE analysed slice -- the same contract
 * `book::manifest` and `export::build_items` read -- so this must never be
 * confused with a placement's position on its page. The two differ on every
 * page whose photos were not assigned in order, and the symptom is the
 * preview showing one photograph where another one prints.
 */
/**
 * Which side of the fold a page is, for choosing its guides.
 *
 * A page's side is a PROPERTY OF THE PAGE, never of the half of the opening it
 * happens to be drawn in. The two agree for every real SKU -- `pace::assemble`
 * builds R, (L,R)..., L -- but the padding path at `pace.rs:418` does not: it
 * appends by parity, so the last page of a degenerate 5-page book is a
 * right-hand page even though `toSpreads` renders it in the left half.
 *
 * Inferring the side from position there would inset the trim and draw the
 * gutter band on the wrong edges, i.e. the preview misrepresenting the
 * physical book -- the one thing it must never do.
 *
 * The half is used only for `null`, which is an inside cover rather than a
 * page, and so has no side of its own.
 */
export function pageSide(page: PreviewPage | null, half: PageSide): PageSide {
  return page?.side ?? half;
}

/**
 * Which inside cover fills the empty half of a lone page's opening. Page 1 is
 * drawn in the RIGHT half, so the left half beside it is the inside front
 * cover; the last page is drawn in the left half, facing the inside back
 * cover. The template used to have these swapped.
 */
export function insideCover(half: PageSide): "front" | "back" {
  return half === "left" ? "front" : "back";
}

/**
 * The distinct templates an opening drew from, in left-then-right order.
 *
 * Deduplicated because a spread's two halves normally come from ONE template
 * decomposed at the fold, and naming it twice would read as a repeat. A
 * genuinely repeating template across consecutive spreads is one of the
 * defects this preview exists to reveal, and it is only inferable from layout
 * shape unless the id is on screen.
 */
export function spreadTemplates(opening: PreviewSpread | undefined): string[] {
  const ids: string[] = [];
  for (const page of [opening?.left, opening?.right]) {
    if (page && !ids.includes(page.templateId)) ids.push(page.templateId);
  }
  return ids;
}

export function photoFor(layout: BookLayout, photoIndex: number): PreviewPhoto | null {
  return layout.photos[photoIndex] ?? null;
}

/** Everything needed to draw one page's slots, in stacking order. */
export function pageSlots(layout: BookLayout, page: PreviewPage): PreviewSlot[] {
  return page.placements.map((placement) => ({
    key: `p${page.number}-z${placement.z}`,
    z: placement.z,
    photo: photoFor(layout, placement.photoIndex),
    slot: rectStyle(placement.slotRect),
    crop: cropStyle(placement.crop),
    filename: placement.filename,
  }));
}

/**
 * The photos no page placed -- what "N left out" actually refers to.
 *
 * Derived from which indices the pages reference rather than from a flag,
 * because there is no flag: the engine reports only a COUNT (`Book::dropped`,
 * covering culling, capacity trimming, unplaced groups and page-half
 * overflow), and the preview carries the full photo list precisely so the
 * count can be turned back into pictures.
 */
export function leftOutPhotos(layout: BookLayout): PreviewPhoto[] {
  const placed = new Set(
    layout.pages.flatMap((page) => page.placements.map((placement) => placement.photoIndex)),
  );
  return layout.photos.filter((_, index) => !placed.has(index));
}

/** No decisions and nothing to offer: what a payload without controls means. */
const UNCONTROLLED: Omit<PreviewOpening, "index"> = { locked: false, rejected: [], alternatives: [] };

/**
 * The controls for the opening at `index` of `toSpreads(layout.pages)`.
 *
 * Falls back to "unlocked, nothing to offer" rather than throwing when the
 * payload carries no entry, so a preview drawn from an older payload still
 * renders every page; the buttons simply have nothing to do.
 */
export function openingFor(layout: BookLayout, index: number): PreviewOpening {
  return layout.openings.find((opening) => opening.index === index) ?? { index, ...UNCONTROLLED };
}

/** Whether "regenerate", "reject" and "change layout" can do anything here. */
export function canRelayout(opening: PreviewOpening): boolean {
  return !opening.locked && opening.alternatives.length > 0;
}

export function samePlacement(a: PlacementRef, b: PlacementRef): boolean {
  return a.page === b.page && a.z === b.z;
}

/** What a click on a photo does while swapping: the next selection, and the edit to send if any. */
export interface SwapStep {
  selected: PlacementRef | null;
  edit: BookEdit | null;
}

/**
 * The swap gesture as a pure function: the first click selects a photo, a
 * second click on the SAME photo deselects it, and a click on any other photo
 * -- on any page of the book -- swaps the two and clears the selection.
 *
 * Nothing about which swaps are allowed lives here. Rust refuses a swap that
 * would cut a face, put one in the gutter or the margin, or print below 200
 * DPI, and returns the untouched book with the reason; the webview shows the
 * reason and the book it was given.
 */
export function nextSwapStep(selected: PlacementRef | null, clicked: PlacementRef): SwapStep {
  if (selected === null) return { selected: clicked, edit: null };
  if (samePlacement(selected, clicked)) return { selected: null, edit: null };
  return { selected: null, edit: { kind: "swapPhotos", a: selected, b: clicked } };
}

/**
 * A template id as a menu label: `07-two-up-symmetric-margin` reads
 * "two-up symmetric margin", and a page half `...:right` says which half. The
 * number is the library's own sort key, not something a user chooses by.
 */
export function templateLabel(id: string): string {
  const [base, side] = id.split(":");
  const words = (base ?? id).replace(/^\d+-/, "").replaceAll("-", " ");
  return side ? `${words} (${side} half)` : words;
}

/** How far a pointer moved, as a fraction of the SLOT's rendered size. */
export interface SlotDelta {
  dx: number;
  dy: number;
}

/**
 * The crop after dragging the photo by `delta` slot-widths and slot-heights.
 *
 * Dragging the photo right shows more of its left, so the window moves the
 * other way; and because the window is drawn scaled to `1/crop.w` of the slot,
 * one slot-width of pointer travel is `crop.w` of photo. The window stays
 * inside the photo and keeps its size, so a drag can never distort or reveal
 * pixels the photo does not have.
 */
export function cropMoved(crop: PreviewRect, delta: SlotDelta): PreviewRect {
  const x = clamp(crop.x - delta.dx * crop.w, 0, 1 - crop.w);
  const y = clamp(crop.y - delta.dy * crop.h, 0, 1 - crop.h);
  return { x, y, w: crop.w, h: crop.h };
}

/**
 * The crop after zooming by `factor` about its centre: `factor > 1` shows LESS
 * of the photo (the window shrinks), `factor < 1` shows more. The window never
 * grows past the photo on either axis, never shrinks below 2% of it, and keeps
 * its shape, so the slot's aspect is preserved. Rust re-derives the height
 * from the slot anyway and refuses a window that would print below the book's DPI floor.
 */
export function cropZoomed(crop: PreviewRect, factor: number): PreviewRect {
  if (!(factor > 0) || !Number.isFinite(factor)) return crop;
  const shape = crop.h / crop.w;
  const largest = Math.min(1, 1 / shape);
  const w = clamp(crop.w / factor, Math.min(0.02, largest), largest);
  const h = w * shape;
  const cx = crop.x + crop.w / 2;
  const cy = crop.y + crop.h / 2;
  return {
    x: clamp(cx - w / 2, 0, 1 - w),
    y: clamp(cy - h / 2, 0, 1 - h),
    w,
    h,
  };
}

const ZOOM_PER_WHEEL_UNIT = 0.0015;

/**
 * How much a wheel event over a photo zooms its crop, or `null` when it should
 * scroll the page instead. Zoom needs ⌘ (or Ctrl) held, because the preview is a long scroll of
 * spreads, and a bare wheel that zoomed whichever photo drifted under the
 * pointer turned scrolling the book into accidental crop edits.
 */
export function wheelZoomFactor(
  event: Pick<WheelEvent, "deltaY" | "ctrlKey" | "metaKey">,
): number | null {
  if (!event.ctrlKey && !event.metaKey) return null;
  // Scrolling up (negative deltaY) zooms in, as in every image viewer.
  return Math.exp(-event.deltaY * ZOOM_PER_WHEEL_UNIT);
}

/** The edit that saves a crop the user dragged or zoomed into place. */
export function setCropEdit(placement: PlacementRef, crop: PreviewRect): BookEdit {
  return { kind: "setCrop", placement, x: crop.x, y: crop.y, w: crop.w };
}

/** `setCropEdit` for a cover photo. */
export function setCoverCropEdit(side: CoverSide, crop: PreviewRect): BookEdit {
  return { kind: "setCoverCrop", side, x: crop.x, y: crop.y, w: crop.w };
}

function clamp(value: number, lo: number, hi: number): number {
  return Math.min(Math.max(value, lo), Math.max(lo, hi));
}

/** The edit that saves a slot the user moved or resized. */
export function setSlotEdit(placement: PlacementRef, rect: PreviewRect): BookEdit {
  return { kind: "setSlot", placement, rect };
}

/** Which corner of a slot a resize handle drags. */
export type Corner = "nw" | "ne" | "sw" | "se";

/** Lines on one page, one list per axis, page-normalised, deduped and ascending. */
export interface AxisLines {
  xs: number[];
  ys: number[];
}

/**
 * What a slot can snap to on a page, split by which feature of the slot may
 * land on them. The pool IS the kind: a line's meaning is the array it sits
 * in, and a line that is meaningful two ways (the trim centre) sits in both.
 *
 * `edges` holds the page's own lines -- canvas, trim, safe, gutter -- the trim
 * centre, and the edges of every OTHER slot. `centres` holds the trim centre
 * and the centres of every other slot; a MOVED slot's centre snaps here, a
 * resized slot's centre never does.
 *
 * A feature snaps to the same feature. A box centre never catches on a
 * neighbour's edge and an edge never on a neighbour's centre: nobody lays out
 * to those, and each position offered is another place for a drag to stick.
 */
export interface SnapGuides {
  edges: AxisLines;
  centres: AxisLines;
}

/**
 * Where a slot is held after a snap: per axis, the line it landed on, or
 * `null` when that axis was left where the pointer put it.
 *
 * When `x` is not null, one of the slot's edges -- or, after a move, its
 * centre -- lies at exactly `x`, and `x` is a member of the pool it was drawn
 * from. A held line is a line the box is ON, never one it was pulled toward
 * and then clamped off.
 */
export interface HeldLines {
  readonly x: number | null;
  readonly y: number | null;
}

/** Held on nothing. Frozen, so it is safe to share as the resting state. */
export const NO_HOLD: HeldLines = Object.freeze({ x: null, y: null });

/** A snapped slot: where it is now, and what holds it there. */
export interface SnappedSlot {
  rect: PreviewRect;
  held: HeldLines;
}

/**
 * Every line worth snapping to on a page, in two pools -- see `SnapGuides`.
 * The same geometry the preview draws and the engine enforces:
 * `PreviewGeometry` is the book's own spec on the wire, so a slot snapped to
 * the safe line here is exactly inside `in_safe_margin` there.
 *
 * Exactly one centre per axis comes from the page: the TRIM centre, the middle
 * of the finished printed page. On the fold side the canvas and safe centres
 * sit within one threshold of it, because a page carries no bleed at the fold
 * (trim 0.508797, safe 0.514379, canvas 0.5 on a left page), so offering all
 * three would make one snap three ways. Vertically all three coincide at 0.5.
 */
export function pageGuides(
  geometry: PreviewGeometry,
  side: PageSide,
  others: readonly PreviewRect[],
): SnapGuides {
  const { trim, safe, gutter } = geometry[side];
  const cx = trim.x + trim.w / 2;
  const cy = trim.y + trim.h / 2;
  const xs = [0, 1, trim.x, trim.x + trim.w, safe.x, safe.x + safe.w, gutter.x, gutter.x + gutter.w, cx];
  const ys = [0, 1, trim.y, trim.y + trim.h, safe.y, safe.y + safe.h, cy];
  const centreXs = [cx];
  const centreYs = [cy];
  for (const r of others) {
    xs.push(r.x, r.x + r.w);
    ys.push(r.y, r.y + r.h);
    centreXs.push(r.x + r.w / 2);
    centreYs.push(r.y + r.h / 2);
  }
  return {
    edges: { xs: dedupe(xs), ys: dedupe(ys) },
    centres: { xs: dedupe(centreXs), ys: dedupe(centreYs) },
  };
}

function dedupe(values: number[]): number[] {
  return [...new Set(values.map((v) => Math.round(v * 1e6) / 1e6))].toSorted((a, b) => a - b);
}

/**
 * The nearest guide within `threshold` of `value` that also lies in `[lo, hi]`,
 * or `null` when there is none. A guide outside the range is not a candidate at
 * all, so the next-nearest admissible one can win instead of the caller
 * snapping and then clamping to a position that is on no line.
 */
export function nearestGuide(
  value: number,
  guides: readonly number[],
  threshold: number,
  lo: number,
  hi: number,
): number | null {
  let best: number | null = null;
  let gap = threshold;
  for (const g of guides) {
    if (g < lo || g > hi) continue;
    const d = Math.abs(g - value);
    if (d < gap) {
      gap = d;
      best = g;
    }
  }
  return best;
}

/** One axis of a move: where the span starts now, and the line that holds it. */
interface SpanSnap {
  at: number;
  held: number | null;
}

/**
 * A span of `size` starting at `start`, pulled so that whichever of its three
 * anchors can land on a guide with the smallest correction does: its start or
 * end on an `edges` line, or its centre on a `centres` line. An anchor whose
 * pull would put the start outside `[0, hi]` is not a candidate.
 *
 * This function exists only for moves, and a move always has all three
 * anchors. A resize snaps single edges with `nearestGuide` and never comes
 * here, which is what keeps a box's centre out of a resize: there is no span
 * to anchor.
 *
 * Ties keep the earlier anchor (start, then end, then centre), so an existing
 * edge snap is never displaced by an equally close centre.
 *
 * Returns a START only, so nothing on the move path can recompute a size.
 */
function snapSpan(
  start: number,
  size: number,
  edges: readonly number[],
  centres: readonly number[],
  threshold: number,
  hi: number,
): SpanSnap {
  const anchors: [number, readonly number[]][] = [
    [0, edges],
    [size, edges],
    [size / 2, centres],
  ];
  let best: SpanSnap | null = null;
  let gap = Infinity;
  for (const [offset, pool] of anchors) {
    const g = nearestGuide(start + offset, pool, threshold, offset, hi + offset);
    if (g === null) continue;
    const correction = Math.abs(g - (start + offset));
    if (correction < gap) {
      gap = correction;
      best = { at: g - offset, held: g };
    }
  }
  return best ?? { at: start, held: null };
}

/**
 * The slot after being dragged by `delta` page-widths and page-heights, and
 * the lines that now hold it. Whichever of the slot's left edge, right edge or
 * centre is nearest a line in its pool pulls the whole slot, and the slot
 * never leaves the page.
 *
 * The size is kept BIT-IDENTICAL, by spreading `rect` rather than recomputing
 * it: Rust's `edit_label` (src-tauri/src/book/history.rs:72) calls a `SetSlot`
 * a move only when the size matches within 1e-9, and a width recomputed as
 * `right - left` drifts by ~5e-17 after a centre snap, which relabels the move
 * "Undo resize". Commit b896389 fixed exactly that once.
 */
export function slotMoved(
  rect: PreviewRect,
  delta: SlotDelta,
  guides: SnapGuides,
  threshold: number,
): SnappedSlot {
  const x = clamp(rect.x + delta.dx, 0, 1 - rect.w);
  const y = clamp(rect.y + delta.dy, 0, 1 - rect.h);
  const sx = snapSpan(x, rect.w, guides.edges.xs, guides.centres.xs, threshold, 1 - rect.w);
  const sy = snapSpan(y, rect.h, guides.edges.ys, guides.centres.ys, threshold, 1 - rect.h);
  return { rect: { ...rect, x: sx.at, y: sy.at }, held: { x: sx.held, y: sy.held } };
}

/**
 * The slot after one corner is dragged by `delta`, and the lines that hold its
 * moving edges. The opposite corner stays put; each moving edge snaps to the
 * `edges` pool within the room its floor and the page leave it; and the slot
 * can shrink no further than `minSize` each way. Free-form on purpose: a
 * slot's shape is the user's to choose; Rust re-crops the photo for whatever
 * shape results and refuses one that breaks a hard constraint.
 *
 * The slot's centre is never a candidate. A resize is the user steering an
 * EDGE, and pulling the centre onto a line would move that edge by twice the
 * correction, to somewhere the pointer is not.
 *
 * With `lockAspect` the slot is scaled by `slotScaled`, which sees no guides,
 * so the indicator goes dark the instant shift goes down.
 */
export function slotResized(
  rect: PreviewRect,
  corner: Corner,
  delta: SlotDelta,
  guides: SnapGuides,
  threshold: number,
  minSize: number,
  lockAspect = false,
): SnappedSlot {
  if (lockAspect) return { rect: slotScaled(rect, corner, delta, minSize), held: NO_HOLD };
  let left = rect.x;
  let right = rect.x + rect.w;
  let top = rect.y;
  let bottom = rect.y + rect.h;
  let heldX: number | null;
  let heldY: number | null;
  if (corner === "nw" || corner === "sw") {
    const v = clamp(left + delta.dx, 0, 1);
    heldX = nearestGuide(v, guides.edges.xs, threshold, 0, right - minSize);
    left = heldX ?? clamp(v, 0, right - minSize);
  } else {
    const v = clamp(right + delta.dx, 0, 1);
    heldX = nearestGuide(v, guides.edges.xs, threshold, left + minSize, 1);
    right = heldX ?? clamp(v, left + minSize, 1);
  }
  if (corner === "nw" || corner === "ne") {
    const v = clamp(top + delta.dy, 0, 1);
    heldY = nearestGuide(v, guides.edges.ys, threshold, 0, bottom - minSize);
    top = heldY ?? clamp(v, 0, bottom - minSize);
  } else {
    const v = clamp(bottom + delta.dy, 0, 1);
    heldY = nearestGuide(v, guides.edges.ys, threshold, top + minSize, 1);
    bottom = heldY ?? clamp(v, top + minSize, 1);
  }
  return { rect: { x: left, y: top, w: right - left, h: bottom - top }, held: { x: heldX, y: heldY } };
}

/**
 * The locked half of `slotResized`: the slot keeps its shape and only changes
 * size. Guides are ignored, because snapping one edge onto a line is exactly
 * what would break the ratio -- the lock is the constraint the user asked for,
 * so it wins.
 *
 * The ratio is held in NORMALISED space, which is also the printed one: both
 * `w` and `h` are scaled by the page's own inches, so `w/h` constant there is
 * `(w * pageWIn)/(h * pageHIn)` constant on paper.
 */
function slotScaled(rect: PreviewRect, corner: Corner, delta: SlotDelta, minSize: number): PreviewRect {
  const aspect = rect.w / rect.h;
  const growsRight = corner === "ne" || corner === "se";
  const growsDown = corner === "se" || corner === "sw";
  const dw = growsRight ? delta.dx : -delta.dx;
  const dh = growsDown ? delta.dy : -delta.dy;
  // Whichever axis the pointer pushed harder drives; the other follows it.
  const w = Math.abs(dw) >= Math.abs(dh * aspect) ? rect.w + dw : (rect.h + dh) * aspect;
  // Room from the fixed corner to the page edge, in each direction, as a width.
  const roomW = growsRight ? 1 - rect.x : rect.x + rect.w;
  const roomH = (growsDown ? 1 - rect.y : rect.y + rect.h) * aspect;
  const sized = clamp(w, Math.max(minSize, minSize * aspect), Math.max(roomW, roomH) === 0 ? 1 : Math.min(roomW, roomH));
  const h = sized / aspect;
  return {
    x: growsRight ? rect.x : rect.x + rect.w - sized,
    y: growsDown ? rect.y : rect.y + rect.h - h,
    w: sized,
    h,
  };
}

/** A hairline the indicator draws, positioned like any other guide box. */
export interface SnapLine {
  axis: "x" | "y";
  style: BoxStyle;
}

/**
 * The held lines as zero-thickness boxes: a held `x` is a full-height box of
 * zero width at `x`, a held `y` a full-width box of zero height at `y`. The
 * component gives each a one-sided border, which is the line. A zero-extent
 * box puts that hairline on the exact coordinate; a 1px-wide box would sit
 * half a pixel off it.
 */
export function snapLines(held: HeldLines): SnapLine[] {
  const lines: SnapLine[] = [];
  if (held.x !== null) lines.push({ axis: "x", style: rectStyle({ x: held.x, y: 0, w: 0, h: 1 }) });
  if (held.y !== null) lines.push({ axis: "y", style: rectStyle({ x: 0, y: held.y, w: 1, h: 0 }) });
  return lines;
}

/**
 * How far a pointer must travel before a press counts as a drag and not a
 * click. Deliberately generous: a real trackpad click carries several pixels
 * of tremor, and at 3px an ordinary click on a cover photo registered as a
 * tiny crop drag, so the picker never opened and the user clicked again. That
 * is the "I had to set the cover several times" bug. Synthetic clicks are
 * pixel-exact, which is why no test caught it.
 */
export const DRAG_THRESHOLD_PX = 10;

/**
 * The same question for a gesture where a press that never travels means
 * nothing at all -- moving or resizing a slot in layout mode, where releasing
 * without having moved discards the live rect and emits no edit.
 *
 * Ten pixels there bought nothing and cost every nudge smaller than ten
 * pixels, because the move applies the whole offset from the origin as soon as
 * the threshold is crossed: a slot could not be shifted by three pixels at
 * all, and a ten-pixel drag jumped to exactly ten. This floor is only large
 * enough to stop tremor during a press committing an edit the user did not
 * ask for and then has to undo.
 */
export const SLOT_DRAG_THRESHOLD_PX = 3;

/**
 * Whether a press now `dx`,`dy` from where it began has become a drag.
 *
 * Asked only while the pointer is moving. Once it answers yes the gesture is a
 * drag for good, carried to release on the handler's `moved` flag, so letting
 * go near where the press started commits the crop or rect on screen instead
 * of discarding it. Re-testing the net offset on release threw those away.
 */
export function pressIsDrag(dx: number, dy: number, threshold = DRAG_THRESHOLD_PX): boolean {
  return Math.hypot(dx, dy) >= threshold;
}

/** Undated photos sort after every dated one. */
function takenAt(row: CandidateRow): number {
  return row.photo.capturedAt ?? Number.POSITIVE_INFINITY;
}

/**
 * What the photo picker fills: a slot on a page, which trades places with a
 * photo already in the book, or one side of the cover, which takes a copy and
 * leaves the pages alone.
 */
export type PickTarget = { kind: "slot"; placement: PlacementRef } | { kind: "cover"; side: CoverSide };

export type CandidateFilter = "leftOut" | "inBook" | "all";
export type CandidateSort = "best" | "taken";

/** One tile of the "choose a photo for this slot" picker. */
export interface CandidateRow {
  /** Index into `BookLayout.photos`, what `replacePhoto` sends. */
  index: number;
  photo: PreviewPhoto;
  /** The window this slot would print of the photo. */
  crop: PreviewRect;
  refused: Rejection | null;
  /** Where the photo already is, or `null` when it was left out. */
  placedAt: PlacementRef | null;
  /** The photo the target holds now. */
  current: boolean;
  /** Placed on a locked opening, so a swap with it would be refused. Never set for the cover. */
  locked: boolean;
}

/**
 * The layout the picker draws, with photos imported during this session
 * merged in at the indices `import_photo` gave them.
 *
 * `import_photo` APPENDS to the project's photo list and hands back the new
 * index, but the dialog is holding the `BookLayout` it was opened with --
 * one photo shorter -- and nothing re-fetches it before the tiles are drawn.
 * `replaceCandidates` walks `photos`, so without this the imported photo has
 * no row, `chosen` matches no tile, and the import silently does nothing.
 * That is the whole feature failing, so it is pinned in `tests/preview.test.ts`
 * rather than left to the component.
 *
 * Keyed by index rather than appended blind: the layout prop can be replaced
 * mid-dialog by an edit made elsewhere, and a photo already counted in a
 * longer `photos` must land on itself, not past the end.
 */
export function withImportedPhotos(
  layout: BookLayout,
  imported: ReadonlyMap<number, PreviewPhoto>,
): BookLayout {
  if (imported.size === 0) return layout;
  const photos = [...layout.photos];
  for (const [index, photo] of imported) photos[index] = photo;
  return { ...layout, photos };
}

/**
 * The picker's tiles for the slot at `target`: every photo `candidates`
 * answers for (one per `layout.photos` entry, in that order), filtered and
 * sorted. Ties keep photo order, so the grid never reshuffles between two
 * equal scores.
 */
export function replaceCandidates(
  layout: BookLayout,
  candidates: readonly SlotCandidate[],
  target: PickTarget,
  filter: CandidateFilter,
  sort: CandidateSort,
): CandidateRow[] {
  const coverPhoto = target.kind === "cover" ? layout.cover[target.side].photo?.photoIndex : undefined;
  const placed = new Map<number, { at: PlacementRef; locked: boolean }>();
  toSpreads(layout.pages).forEach((opening, index) => {
    const locked = openingFor(layout, index).locked;
    for (const page of [opening.left, opening.right]) {
      if (!page) continue;
      for (const placement of page.placements) {
        placed.set(placement.photoIndex, { at: { page: page.number, z: placement.z }, locked });
      }
    }
  });

  const rows: CandidateRow[] = [];
  layout.photos.forEach((photo, index) => {
    const candidate = candidates[index];
    if (!candidate) return;
    const where = placed.get(index);
    if (filter === "leftOut" && where) return;
    if (filter === "inBook" && !where) return;
    rows.push({
      index,
      photo,
      crop: candidate.crop,
      refused: candidate.refused,
      placedAt: where?.at ?? null,
      current:
        target.kind === "cover"
          ? index === coverPhoto
          : where !== undefined && samePlacement(where.at, target.placement),
      locked: target.kind === "slot" && (where?.locked ?? false),
    });
  });

  return rows.toSorted((a, b) =>
    sort === "best"
      ? b.photo.aestheticPct - a.photo.aestheticPct || a.index - b.index
      : takenAt(a) - takenAt(b) || a.index - b.index,
  );
}

/** The target's printed shape, so every tile is framed the way the book frames it. */
export function pickAspect(layout: BookLayout, target: PickTarget): number {
  if (target.kind === "cover") return layout.cover.aspect;
  const page = layout.pages.find((p) => p.number === target.placement.page);
  const rect = page?.placements.find((p) => p.z === target.placement.z)?.slotRect;
  if (!rect) return 1;
  return (rect.w * layout.geometry.pageWIn) / (rect.h * layout.geometry.pageHIn);
}

/** The edit that puts photo `photo` on the target. */
export function pickEdit(target: PickTarget, photo: number): BookEdit {
  return target.kind === "cover"
    ? { kind: "setCoverPhoto", side: target.side, photo }
    : { kind: "replacePhoto", placement: target.placement, photo };
}

const REFUSALS: Record<Rejection, string> = {
  faceClipped: "A face would be cut off",
  faceInGutter: "A face would fall in the fold",
  faceInSafeMargin: "A face would sit in the trim margin",
  tooLowResolution: "Too low resolution to print this size",
};

export function refusalText(reason: Rejection, on: PickTarget["kind"]): string {
  if (on === "cover" && (reason === "faceInSafeMargin" || reason === "faceInGutter")) {
    return "A face would fold under the board or sit too near its edge";
  }
  return REFUSALS[reason];
}
